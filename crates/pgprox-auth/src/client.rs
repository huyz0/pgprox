//! The sidecar gRPC client, over a Unix domain socket.
//!
//! # Why a Unix socket
//!
//! The proxy and the sidecar share a pod, so there is no network hop and no TLS
//! between them. That is only correct because they share a pod: running the
//! sidecar off-pod would put every tenant's database password on the wire in
//! clear. See ADR 0003, and say so in any deployment documentation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pgprox_core::auth::{
    AuthError, AuthRequest, Backend, ClaimSet, CredentialResolver, Grant, PoolHints, PoolMode,
    TlsMode,
};
use pgprox_core::error::AuthRejection;
use pgprox_core::ids::{ServerId, TenantId};
use pgprox_core::secret::SecretString;
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};

/// The generated proto types and service stubs.
///
/// Never hand-edited, and excluded from the coverage gate: asserting on prost's
/// output would test prost.
// The workspace lints reach in here through include_proto!, but this code is
// generated and standards/behavior.md forbids editing it, so linting it can only
// produce warnings nobody may act on.
#[allow(clippy::all, clippy::pedantic, missing_docs)]
pub mod pb {
    tonic::include_proto!("pgprox.auth.v1");
}

/// How the client is configured.
#[derive(Clone, Debug)]
pub struct SidecarConfig {
    /// Path to the sidecar's Unix socket.
    pub socket_path: PathBuf,
    /// How long a single resolve may take.
    ///
    /// This sits on the connection path, so a slow sidecar becomes slow
    /// connection establishment for every tenant. Failing fast and letting the
    /// client retry is better than holding the connection open indefinitely.
    pub timeout: Duration,
}

impl SidecarConfig {
    /// Points at a socket with the default timeout.
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout: Duration::from_secs(5),
        }
    }
}

/// Resolves credentials by asking the sidecar.
#[derive(Clone, Debug)]
pub struct SidecarResolver {
    client: pb::credential_resolver_client::CredentialResolverClient<Channel>,
    timeout: Duration,
}

impl SidecarResolver {
    /// Connects to the sidecar.
    ///
    /// # Errors
    ///
    /// Fails if the socket cannot be reached.
    pub async fn connect(config: &SidecarConfig) -> Result<Self, AuthError> {
        let path = Arc::new(config.socket_path.clone());

        // The URI is ignored for a Unix socket but tonic requires a well-formed
        // one, so this is a placeholder rather than a real address.
        let channel = Endpoint::try_from("http://[::]:50051")
            .map_err(|e| AuthError::Unavailable {
                reason: format!("invalid endpoint: {e}"),
            })?
            .connect_timeout(config.timeout)
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let path = Arc::clone(&path);
                async move {
                    let stream = UnixStream::connect(path.as_path()).await?;
                    Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
                }
            }))
            .await
            .map_err(|e| AuthError::Unavailable {
                reason: format!("could not connect to the sidecar socket: {e}"),
            })?;

        Ok(Self {
            client: pb::credential_resolver_client::CredentialResolverClient::new(channel),
            timeout: config.timeout,
        })
    }

    /// The socket path this resolver was built for, for diagnostics.
    #[must_use]
    pub fn describe(path: &Path) -> String {
        format!("sidecar at {}", path.display())
    }
}

#[async_trait::async_trait]
impl CredentialResolver for SidecarResolver {
    async fn resolve(&self, request: AuthRequest) -> Result<Grant, AuthError> {
        let proto = pb::ResolveRequest {
            token: request.token.expose().to_owned(),
            startup_database: request.startup_database.clone(),
            startup_user: request.startup_user.clone(),
            client_address: request.client_addr.to_string(),
        };

        let mut client = self.client.clone();
        let mut rpc = tonic::Request::new(proto);
        rpc.set_timeout(self.timeout);

        let response = client.resolve(rpc).await.map_err(|s| map_status(&s))?;
        grant_from_proto(response.into_inner())
    }
}

/// Maps a gRPC status onto the auth taxonomy.
///
/// The distinction that matters: a refusal is the sidecar answering, and
/// anything else is the sidecar failing. Only the first is worth caching.
fn map_status(status: &tonic::Status) -> AuthError {
    use tonic::Code;
    match status.code() {
        Code::Unauthenticated => AuthError::Refused(AuthRejection::TokenRejected),
        Code::PermissionDenied => AuthError::Refused(AuthRejection::NotPermitted),
        Code::InvalidArgument => AuthError::Refused(AuthRejection::Malformed),
        Code::DeadlineExceeded => AuthError::Unavailable {
            reason: "sidecar did not answer within the timeout".into(),
        },
        _ => AuthError::Unavailable {
            reason: format!("sidecar returned {}: {}", status.code(), status.message()),
        },
    }
}

/// Converts a sidecar response into a [`Grant`].
fn grant_from_proto(response: pb::ResolveResponse) -> Result<Grant, AuthError> {
    let primary = response
        .primary
        .ok_or_else(|| AuthError::Malformed {
            reason: "response has no primary backend".into(),
        })
        .and_then(backend_from_proto)?;

    let replicas = response
        .replicas
        .into_iter()
        .map(backend_from_proto)
        .collect::<Result<Vec<_>, _>>()?;

    if response.tenant_id.is_empty() {
        return Err(AuthError::Malformed {
            reason: "response has no tenant id".into(),
        });
    }

    Ok(Grant {
        tenant: TenantId::new(&response.tenant_id),
        primary,
        replicas,
        pool: response.pool.map(pool_hints_from_proto).unwrap_or_default(),
        ttl: Duration::from_secs(u64::from(response.ttl_seconds)),
        claims: response.claims.map(claims_from_proto).unwrap_or_default(),
    })
}

fn backend_from_proto(backend: pb::Backend) -> Result<Backend, AuthError> {
    if backend.host.is_empty() {
        return Err(AuthError::Malformed {
            reason: "backend has no host".into(),
        });
    }
    let port = u16::try_from(backend.port).map_err(|_| AuthError::Malformed {
        reason: format!("backend port {} is out of range", backend.port),
    })?;

    Ok(Backend {
        server: ServerId::new(&backend.host, port),
        database: Arc::from(backend.database.as_str()),
        user: Arc::from(backend.user.as_str()),
        // The one place a password crosses into this process. It becomes a
        // SecretString immediately, so nothing downstream can print it.
        password: SecretString::new(backend.password),
        tls: match pb::TlsMode::try_from(backend.tls) {
            // Unspecified means verified. A sidecar that omits the field must
            // not accidentally turn TLS off.
            Ok(pb::TlsMode::Disabled) => TlsMode::Disabled,
            _ => TlsMode::Verified,
        },
    })
}

fn pool_hints_from_proto(hints: pb::PoolHints) -> PoolHints {
    PoolHints {
        max_upstream: (hints.max_upstream > 0).then_some(hints.max_upstream),
        mode: match pb::PoolMode::try_from(hints.mode) {
            // Unspecified means transaction pooling, the default that makes the
            // connection ratio work.
            Ok(pb::PoolMode::Session) => PoolMode::Session,
            _ => PoolMode::Transaction,
        },
        statement_timeout: (hints.statement_timeout_ms > 0)
            .then(|| Duration::from_millis(u64::from(hints.statement_timeout_ms))),
    }
}

fn claims_from_proto(claims: pb::ClaimSet) -> ClaimSet {
    ClaimSet {
        subject: (!claims.subject.is_empty()).then_some(claims.subject),
        // Zero means absent, not the Unix epoch. Treating it as an expiry would
        // make every token from a sidecar that omits the field look expired.
        expires_at: unix_to_system_time(claims.expires_at_unix),
        issued_at: unix_to_system_time(claims.issued_at_unix),
    }
}

fn unix_to_system_time(seconds: i64) -> Option<std::time::SystemTime> {
    if seconds <= 0 {
        return None;
    }
    u64::try_from(seconds)
        .ok()
        .map(|s| std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(s))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn proto_backend() -> pb::Backend {
        pb::Backend {
            host: "db-1.internal".into(),
            port: 5432,
            database: "tenant_acme".into(),
            user: "acme_app".into(),
            password: "hunter2".into(),
            tls: pb::TlsMode::Verified as i32,
        }
    }

    fn proto_response() -> pb::ResolveResponse {
        pb::ResolveResponse {
            tenant_id: "acme".into(),
            primary: Some(proto_backend()),
            replicas: vec![],
            ttl_seconds: 60,
            pool: None,
            claims: None,
        }
    }

    #[test]
    fn a_response_converts_to_a_grant() {
        let grant = grant_from_proto(proto_response()).unwrap();
        assert_eq!(grant.tenant, TenantId::new("acme"));
        assert_eq!(grant.primary.server, ServerId::new("db-1.internal", 5432));
        assert_eq!(grant.ttl, Duration::from_secs(60));
    }

    #[test]
    fn the_password_arrives_as_a_secret() {
        // The one place a tenant database password crosses into this process.
        let grant = grant_from_proto(proto_response()).unwrap();
        assert_eq!(grant.primary.password.expose(), "hunter2");
        assert!(!format!("{:?}", grant.primary).contains("hunter2"));
        assert!(!format!("{grant:#?}").contains("hunter2"));
    }

    #[test]
    fn a_response_with_no_primary_is_malformed() {
        let response = pb::ResolveResponse {
            primary: None,
            ..proto_response()
        };
        let err = grant_from_proto(response).unwrap_err();
        assert!(matches!(err, AuthError::Malformed { .. }), "{err:?}");
        assert!(err.to_string().contains("no primary"));
    }

    #[test]
    fn a_response_with_no_tenant_is_malformed() {
        let response = pb::ResolveResponse {
            tenant_id: String::new(),
            ..proto_response()
        };
        assert!(matches!(
            grant_from_proto(response).unwrap_err(),
            AuthError::Malformed { .. }
        ));
    }

    #[test]
    fn a_backend_with_no_host_is_malformed() {
        let response = pb::ResolveResponse {
            primary: Some(pb::Backend {
                host: String::new(),
                ..proto_backend()
            }),
            ..proto_response()
        };
        assert!(matches!(
            grant_from_proto(response).unwrap_err(),
            AuthError::Malformed { .. }
        ));
    }

    #[test]
    fn an_out_of_range_port_is_malformed() {
        // proto3 has no u16, so the field is u32 and the range check lives here.
        let response = pb::ResolveResponse {
            primary: Some(pb::Backend {
                port: 70_000,
                ..proto_backend()
            }),
            ..proto_response()
        };
        let err = grant_from_proto(response).unwrap_err();
        assert!(err.to_string().contains("70000"), "{err}");
    }

    #[test]
    fn an_unspecified_tls_mode_means_verified() {
        // The default that stops a sidecar omitting the field from silently
        // disabling TLS.
        for raw in [pb::TlsMode::Unspecified as i32, 999] {
            let response = pb::ResolveResponse {
                primary: Some(pb::Backend {
                    tls: raw,
                    ..proto_backend()
                }),
                ..proto_response()
            };
            let grant = grant_from_proto(response).unwrap();
            assert_eq!(grant.primary.tls, TlsMode::Verified, "raw {raw}");
        }
    }

    #[test]
    fn tls_can_be_disabled_only_explicitly() {
        let response = pb::ResolveResponse {
            primary: Some(pb::Backend {
                tls: pb::TlsMode::Disabled as i32,
                ..proto_backend()
            }),
            ..proto_response()
        };
        assert_eq!(
            grant_from_proto(response).unwrap().primary.tls,
            TlsMode::Disabled
        );
    }

    #[test]
    fn an_unspecified_pool_mode_means_transaction() {
        let hints = pool_hints_from_proto(pb::PoolHints {
            max_upstream: 0,
            mode: pb::PoolMode::Unspecified as i32,
            statement_timeout_ms: 0,
        });
        assert_eq!(hints.mode, PoolMode::Transaction);
        // Zero means unset, not a limit of zero, which would refuse every
        // connection.
        assert!(hints.max_upstream.is_none());
        assert!(hints.statement_timeout.is_none());
    }

    #[test]
    fn pool_hints_carry_real_values() {
        let hints = pool_hints_from_proto(pb::PoolHints {
            max_upstream: 32,
            mode: pb::PoolMode::Session as i32,
            statement_timeout_ms: 1_500,
        });
        assert_eq!(hints.max_upstream, Some(32));
        assert_eq!(hints.mode, PoolMode::Session);
        assert_eq!(hints.statement_timeout, Some(Duration::from_millis(1_500)));
    }

    #[test]
    fn a_zero_timestamp_means_absent_rather_than_the_epoch() {
        // Treating zero as an expiry would make every token from a sidecar that
        // omits the field look already expired.
        let claims = claims_from_proto(pb::ClaimSet {
            subject: String::new(),
            expires_at_unix: 0,
            issued_at_unix: 0,
        });
        assert!(claims.expires_at.is_none());
        assert!(claims.issued_at.is_none());
        assert!(claims.subject.is_none());

        assert!(unix_to_system_time(-5).is_none());
        assert!(unix_to_system_time(1).is_some());
    }

    #[test]
    fn claims_carry_real_values() {
        let claims = claims_from_proto(pb::ClaimSet {
            subject: "user-1".into(),
            expires_at_unix: 1_700_000_000,
            issued_at_unix: 1_699_999_000,
        });
        assert_eq!(claims.subject.as_deref(), Some("user-1"));
        assert!(claims.expires_at.is_some());
        assert!(claims.issued_at.is_some());
    }

    #[test]
    fn replicas_convert_and_a_bad_one_fails_the_whole_response() {
        let response = pb::ResolveResponse {
            replicas: vec![proto_backend(), proto_backend()],
            ..proto_response()
        };
        assert_eq!(grant_from_proto(response).unwrap().replicas.len(), 2);

        let bad = pb::ResolveResponse {
            replicas: vec![
                proto_backend(),
                pb::Backend {
                    host: String::new(),
                    ..proto_backend()
                },
            ],
            ..proto_response()
        };
        assert!(grant_from_proto(bad).is_err(), "a bad replica was accepted");
    }

    #[test]
    fn grpc_status_codes_split_refusals_from_failures() {
        // Only a refusal is the sidecar answering, and only a refusal is worth
        // caching. Everything else is the sidecar failing.
        use tonic::{Code, Status};

        for (code, expect_refused) in [
            (Code::Unauthenticated, true),
            (Code::PermissionDenied, true),
            (Code::InvalidArgument, true),
            (Code::DeadlineExceeded, false),
            (Code::Unavailable, false),
            (Code::Internal, false),
            (Code::Unknown, false),
        ] {
            let err = map_status(&Status::new(code, "detail"));
            assert_eq!(
                matches!(err, AuthError::Refused(_)),
                expect_refused,
                "{code:?} mapped to {err:?}"
            );
        }
    }

    #[test]
    fn a_failure_status_keeps_its_detail_for_operators() {
        let err = map_status(&tonic::Status::new(
            tonic::Code::Internal,
            "backend store unreachable",
        ));
        assert!(err.to_string().contains("backend store unreachable"));
    }

    #[test]
    fn describe_names_the_socket() {
        assert!(
            SidecarResolver::describe(Path::new("/var/run/sidecar.sock"))
                .contains("/var/run/sidecar.sock")
        );
    }

    #[tokio::test]
    async fn connecting_to_a_missing_socket_reports_unavailable() {
        let config = SidecarConfig::new("/nonexistent/pgprox/sidecar.sock");
        let err = SidecarResolver::connect(&config).await.unwrap_err();
        assert!(matches!(err, AuthError::Unavailable { .. }), "{err:?}");
    }

    #[test]
    fn the_default_timeout_is_short_enough_to_fail_fast() {
        // This sits on the connection path, so a slow sidecar becomes slow
        // connection establishment for every tenant.
        let config = SidecarConfig::new("/tmp/x.sock");
        assert!(config.timeout <= Duration::from_secs(10));
    }
}
