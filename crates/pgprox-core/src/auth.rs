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
///
/// Not `#[non_exhaustive]`: `pgprox-auth` builds these from the sidecar's
/// response, and the attribute would make them unconstructable outside this
/// crate. Adding a field is therefore a breaking change here, which is the
/// trade the `contract-change` skill exists to manage.
#[derive(Clone, Debug, Default)]
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

/// Why credential resolution failed.
#[derive(Clone, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// The sidecar refused the token.
    #[error("token refused: {0:?}")]
    Refused(crate::error::AuthRejection),
    /// The sidecar could not be reached.
    #[error("credential sidecar unavailable: {reason}")]
    Unavailable {
        /// Operator-facing detail. Never shown to a client.
        reason: String,
    },
    /// The sidecar answered, but the answer did not make sense.
    #[error("credential sidecar returned an unusable grant: {reason}")]
    Malformed {
        /// What was wrong with the response.
        reason: String,
    },
}

impl From<AuthError> for crate::error::ClientError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::Refused(reason) => Self::AuthRefused(reason),
            // A malformed grant is our problem, not the client's, and from the
            // client's side it is indistinguishable from the sidecar being
            // down. Both map to the same wire error.
            AuthError::Unavailable { .. } | AuthError::Malformed { .. } => Self::SidecarUnavailable,
        }
    }
}

/// Resolves a client's token into the credentials for its database.
///
/// Implemented by `pgprox-auth` against the sidecar. The sidecar owns token
/// validation; implementations here must not add a second validator.
#[async_trait::async_trait]
pub trait CredentialResolver: Send + Sync + fmt::Debug {
    /// Resolves a token, or explains why it cannot be.
    async fn resolve(&self, request: AuthRequest) -> Result<Grant, AuthError>;
}

#[async_trait::async_trait]
impl<T: CredentialResolver + ?Sized> CredentialResolver for Arc<T> {
    async fn resolve(&self, request: AuthRequest) -> Result<Grant, AuthError> {
        (**self).resolve(request).await
    }
}

/// An in-memory [`CredentialResolver`] for tests.
///
/// Behaves like the real thing rather than recording calls: it resolves tokens
/// it knows, refuses ones it does not, and can be told to fail so callers'
/// error paths are reachable. The call counter exists so singleflight and cache
/// behaviour can be asserted, which is the property most worth testing in
/// anything that wraps a resolver.
#[cfg(any(test, feature = "test-fakes"))]
#[derive(Debug, Default)]
pub struct FakeCredentialResolver {
    grants: std::sync::Mutex<std::collections::HashMap<String, Grant>>,
    calls: std::sync::atomic::AtomicUsize,
    unavailable: std::sync::atomic::AtomicBool,
}

#[cfg(any(test, feature = "test-fakes"))]
impl FakeCredentialResolver {
    /// An empty resolver that refuses everything.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Teaches the resolver a token.
    #[must_use]
    pub fn with_grant(self, token: impl Into<String>, grant: Grant) -> Self {
        self.insert(token, grant);
        self
    }

    /// Teaches the resolver a token after construction.
    pub fn insert(&self, token: impl Into<String>, grant: Grant) {
        self.lock_grants().insert(token.into(), grant);
    }

    /// Forgets a token, so a previously valid one starts being refused. Models
    /// revocation.
    pub fn revoke(&self, token: &str) {
        self.lock_grants().remove(token);
    }

    /// Makes every subsequent call fail as if the sidecar were down.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable
            .store(unavailable, std::sync::atomic::Ordering::SeqCst);
    }

    /// How many times [`CredentialResolver::resolve`] has been called.
    ///
    /// Use this to assert that a cache actually caches and that a singleflight
    /// actually collapses concurrent lookups.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn lock_grants(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, Grant>> {
        self.grants
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(any(test, feature = "test-fakes"))]
#[async_trait::async_trait]
impl CredentialResolver for FakeCredentialResolver {
    async fn resolve(&self, request: AuthRequest) -> Result<Grant, AuthError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AuthError::Unavailable {
                reason: "fake resolver set unavailable".into(),
            });
        }

        self.lock_grants()
            .get(request.token.expose())
            .cloned()
            .ok_or(AuthError::Refused(
                crate::error::AuthRejection::TokenRejected,
            ))
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

    fn auth_request(token: &str) -> AuthRequest {
        AuthRequest {
            token: SecretString::new(token),
            startup_database: "tenant_acme".into(),
            startup_user: "acme_app".into(),
            client_addr: "10.0.0.7".parse().unwrap(),
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

    #[tokio::test]
    async fn fake_resolver_resolves_a_known_token() {
        let resolver = FakeCredentialResolver::new()
            .with_grant("good-token", grant(Duration::from_secs(60), None));

        let g = resolver.resolve(auth_request("good-token")).await.unwrap();
        assert_eq!(g.tenant, TenantId::new("acme"));
        assert_eq!(resolver.call_count(), 1);
    }

    #[tokio::test]
    async fn fake_resolver_refuses_an_unknown_token() {
        // The fake must actually refuse, not record a call and return success.
        // A mock that only records lets a caller's error path go untested.
        let resolver = FakeCredentialResolver::new();
        let err = resolver.resolve(auth_request("nope")).await.unwrap_err();
        assert!(matches!(err, AuthError::Refused(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn fake_resolver_models_revocation() {
        let resolver =
            FakeCredentialResolver::new().with_grant("tok", grant(Duration::from_secs(60), None));
        assert!(resolver.resolve(auth_request("tok")).await.is_ok());

        resolver.revoke("tok");
        assert!(
            resolver.resolve(auth_request("tok")).await.is_err(),
            "a revoked token must stop working"
        );
    }

    #[tokio::test]
    async fn fake_resolver_can_be_made_unavailable() {
        let resolver =
            FakeCredentialResolver::new().with_grant("tok", grant(Duration::from_secs(60), None));
        resolver.set_unavailable(true);

        let err = resolver.resolve(auth_request("tok")).await.unwrap_err();
        assert!(matches!(err, AuthError::Unavailable { .. }), "got {err:?}");

        resolver.set_unavailable(false);
        assert!(resolver.resolve(auth_request("tok")).await.is_ok());
    }

    #[tokio::test]
    async fn call_count_makes_caching_testable() {
        // This counter is why the fake exists in this shape: a cache or a
        // singleflight wrapped around a resolver is only testable if the number
        // of underlying calls is observable.
        let resolver =
            FakeCredentialResolver::new().with_grant("tok", grant(Duration::from_secs(60), None));

        for _ in 0..3 {
            resolver.resolve(auth_request("tok")).await.unwrap();
        }
        assert_eq!(resolver.call_count(), 3);
    }

    #[tokio::test]
    async fn resolver_works_through_an_arc() {
        let resolver: Arc<dyn CredentialResolver> = Arc::new(
            FakeCredentialResolver::new().with_grant("tok", grant(Duration::from_secs(60), None)),
        );
        assert!(resolver.resolve(auth_request("tok")).await.is_ok());
    }

    #[test]
    fn auth_errors_map_to_the_right_client_error() {
        use crate::error::{AuthRejection, ClientError};

        let refused: ClientError = AuthError::Refused(AuthRejection::TokenExpired).into();
        assert!(matches!(refused, ClientError::AuthRefused(_)));

        // A malformed grant is our bug, not the client's, and is
        // indistinguishable from the sidecar being down from their side.
        let malformed: ClientError = AuthError::Malformed {
            reason: "no primary backend".into(),
        }
        .into();
        assert!(matches!(malformed, ClientError::SidecarUnavailable));

        let unavailable: ClientError = AuthError::Unavailable {
            reason: "connection refused".into(),
        }
        .into();
        assert!(matches!(unavailable, ClientError::SidecarUnavailable));
    }

    #[test]
    fn auth_error_detail_stays_out_of_the_client_message() {
        use crate::error::ClientError;
        let client: ClientError = AuthError::Unavailable {
            reason: "dial unix /var/run/sidecar.sock: connection refused".into(),
        }
        .into();
        assert!(!client.client_message().contains("sidecar.sock"));
    }
}
