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

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::io::Write as _;
use std::time::Duration;

use pgprox_auth::client::pb;
use tokio::net::UnixListener;
use tonic::{Request, Response, Status};

#[derive(Default)]
struct MockSidecar;

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

        Ok(Response::new(pb::ResolveResponse {
            tenant_id: format!("tenant-for-{}", &token[..token.len().min(8)]),
            primary: Some(pb::Backend {
                host: "db-1.internal".into(),
                port: 5432,
                database: req.startup_database,
                user: req.startup_user,
                password: "mock-password".into(),
                tls: pb::TlsMode::Verified as i32,
            }),
            replicas: vec![pb::Backend {
                host: "db-1-replica.internal".into(),
                port: 5432,
                database: "tenant_acme".into(),
                user: "acme_app".into(),
                password: "mock-password".into(),
                tls: pb::TlsMode::Verified as i32,
            }],
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
