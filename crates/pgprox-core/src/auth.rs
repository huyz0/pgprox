//! Authentication data transfer objects.
//!
//! A client authenticates with a JWT in the password field. The sidecar
//! validates it and returns a [`Grant`]: where the tenant's database actually
//! lives, and the credentials to reach it.
//!
//! Everything here holds credentials for a tenant database, so nothing in this
//! module derives `Debug`.

use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::ids::{PoolKey, ServerId, TenantId};
use crate::secret::SecretString;

/// How to secure a connection to an upstream server.
///
/// There is deliberately no "verify nothing" variant. A flag that skips
/// certificate verification always ends up set in production.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum TlsMode {
    /// Plaintext. Only valid inside a trusted network boundary.
    Disabled,
    /// TLS with full chain verification against the configured CA.
    #[default]
    Verified,
}

/// How a tenant's connections are pooled.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum PoolMode {
    /// Multiplex at transaction boundaries, pinning when a session-scoped
    /// feature is used. The default, and what makes the connection ratio work.
    #[default]
    Transaction,
    /// One upstream connection per client for its lifetime. Fully transparent,
    /// and gives up almost all of the multiplexing benefit.
    Session,
}

/// Where a tenant's database actually lives, and how to connect to it.
///
/// No derived `Debug`: the hand-written one prints everything except the
/// password.
#[derive(Clone)]
pub struct Backend {
    /// The upstream server, which is the unit the connection cap applies to.
    pub server: ServerId,
    /// The database name.
    pub database: Arc<str>,
    /// The role to connect as.
    pub user: Arc<str>,
    /// That role's password.
    pub password: SecretString,
    /// How to secure the connection.
    pub tls: TlsMode,
}

impl Backend {
    /// The pool this backend's connections belong to.
    #[must_use]
    pub fn pool_key(&self) -> PoolKey {
        PoolKey {
            server: self.server.clone(),
            database: Arc::clone(&self.database),
            user: Arc::clone(&self.user),
        }
    }
}

impl fmt::Debug for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Backend")
            .field("server", &self.server)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &self.password)
            .field("tls", &self.tls)
            .finish()
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}@{}", self.server, self.database, self.user)
    }
}

/// Per-tenant pool tuning, supplied by the sidecar alongside the credentials.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct PoolHints {
    /// Cap on upstream connections for this tenant, if the sidecar sets one.
    pub max_upstream: Option<u32>,
    /// Whether to multiplex or pin for the session's lifetime.
    pub mode: PoolMode,
    /// `statement_timeout` to apply to this tenant's upstream connections.
    pub statement_timeout: Option<Duration>,
}

/// Claims parsed from the token.
///
/// Parsed for policy and logging only. The sidecar owns validation, and this
/// crate does not implement a second validator, because two validators that
/// disagree about token validity is a vulnerability rather than redundancy.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ClaimSet {
    /// The `sub` claim, if present.
    pub subject: Option<String>,
    /// The `exp` claim, if present.
    pub expires_at: Option<SystemTime>,
    /// The `iat` claim, if present.
    pub issued_at: Option<SystemTime>,
}

/// A request to resolve a client's credentials.
///
/// No derived `Debug`: this holds the raw token.
#[derive(Clone)]
pub struct AuthRequest {
    /// The JWT, as sent in the password field.
    pub token: SecretString,
    /// The database the client asked for in its startup message.
    pub startup_database: String,
    /// The user the client asked for in its startup message.
    pub startup_user: String,
    /// Where the client connected from, for policy and audit.
    pub client_addr: IpAddr,
}

impl fmt::Debug for AuthRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthRequest")
            .field("token", &self.token)
            .field("startup_database", &self.startup_database)
            .field("startup_user", &self.startup_user)
            .field("client_addr", &self.client_addr)
            .finish()
    }
}

/// What the sidecar returns for a valid token.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Grant {
    /// The tenant this token belongs to.
    pub tenant: TenantId,
    /// The primary, which serves every write and any read that cannot be
    /// proven safe on a replica.
    pub primary: Backend,
    /// Read replicas, in no particular order.
    pub replicas: Vec<Backend>,
    /// Per-tenant pool tuning.
    pub pool: PoolHints,
    /// How long the sidecar says this grant may be cached.
    pub ttl: Duration,
    /// Claims parsed from the token.
    pub claims: ClaimSet,
}

impl Grant {
    /// How long this grant may actually be cached.
    ///
    /// The earliest of three limits: what the sidecar said, when the token
    /// expires, and the locally configured maximum. Taking anything longer
    /// would let a revoked or expired token keep working because the cache had
    /// a longer opinion than the issuer did.
    ///
    /// Returns [`Duration::ZERO`] for a token that has already expired, so a
    /// caller that only checks the TTL still refuses to cache it.
    #[must_use]
    pub fn effective_ttl(&self, now: SystemTime, configured_cap: Duration) -> Duration {
        let mut ttl = self.ttl.min(configured_cap);
        if let Some(expires_at) = self.claims.expires_at {
            let until_expiry = expires_at.duration_since(now).unwrap_or(Duration::ZERO);
            ttl = ttl.min(until_expiry);
        }
        ttl
    }

    /// Whether the token backing this grant has already expired.
    #[must_use]
    pub fn is_expired(&self, now: SystemTime) -> bool {
        self.claims
            .expires_at
            .is_some_and(|expires_at| expires_at <= now)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const PASSWORD: &str = "tenant-db-password-do-not-log";

    fn backend() -> Backend {
        Backend {
            server: ServerId::new("db-1.internal", 5432),
            database: Arc::from("tenant_acme"),
            user: Arc::from("acme_app"),
            password: SecretString::new(PASSWORD),
            tls: TlsMode::Verified,
        }
    }

    fn grant(ttl: Duration, expires_at: Option<SystemTime>) -> Grant {
        Grant {
            tenant: TenantId::new("acme"),
            primary: backend(),
            replicas: vec![],
            pool: PoolHints::default(),
            ttl,
            claims: ClaimSet {
                subject: Some("user-1".into()),
                expires_at,
                issued_at: None,
            },
        }
    }

    #[test]
    fn backend_debug_shows_everything_except_the_password() {
        let rendered = format!("{:?}", backend());
        assert!(!rendered.contains(PASSWORD), "password leaked: {rendered}");
        assert!(rendered.contains("db-1.internal"), "lost the host");
        assert!(rendered.contains("tenant_acme"), "lost the database");
        assert!(rendered.contains("acme_app"), "lost the user");
        assert!(rendered.contains("[redacted]"), "password field missing");
    }

    #[test]
    fn backend_display_never_includes_the_password() {
        let b = backend();
        assert_eq!(b.to_string(), "db-1.internal:5432/tenant_acme@acme_app");
        assert!(!b.to_string().contains(PASSWORD));
    }

    #[test]
    fn a_grant_containing_backends_still_hides_passwords() {
        // The realistic leak: someone logs the whole grant.
        let rendered = format!("{:#?}", grant(Duration::from_secs(60), None));
        assert!(!rendered.contains(PASSWORD), "password leaked: {rendered}");
        assert!(rendered.contains("acme"), "tenant should still be visible");
    }

    #[test]
    fn auth_request_debug_hides_the_token() {
        let req = AuthRequest {
            token: SecretString::new("eyJhbGciOiJSUzI1NiJ9.secret.sig"),
            startup_database: "tenant_acme".into(),
            startup_user: "acme_app".into(),
            client_addr: "10.0.0.7".parse().unwrap(),
        };
        let rendered = format!("{req:?}");
        assert!(!rendered.contains("eyJhbGci"), "token leaked: {rendered}");
        assert!(rendered.contains("10.0.0.7"), "lost the client address");
    }

    #[test]
    fn backend_yields_its_pool_key() {
        let b = backend();
        let key = b.pool_key();
        assert_eq!(key.server, b.server);
        assert_eq!(&*key.database, "tenant_acme");
        assert_eq!(&*key.user, "acme_app");
    }

    #[test]
    fn ttl_is_clamped_by_the_sidecar_value() {
        let now = SystemTime::UNIX_EPOCH;
        let g = grant(Duration::from_secs(30), None);
        assert_eq!(
            g.effective_ttl(now, Duration::from_secs(300)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn ttl_is_clamped_by_the_configured_cap() {
        let now = SystemTime::UNIX_EPOCH;
        let g = grant(Duration::from_secs(3600), None);
        assert_eq!(
            g.effective_ttl(now, Duration::from_secs(60)),
            Duration::from_secs(60),
            "local cap must win over a generous sidecar TTL"
        );
    }

    #[test]
    fn ttl_is_clamped_by_token_expiry() {
        // The case that matters: a token expiring sooner than either limit must
        // not keep working because the cache had a longer opinion.
        let now = SystemTime::UNIX_EPOCH;
        let g = grant(
            Duration::from_secs(3600),
            Some(now + Duration::from_secs(10)),
        );
        assert_eq!(
            g.effective_ttl(now, Duration::from_secs(300)),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn an_already_expired_token_yields_a_zero_ttl() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let g = grant(
            Duration::from_secs(3600),
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(500)),
        );
        assert_eq!(
            g.effective_ttl(now, Duration::from_secs(300)),
            Duration::ZERO
        );
        assert!(g.is_expired(now));
    }

    #[test]
    fn expiry_exactly_now_counts_as_expired() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let g = grant(Duration::from_secs(60), Some(now));
        assert!(g.is_expired(now), "an expiry of exactly now must not pass");
        assert_eq!(
            g.effective_ttl(now, Duration::from_secs(60)),
            Duration::ZERO
        );
    }

    #[test]
    fn a_token_without_an_expiry_claim_is_not_expired() {
        let g = grant(Duration::from_secs(60), None);
        assert!(!g.is_expired(SystemTime::UNIX_EPOCH));
    }

    #[test]
    fn defaults_are_the_safe_choices() {
        // A forgotten field must not silently disable TLS or session pooling.
        assert_eq!(TlsMode::default(), TlsMode::Verified);
        assert_eq!(PoolMode::default(), PoolMode::Transaction);
        let hints = PoolHints::default();
        assert!(hints.max_upstream.is_none());
        assert!(hints.statement_timeout.is_none());
        assert!(ClaimSet::default().subject.is_none());
    }
}
