//! A mock credential sidecar, for testing everything that depends on one.
//!
//! Behaves like the real thing rather than recording calls: it resolves tokens
//! it knows, refuses ones it does not, and can be told to stall or return a
//! malformed grant so callers' error paths are reachable.
//!
//! Usage: `mock_sidecar <socket-path>`, printing `ready` on stdout once it is
//! listening.
//!
//! Behaviour is chosen by the token, so a test needs no control channel:
//!
//! | Token prefix | Behaviour |
//! | --- | --- |
//! | `refuse-`    | `Unauthenticated`, which the client maps to a refusal |
//! | `denied-`    | `PermissionDenied` |
//! | `stall-`     | Sleeps past any sane timeout |
//! | `broken-`    | Returns a response with no primary backend |
//! | `boom-`      | `Internal`, a sidecar failure rather than a refusal |
//! | anything else| A valid grant naming the token in its tenant id |
//!
//! Where that grant points is read from the environment, so the same binary
//! serves a unit test (defaults, which resolve to nothing) and the e2e stack
//! (the compose services). A mock that hard-coded its backends could only ever
//! be used by tests that never connect to one.
//!
//! | Variable | Default |
//! | --- | --- |
//! | `PGPROX_MOCK_PRIMARY`   | `db-1.internal:5432` |
//! | `PGPROX_MOCK_REPLICAS`  | `db-1-replica.internal:5432`, comma separated |
//! | `PGPROX_MOCK_DATABASE`  | the startup database the client asked for |
//! | `PGPROX_MOCK_USER`      | the startup user the client asked for |
//! | `PGPROX_MOCK_PASSWORD`  | `mock-password` |
//! | `PGPROX_MOCK_TLS`       | `verified`, or `disabled` |
//! | `PGPROX_MOCK_TENANT`    | `tenant-for-<the token's first eight bytes>` |
//!
//! The tenant is derived from the token by default because a test wanting two
//! tenants gets them by sending two tokens, with nothing to configure. That
//! stops working the moment something has to *name* the tenant somewhere else:
//! the query cache is opted into per tenant in the config document, and a
//! document naming a tenant derived from a base64 prefix is a document nobody
//! can write. `PGPROX_MOCK_TENANT` is for that, and only that.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::io::Write as _;
use std::time::Duration;

use pgprox_auth::client::pb;
use tokio::net::UnixListener;
use tonic::{Request, Response, Status};

#[derive(Default)]
struct MockSidecar;

/// Splits a `host:port` into its parts, falling back to 5432.
fn address(text: &str) -> (String, u32) {
    match text.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), port.parse().unwrap_or(5432)),
        None => (text.to_owned(), 5432),
    }
}

/// One backend, as the environment describes it.
fn backend(text: &str, database: &str, user: &str) -> pb::Backend {
    let (host, port) = address(text);
    pb::Backend {
        host,
        port,
        database: database.to_owned(),
        user: user.to_owned(),
        password: std::env::var("PGPROX_MOCK_PASSWORD").unwrap_or_else(|_| "mock-password".into()),
        tls: if std::env::var("PGPROX_MOCK_TLS").as_deref() == Ok("disabled") {
            pb::TlsMode::Disabled as i32
        } else {
            pb::TlsMode::Verified as i32
        },
    }
}

#[tonic::async_trait]
impl pb::credential_resolver_server::CredentialResolver for MockSidecar {
    async fn resolve(
        &self,
        request: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ResolveResponse>, Status> {
        let req = request.into_inner();
        let token = req.token.as_str();

        if token.starts_with("refuse-") {
            return Err(Status::unauthenticated("token refused by mock"));
        }
        if token.starts_with("denied-") {
            return Err(Status::permission_denied("tenant not permitted"));
        }
        if token.starts_with("boom-") {
            return Err(Status::internal("mock sidecar exploded"));
        }
        if token.starts_with("stall-") {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
        if token.starts_with("broken-") {
            // A response the client must reject: our bug, not the client's.
            return Ok(Response::new(pb::ResolveResponse {
                tenant_id: "acme".into(),
                primary: None,
                replicas: vec![],
                ttl_seconds: 60,
                pool: None,
                claims: None,
            }));
        }

        let database =
            std::env::var("PGPROX_MOCK_DATABASE").unwrap_or_else(|_| req.startup_database.clone());
        let user = std::env::var("PGPROX_MOCK_USER").unwrap_or_else(|_| req.startup_user.clone());
        let primary =
            std::env::var("PGPROX_MOCK_PRIMARY").unwrap_or_else(|_| "db-1.internal:5432".into());
        let replicas = std::env::var("PGPROX_MOCK_REPLICAS")
            .unwrap_or_else(|_| "db-1-replica.internal:5432".into());

        Ok(Response::new(pb::ResolveResponse {
            tenant_id: std::env::var("PGPROX_MOCK_TENANT")
                .unwrap_or_else(|_| format!("tenant-for-{}", &token[..token.len().min(8)])),
            primary: Some(backend(&primary, &database, &user)),
            replicas: replicas
                .split(',')
                .filter(|entry| !entry.trim().is_empty())
                .map(|entry| backend(entry.trim(), &database, &user))
                .collect(),
            ttl_seconds: 60,
            pool: Some(pb::PoolHints {
                max_upstream: 16,
                mode: pb::PoolMode::Transaction as i32,
                statement_timeout_ms: 30_000,
            }),
            claims: Some(pb::ClaimSet {
                subject: "mock-user".into(),
                expires_at_unix: 0,
                issued_at_unix: 0,
            }),
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .expect("usage: mock_sidecar <socket-path>");

    // A stale socket from a crashed run would make bind fail.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;

    println!("ready");
    std::io::stdout().flush()?;

    tonic::transport::Server::builder()
        .add_service(pb::credential_resolver_server::CredentialResolverServer::new(MockSidecar))
        .serve_with_incoming(tokio_stream::wrappers::UnixListenerStream::new(listener))
        .await?;
    Ok(())
}
