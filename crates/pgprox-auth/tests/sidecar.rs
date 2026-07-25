//! The client against the mock sidecar, over a real Unix socket.
//!
//! Gated behind the `integration` feature so tier 1 never spawns a process.

#![cfg(feature = "integration")]
// An integration test is a separate crate target, so the workspace lints that
// ban these in production code apply here too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use pgprox_auth::cache::{CacheConfig, CachingResolver};
use pgprox_auth::client::{SidecarConfig, SidecarResolver};
use pgprox_core::auth::{AuthError, AuthRequest, CredentialResolver};
use pgprox_core::clock::{Clock, FakeClock};
use pgprox_core::error::AuthRejection;
use pgprox_core::secret::SecretString;

/// The mock sidecar process and its socket, cleaned up on drop.
struct Sidecar {
    child: Child,
    path: std::path::PathBuf,
}

impl Sidecar {
    fn start() -> Self {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        // A per-process socket name: nextest runs each test in its own process,
        // and a shared path would collide exactly as the Postgres container
        // name did in M1.
        let path = std::env::temp_dir().join(format!("pgprox-mock-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);

        // Built by the same cargo invocation that built this test, because the
        // binary is feature-gated on `integration`. Shelling out to cargo here
        // instead made every test contend on the build lock.
        let binary = format!("{root}/target/debug/mock_sidecar");
        assert!(
            std::path::Path::new(&binary).exists(),
            "{binary} is missing; run with --features integration"
        );

        let child = Command::new(&binary)
            .arg(&path)
            .stdout(Stdio::piped())
            .spawn()
            .expect("could not start the mock sidecar");

        let sidecar = Self { child, path };
        sidecar.wait_ready();
        sidecar
    }

    /// Waits for the socket to accept a connection.
    ///
    /// Polling rather than sleeping, for the same reason the Postgres probe in
    /// M1 does: a fixed sleep is how a suite becomes red under load.
    fn wait_ready(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if self.path.exists() && std::os::unix::net::UnixStream::connect(&self.path).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "mock sidecar did not start listening on {}",
            self.path.display()
        );
    }

    fn config(&self) -> SidecarConfig {
        SidecarConfig {
            socket_path: self.path.clone(),
            timeout: Duration::from_secs(3),
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn request(token: &str) -> AuthRequest {
    AuthRequest {
        token: SecretString::new(token),
        startup_database: "tenant_acme".into(),
        startup_user: "acme_app".into(),
        client_addr: "10.0.0.7".parse().unwrap(),
    }
}

#[tokio::test]
async fn resolves_a_grant_over_a_real_socket() {
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&sidecar.config()).await.unwrap();

    let grant = resolver.resolve(request("good-token")).await.unwrap();
    assert_eq!(grant.primary.server.as_str(), "db-1.internal:5432");
    assert_eq!(&*grant.primary.database, "tenant_acme");
    assert_eq!(grant.replicas.len(), 1);
    assert_eq!(grant.ttl, Duration::from_secs(60));
}

#[tokio::test]
async fn the_password_survives_the_wire_as_a_secret() {
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&sidecar.config()).await.unwrap();

    let grant = resolver.resolve(request("good-token")).await.unwrap();
    assert_eq!(grant.primary.password.expose(), "mock-password");
    // The whole grant, as someone would log it.
    assert!(!format!("{grant:#?}").contains("mock-password"));
}

#[tokio::test]
async fn pool_hints_and_claims_arrive() {
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&sidecar.config()).await.unwrap();

    let grant = resolver.resolve(request("good-token")).await.unwrap();
    assert_eq!(grant.pool.max_upstream, Some(16));
    assert_eq!(grant.pool.statement_timeout, Some(Duration::from_secs(30)));
    assert_eq!(grant.claims.subject.as_deref(), Some("mock-user"));
    // Zero timestamps mean absent, not the epoch.
    assert!(grant.claims.expires_at.is_none());
}

#[tokio::test]
async fn a_refused_token_is_a_refusal_not_a_failure() {
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&sidecar.config()).await.unwrap();

    let err = resolver.resolve(request("refuse-me")).await.unwrap_err();
    assert!(
        matches!(err, AuthError::Refused(AuthRejection::TokenRejected)),
        "{err:?}"
    );

    let err = resolver.resolve(request("denied-me")).await.unwrap_err();
    assert!(
        matches!(err, AuthError::Refused(AuthRejection::NotPermitted)),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_sidecar_failure_is_not_a_refusal() {
    // The distinction the cache depends on: only a refusal is cached, because
    // caching a failure keeps tenants locked out after recovery.
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&sidecar.config()).await.unwrap();

    let err = resolver.resolve(request("boom-now")).await.unwrap_err();
    assert!(matches!(err, AuthError::Unavailable { .. }), "{err:?}");
}

#[tokio::test]
async fn a_malformed_grant_is_rejected_by_the_client() {
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&sidecar.config()).await.unwrap();

    let err = resolver.resolve(request("broken-grant")).await.unwrap_err();
    assert!(matches!(err, AuthError::Malformed { .. }), "{err:?}");
}

#[tokio::test]
async fn a_stalled_sidecar_times_out_rather_than_hanging() {
    // This call sits on the connection path. Hanging means the client's
    // connection attempt hangs too.
    let sidecar = Sidecar::start();
    let resolver = SidecarResolver::connect(&SidecarConfig {
        timeout: Duration::from_millis(300),
        ..sidecar.config()
    })
    .await
    .unwrap();

    let started = std::time::Instant::now();
    let err = resolver.resolve(request("stall-please")).await.unwrap_err();
    let elapsed = started.elapsed();

    assert!(matches!(err, AuthError::Unavailable { .. }), "{err:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "waited {elapsed:?}, so the timeout did not apply"
    );
}

#[tokio::test]
async fn connecting_to_a_socket_that_is_not_there_fails_fast() {
    let config = SidecarConfig::new("/nonexistent/pgprox/sidecar.sock");
    let err = SidecarResolver::connect(&config).await.unwrap_err();
    assert!(matches!(err, AuthError::Unavailable { .. }), "{err:?}");
}

#[tokio::test]
async fn the_cache_wraps_the_real_client() {
    // The composition the proxy actually uses, over a real socket rather than
    // against the fake.
    let sidecar = Sidecar::start();
    let inner = SidecarResolver::connect(&sidecar.config()).await.unwrap();
    let clock = FakeClock::new();
    let cache = CachingResolver::new(
        inner,
        Arc::new(clock.clone()) as Arc<dyn Clock>,
        CacheConfig::default(),
    );

    // A token whose header names an allowed algorithm, since the cache checks
    // before resolving.
    let token = {
        use base64::Engine as _;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        format!(
            "{}.{}.{}",
            b64.encode(r#"{"alg":"RS256"}"#),
            b64.encode(r#"{"sub":"u"}"#),
            b64.encode("sig")
        )
    };

    let first = cache.resolve(request(&token)).await.unwrap();
    let second = cache.resolve(request(&token)).await.unwrap();
    assert_eq!(first.tenant, second.tenant);
    assert_eq!(cache.len(), 1, "the second call was not served from cache");

    clock.advance(Duration::from_secs(61));
    cache.resolve(request(&token)).await.unwrap();
}

#[tokio::test]
async fn a_banned_algorithm_never_reaches_the_socket() {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let none_token = format!(
        "{}.{}.{}",
        b64.encode(r#"{"alg":"none"}"#),
        b64.encode("{}"),
        b64.encode("")
    );

    let sidecar = Sidecar::start();
    let inner = SidecarResolver::connect(&sidecar.config()).await.unwrap();
    let cache = CachingResolver::new(
        inner,
        Arc::new(FakeClock::new()) as Arc<dyn Clock>,
        CacheConfig::default(),
    );

    let err = cache.resolve(request(&none_token)).await.unwrap_err();
    assert!(
        matches!(err, AuthError::Refused(AuthRejection::AlgorithmNotAllowed)),
        "{err:?}"
    );
    assert!(cache.is_empty(), "a banned token occupied a cache entry");
}
