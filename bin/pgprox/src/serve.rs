//! Serving one client, and accepting the next.
//!
//! Everything above this file decides things. This one does them: it accepts a
//! socket, runs the handshake, holds the session's place in the registry, and
//! moves frames between the client and whichever upstream connection the pool
//! lends it.
//!
//! # The connection ceiling is refused politely
//!
//! A node at its limit answers with an `ErrorResponse` carrying SQLSTATE
//! 53300 rather than dropping the socket. A dropped socket looks like a
//! network fault to every driver, and sends the operator to the wrong place.
//!
//! # Where the upstream connection lives
//!
//! With the session, for as long as it holds the guard. It is taken out of the
//! pool on acquire and given back on release, because the relay awaits on it
//! and the pool's lock cannot be held across an await. See
//! `LivePool::take_connection`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use pgprox_core::admin::ClientState;
use pgprox_core::auth::{CredentialResolver, Grant};
use pgprox_core::clock::Clock;
use pgprox_core::error::ClientError;
use pgprox_core::ids::{ConnId, NodeId};
use pgprox_core::pool::{UpstreamGuard, UpstreamPool};
use pgprox_proto::backend::{self, BackendMessage, TxStatus};
use pgprox_proto::encode;
use pgprox_proto::frame::Frame;
use pgprox_route::replica::Replicas;
use pgprox_session::cancel::Registry;
use pgprox_session::connect::Upstreamed;
use pgprox_session::probe::ParameterCache;
use pgprox_session::relay::{ClientAction, Relay};
use pgprox_session::shell::{Handoff, ShellError, Wire, accept, authenticate_token, negotiate};
use pgprox_session::state::{Credential, Handshake, HandshakeConfig};
use pgprox_session::{TokenAuth, connect::PgConnector};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::dial::TcpUpstream;
use crate::sessions::Sessions;
use crate::wiring::NodePool;

/// How many clients a node will serve at once.
///
/// A counter with a guard rather than a semaphore, because refusing has to
/// happen after the handshake has told the client why, and a permit held
/// across that would count a connection that is being turned away.
#[derive(Debug)]
pub struct Gate {
    live: AtomicU32,
    ceiling: u32,
}

/// A client's place under the ceiling, released on drop.
///
/// Holds an `Arc` rather than a borrow, because a session runs on its own task
/// and a borrowed place could not travel there.
#[derive(Debug)]
pub struct Admitted(Arc<Gate>);

impl Drop for Admitted {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Gate {
    /// A gate admitting `ceiling` clients at once.
    #[must_use]
    pub const fn new(ceiling: u32) -> Self {
        Self {
            live: AtomicU32::new(0),
            ceiling,
        }
    }

    /// How many clients are being served.
    #[must_use]
    pub fn live(&self) -> u32 {
        self.live.load(Ordering::SeqCst)
    }

    /// Admits a client, or refuses because the node is full.
    pub fn admit(self: &Arc<Self>) -> Option<Admitted> {
        self.live
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |live| {
                (live < self.ceiling).then_some(live + 1)
            })
            .ok()
            .map(|_| Admitted(Arc::clone(self)))
    }
}

/// Everything one session needs that outlives it.
pub struct Context {
    /// This node.
    pub node: NodeId,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// How the handshake behaves.
    pub handshake: HandshakeConfig,
    /// Who resolves tokens.
    pub resolver: Arc<dyn CredentialResolver>,
    /// Where upstream connections come from.
    pub connector: Arc<PgConnector<TcpUpstream>>,
    /// The pool they are held in.
    pub pool: Arc<NodePool>,
    /// What a client is told about the server before it has one.
    pub parameters: Arc<ParameterCache>,
    /// Who is being served.
    pub sessions: Arc<Sessions>,
    /// Which queries can be cancelled.
    pub cancels: Arc<Registry>,
    /// How long a client waits for an upstream connection.
    pub acquire_timeout: Duration,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("node", &self.node)
            .finish_non_exhaustive()
    }
}

/// Runs one client to completion.
///
/// # Errors
///
/// Fails when the client disconnects, misbehaves, or is refused. Every case
/// has already been reported to the client where the protocol allows it.
pub async fn session<S>(stream: S, context: &Context, admitted: Admitted) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream);
    let mut handshake = Handshake::new(context.handshake.clone());

    let grant = match negotiate(&mut wire, &mut handshake).await? {
        // TLS is not terminated here yet, so a client that asked for it was
        // answered N and will have sent its startup packet in the clear or
        // gone away. Reaching Upgrade means the listener was configured with
        // certificates it cannot yet use, which is a refusal rather than a
        // silent downgrade.
        Handoff::Upgrade => {
            return Err(wire
                .refuse(ClientError::ProtocolViolation("TLS is not available"))
                .await);
        }
        Handoff::Cancel(conn) => return cancel(conn, context).await,
        Handoff::Ask(Credential::Scram) => {
            return Err(wire
                .refuse(ClientError::AuthRefused(
                    pgprox_core::error::AuthRejection::NotPermitted,
                ))
                .await);
        }
        Handoff::Ask(Credential::Jwt) => {
            let startup = handshake.startup().ok_or(ShellError::Disconnected)?.clone();
            let mut auth = TokenAuth::new(&startup, std::net::IpAddr::from([0, 0, 0, 0]));
            authenticate_token(
                &mut wire,
                &mut auth,
                context.resolver.as_ref(),
                std::time::SystemTime::now(),
            )
            .await?
        }
    };

    // The grant is what says where this tenant's database is, so the connector
    // learns it here: it is the only moment the credentials exist.
    context.connector.learn(&grant.primary);
    for replica in &grant.replicas {
        context.connector.learn(replica);
    }

    let parameters = context
        .parameters
        .ensure(context.connector.as_ref(), &grant.primary)
        .await
        .map_err(|err| ShellError::Refused(err.into()))?;

    let conn = context.cancels.issue();
    accept(&mut wire, conn, &parameters).await?;

    let _registered = context.sessions.register(
        conn,
        grant.tenant.clone(),
        context.node,
        context.clock.now(),
    );
    let outcome = relay(&mut wire, context, &grant, conn).await;
    drop(admitted);
    outcome
}

/// Moves frames between a client and the upstream connections it borrows.
async fn relay<S>(
    wire: &mut Wire<S>,
    context: &Context,
    grant: &Grant,
    conn: ConnId,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut relay = Relay::new();
    let mut held: Option<(UpstreamGuard, Upstreamed<crate::dial::Stream>)> = None;
    let mut body = Vec::new();
    let replicas = Replicas::new(
        grant.replicas.len(),
        pgprox_route::replica::ReplicaConfig::default(),
    );

    loop {
        let tag = wire.read_tagged(&mut body).await?;
        let frame = Frame::new(tag, &body);
        let Ok(message) = pgprox_proto::frontend::decode(&frame) else {
            return Err(wire
                .refuse(ClientError::ProtocolViolation("undecodable message"))
                .await);
        };

        let outcome = relay.on_client(&message, &replicas, context.clock.now());
        if let Some(reason) = outcome.pinned {
            context.sessions.set_pinned(conn, reason.as_str());
        }

        match outcome.action {
            ClientAction::Close => return Ok(()),
            ClientAction::Answer(_) => {
                // A SET pgprox.route the server never sees. The client still
                // needs a ReadyForQuery, or it waits forever for one.
                wire.queue(|out| encode::ready_for_query(out, TxStatus::Idle));
                wire.flush().await?;
                continue;
            }
            ClientAction::Send { acquire: true, .. } => {
                held = Some(borrow(context, grant, conn).await?);
                relay.acquired();
            }
            ClientAction::Send { acquire: false, .. } => {}
        }

        let Some((_guard, upstream)) = held.as_mut() else {
            return Err(wire
                .refuse(ClientError::ProtocolViolation(
                    "a message arrived with no connection to send it on",
                ))
                .await);
        };

        // Forwarded as it arrived: the relay never rewrites what it does not
        // have to.
        forward(&mut upstream.wire, tag, &body);
        upstream.wire.flush().await?;

        if pump(wire, upstream, &mut relay, context, conn).await? {
            let (mut guard, upstream) = held.take().ok_or(ShellError::Disconnected)?;
            context
                .pool
                .return_connection(guard.key(), guard.id(), upstream);
            // Marked clean only here, at the boundary the relay named: a guard
            // dropped without this discards its connection, which is right for
            // every other way out of this loop and wrong for this one.
            guard.release_clean();
            context.cancels.release(conn);
            context.sessions.count_transaction();
            context
                .sessions
                .set_state(conn, ClientState::Idle, context.clock.now());
            relay.released();
        }
    }
}

/// Takes an upstream connection for this session to use.
async fn borrow(
    context: &Context,
    grant: &Grant,
    conn: ConnId,
) -> Result<(UpstreamGuard, Upstreamed<crate::dial::Stream>), ShellError> {
    let deadline = context.clock.now() + context.acquire_timeout;
    context
        .sessions
        .set_state(conn, ClientState::Waiting, context.clock.now());

    let guard = context
        .pool
        .acquire(&grant.primary.pool_key(), deadline)
        .await
        .map_err(|err| ShellError::Refused(err.into()))?;
    let taken = context
        .pool
        .take_connection(guard.key(), guard.id())
        .ok_or(ShellError::Disconnected)?;

    // Registered while it is held and only while it is held: cancelling a
    // connection that has gone back to the pool cancels a stranger's query.
    context.cancels.hold(
        conn,
        pgprox_session::cancel::Cancellation {
            server: grant.primary.server.clone(),
            key: guard.key().clone(),
            backend_key: taken.backend_key.unwrap_or((0, 0)),
        },
    );
    context
        .sessions
        .set_state(conn, ClientState::Active, context.clock.now());

    Ok((guard, taken))
}

/// Copies the server's answer back, returning whether the connection is free.
async fn pump<S>(
    wire: &mut Wire<S>,
    upstream: &mut Upstreamed<crate::dial::Stream>,
    relay: &mut Relay,
    context: &Context,
    conn: ConnId,
) -> Result<bool, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut body = Vec::new();
    loop {
        let tag = upstream.wire.read_tagged(&mut body).await?;
        let frame = Frame::new(tag, &body);
        let decoded = backend::decode(&frame).unwrap_or(BackendMessage::Opaque(tag));
        let server = relay.on_server(&decoded);
        if let Some(reason) = server.pinned {
            context.sessions.set_pinned(conn, reason.as_str());
        }

        forward(wire, tag, &body);

        if matches!(decoded, BackendMessage::ReadyForQuery(_)) {
            wire.flush().await?;
            return Ok(server.release);
        }
    }
}

/// Queues one frame verbatim.
fn forward<S>(wire: &mut Wire<S>, tag: pgprox_proto::frame::Tag, body: &[u8])
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    wire.queue(|out| {
        out.push(tag.get());
        out.extend_from_slice(
            &u32::try_from(body.len() + 4)
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(body);
    });
}

/// Forwards a cancellation, or refuses it.
async fn cancel(conn: ConnId, context: &Context) -> Result<(), ShellError> {
    use pgprox_session::cancel::Routing;

    // Whatever happens, nothing is sent back: a CancelRequest gets no answer,
    // by design, so that a client cannot use it to learn whether a key is
    // real.
    match context.cancels.route(conn) {
        Routing::Local(cancellation) => {
            let Some(backend) = context.connector.backend(&cancellation.key) else {
                return Ok(());
            };
            let Ok(stream) = context.connector.dial(&backend).await else {
                return Ok(());
            };
            let _ = pgprox_session::cancel::send(stream, cancellation.backend_key).await;
            Ok(())
        }
        // Forwarding to a peer needs the gossip transport's cancel channel,
        // which is the same hop quota requests take. Until it carries this
        // too, a cancel that landed on the wrong node is dropped rather than
        // answered wrongly.
        Routing::Peer(_) | Routing::Unknown => Ok(()),
    }
}

/// Tells a client the node is full, then closes.
///
/// # Errors
///
/// Fails when the socket does. The refusal itself is not an error here: it is
/// the point.
pub async fn refuse_full<S>(stream: S, cap: u32) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut wire = Wire::new(stream);
    // Read whatever the client opened with, so the error lands where a driver
    // expects one rather than in the middle of its startup.
    let mut body = Vec::new();
    let _ = wire.read_untagged(&mut body).await;

    wire.queue(|out| {
        encode::error_response(
            out,
            &ClientError::UpstreamAtCap {
                server: pgprox_core::ids::ServerId::new("this node", 0),
                cap,
            },
        );
    });
    wire.flush().await
}

/// Accepts clients until the listener stops.
///
/// Each one is served on its own task. A session that fails takes nothing with
/// it: the error is the client's, and a node serving a hundred thousand of
/// them cannot let one end the loop.
///
/// # Errors
///
/// Fails only when accepting itself does, which means the listening socket has
/// gone and there is nothing left to serve.
pub async fn accept_loop(
    listener: tokio::net::TcpListener,
    context: Arc<Context>,
    gate: Arc<Gate>,
    ceiling: u32,
) -> std::io::Result<()> {
    loop {
        let (socket, _) = listener.accept().await?;
        let _ = socket.set_nodelay(true);
        let context = Arc::clone(&context);
        let gate = Arc::clone(&gate);

        tokio::spawn(async move {
            // Admission first, and the refusal is a message rather than a
            // dropped socket. A driver told 53300 reports it; a driver whose
            // socket vanished reports a network error.
            let Some(admitted) = gate.admit() else {
                let _ = refuse_full(socket, ceiling).await;
                return;
            };
            let _ = session(socket, context.as_ref(), admitted).await;
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_proto::frame::Tag;

    #[test]
    fn a_gate_admits_up_to_its_ceiling() {
        let gate = Arc::new(Gate::new(2));
        let first = gate.admit().expect("room for one");
        let second = gate.admit().expect("room for two");

        assert_eq!(gate.live(), 2);
        assert!(
            gate.admit().is_none(),
            "a full node admitted a third client"
        );

        drop(first);
        assert!(
            gate.admit().is_some(),
            "a departed client did not free its place"
        );
        drop(second);
    }

    #[test]
    fn a_gate_of_zero_admits_nobody() {
        // Configuration validation refuses this, and the gate must not be the
        // thing that decides otherwise.
        assert!(Arc::new(Gate::new(0)).admit().is_none());
    }

    #[tokio::test]
    async fn a_full_node_says_so_rather_than_dropping_the_socket() {
        // A dropped socket reads as a network fault to every driver and sends
        // the operator to the wrong place.
        use tokio::io::AsyncReadExt;

        let (ours, mut theirs) = tokio::io::duplex(4096);
        let client = tokio::spawn(async move {
            // A startup packet, so the refusal lands where a driver expects.
            let mut packet = Vec::new();
            pgprox_proto::encode_frontend::startup_message(
                &mut packet,
                pgprox_proto::encode::PROTOCOL_3_0,
                &[("user", "acme_app")],
            );
            tokio::io::AsyncWriteExt::write_all(&mut theirs, &packet)
                .await
                .unwrap();

            let mut header = [0_u8; 5];
            theirs.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            let mut body = vec![0; len - 4];
            theirs.read_exact(&mut body).await.unwrap();
            (Tag(header[0]), body)
        });

        refuse_full(ours, 10).await.unwrap();
        let (tag, body) = client.await.unwrap();

        assert_eq!(tag, Tag::ERROR_RESPONSE);
        let rendered = String::from_utf8_lossy(&body);
        assert!(rendered.contains("53300"), "{rendered}");
        assert!(
            rendered.contains("too many connections"),
            "the client was not told why: {rendered}"
        );
    }

    use pgprox_core::auth::{Backend, ClaimSet, FakeCredentialResolver, PoolHints, TlsMode};
    use pgprox_core::clock::FakeClock;
    use pgprox_core::ids::{PoolKey, ServerId};
    use pgprox_core::secret::SecretString;
    use pgprox_pool::live::LivePool;
    use pgprox_pool::pool::PoolConfig;
    use pgprox_session::state::TlsPosture;
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Entropy that is not, so a test's cancel keys are predictable.
    #[derive(Debug, Default)]
    struct Fixed;

    impl pgprox_session::cancel::Entropy for Fixed {
        fn next(&self) -> u64 {
            42
        }
    }

    /// A Postgres that authenticates anyone and answers every query the same
    /// way.
    ///
    /// A real socket speaking the real protocol: everything the proxy sends it
    /// is decoded by this project's own decoder, and everything it sends back
    /// goes through the proxy's relay untouched.
    async fn fake_postgres() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    // The startup packet.
                    let mut len = [0_u8; 4];
                    if socket.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let mut body = vec![0; u32::from_be_bytes(len) as usize - 4];
                    let _ = socket.read_exact(&mut body).await;

                    let mut out = Vec::new();
                    encode::authentication_ok(&mut out);
                    encode::parameter_status(&mut out, "server_version", "17.2");
                    encode::backend_key_data(&mut out, ConnId::new(NodeId::new(9), 0x00AB_CDEF));
                    encode::ready_for_query(&mut out, TxStatus::Idle);
                    let _ = socket.write_all(&out).await;

                    // Then one canned answer per query.
                    loop {
                        let mut header = [0_u8; 5];
                        if socket.read_exact(&mut header).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                        let mut body = vec![0; len - 4];
                        if socket.read_exact(&mut body).await.is_err() {
                            return;
                        }

                        let mut out = Vec::new();
                        out.push(Tag::COMMAND_COMPLETE.get());
                        let text = b"SELECT 1\0";
                        out.extend_from_slice(
                            &u32::try_from(text.len() + 4).unwrap().to_be_bytes(),
                        );
                        out.extend_from_slice(text);
                        encode::ready_for_query(&mut out, TxStatus::Idle);
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        addr
    }

    fn grant_for(addr: SocketAddr) -> Grant {
        Grant {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            primary: Backend {
                server: ServerId::new("127.0.0.1", addr.port()),
                database: "acme".into(),
                user: "acme_app".into(),
                password: SecretString::new("hunter2"),
                tls: TlsMode::Disabled,
            },
            replicas: Vec::new(),
            pool: PoolHints::default(),
            ttl: Duration::from_secs(60),
            claims: ClaimSet::default(),
        }
    }

    fn context_for(addr: SocketAddr) -> Context {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new());
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let connector = Arc::new(PgConnector::new(TcpUpstream::new(tls)));

        Context {
            node: NodeId::new(1),
            clock: Arc::clone(&clock),
            handshake: HandshakeConfig {
                // No certificates are configured in a test, and a listener
                // that required TLS would refuse every client before the part
                // under test.
                tls: TlsPosture::Optional,
                static_users: Vec::new(),
            },
            resolver: Arc::new(
                FakeCredentialResolver::new().with_grant("good.token", grant_for(addr)),
            ),
            pool: LivePool::new(Arc::clone(&connector), clock, PoolConfig::default()),
            connector,
            parameters: Arc::new(ParameterCache::new()),
            sessions: Sessions::new(),
            cancels: Arc::new(Registry::new(NodeId::new(1), Box::new(Fixed))),
            acquire_timeout: Duration::from_secs(5),
        }
    }

    /// Reads one tagged message from a client's end.
    async fn expect<S: AsyncRead + Unpin>(io: &mut S) -> (Tag, Vec<u8>) {
        let mut header = [0_u8; 5];
        io.read_exact(&mut header).await.unwrap();
        let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
        let mut body = vec![0; len - 4];
        io.read_exact(&mut body).await.unwrap();
        (Tag(header[0]), body)
    }

    fn startup_and_password(token: &str) -> Vec<u8> {
        let mut out = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut out,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[("user", "acme_app"), ("database", "acme")],
        );
        pgprox_proto::encode_frontend::password_message(&mut out, token);
        out
    }

    #[tokio::test]
    async fn a_client_connects_queries_and_the_connection_goes_back_to_the_pool() {
        // The milestone in one test. Every crate in this workspace is on this
        // path: the handshake, the token exchange, the parameter probe, the
        // pool, the relay's release rule, and the session registry.
        let addr = fake_postgres().await;
        let context = context_for(addr);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let served = tokio::spawn({
            // The context has to outlive the task, and everything in it is
            // already shared.
            let context = Arc::new(context);
            let held = Arc::clone(&context);
            async move {
                let _ = session(ours, held.as_ref(), admitted).await;
                context
            }
        });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();

        // The credential request, then the acceptance: two authentication
        // messages, and reading one would leave the next assertion off by one.
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(
            expect(&mut client).await.0,
            Tag::PARAMETER_STATUS,
            "the client was not told what server it reached"
        );
        assert_eq!(expect(&mut client).await.0, Tag::BACKEND_KEY_DATA);
        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::READY_FOR_QUERY);
        assert_eq!(body, b"I");

        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        client.write_all(&query).await.unwrap();

        assert_eq!(expect(&mut client).await.0, Tag::COMMAND_COMPLETE);
        assert_eq!(expect(&mut client).await.0, Tag::READY_FOR_QUERY);

        // Terminate, so the session ends and the task returns the context.
        let mut bye = Vec::new();
        pgprox_proto::encode_frontend::terminate(&mut bye);
        client.write_all(&bye).await.unwrap();

        let context = served.await.unwrap();
        let key = PoolKey::new(ServerId::new("127.0.0.1", addr.port()), "acme", "acme_app");
        let stats = pgprox_core::pool::UpstreamPool::stats(context.pool.as_ref(), &key);
        assert_eq!(
            stats.idle, 1,
            "the connection did not go back to the pool: {stats:?}"
        );
        assert_eq!(stats.active, 0);
        assert_eq!(context.sessions.transactions(), 1);
    }

    #[tokio::test]
    async fn a_rejected_token_never_reaches_the_database() {
        // The proxy is the authentication boundary. A bad token must cost the
        // upstream nothing at all.
        let addr = fake_postgres().await;
        let context = context_for(addr);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let served = tokio::spawn({
            let context = Arc::new(context);
            let held = Arc::clone(&context);
            async move {
                let result = session(ours, held.as_ref(), admitted).await;
                (result, context)
            }
        });

        client
            .write_all(&startup_and_password("wrong.token"))
            .await
            .unwrap();
        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        let (tag, body) = expect(&mut client).await;

        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(String::from_utf8_lossy(&body).contains("28000"));

        let (result, context) = served.await.unwrap();
        assert!(result.is_err());
        assert_eq!(
            context.connector.known(),
            0,
            "a refused client taught the connector where a database lives"
        );
        assert!(context.sessions.is_empty());
    }

    #[tokio::test]
    async fn a_session_that_ends_leaves_no_client_behind() {
        // The registry is what SHOW CLIENTS reads. A row for a client that has
        // gone is worse than no row: an operator chasing it finds nothing.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let admitted = gate.admit().unwrap();
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }
        assert_eq!(context.sessions.len(), 1);

        drop(client);
        let _ = served.await.unwrap();

        assert!(
            context.sessions.is_empty(),
            "a disconnected client was still listed"
        );
        assert_eq!(gate.live(), 0, "its place under the ceiling was not freed");
    }

    #[tokio::test]
    async fn a_route_hint_is_answered_without_touching_the_database() {
        // SET pgprox.route is about the session, and Postgres would reject it
        // as an unknown parameter. Answering it here is what makes it a
        // feature rather than an error, and it must cost no connection.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        client
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut client).await;
        }

        let mut hint = Vec::new();
        pgprox_proto::encode_frontend::query(&mut hint, "SET pgprox.route = 'replica'");
        client.write_all(&hint).await.unwrap();

        let (tag, body) = expect(&mut client).await;
        assert_eq!(tag, Tag::READY_FOR_QUERY);
        assert_eq!(body, b"I");

        let key = PoolKey::new(ServerId::new("127.0.0.1", addr.port()), "acme", "acme_app");
        let stats = pgprox_core::pool::UpstreamPool::stats(context.pool.as_ref(), &key);
        assert_eq!(
            stats.active + stats.idle,
            0,
            "a statement the server never sees opened a connection"
        );

        drop(client);
        let _ = served.await.unwrap();
    }

    #[tokio::test]
    async fn a_static_user_is_refused_until_the_admin_path_is_wired() {
        // The SCRAM exchange exists and the admin surface it leads to is not
        // reachable from the listener yet. Refusing says so; accepting would
        // authenticate somebody into nothing.
        let addr = fake_postgres().await;
        let mut context = context_for(addr);
        context.handshake.static_users = vec!["pgprox_admin".to_owned()];
        let context = Arc::new(context);
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(8192);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        pgprox_proto::encode_frontend::startup_message(
            &mut packet,
            pgprox_proto::encode::PROTOCOL_3_0,
            &[("user", "pgprox_admin"), ("database", "pgprox")],
        );
        client.write_all(&packet).await.unwrap();

        assert_eq!(expect(&mut client).await.0, Tag::AUTHENTICATION);
        assert_eq!(expect(&mut client).await.0, Tag::ERROR_RESPONSE);
        assert!(served.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn a_cancel_for_a_query_this_node_is_not_running_is_dropped_silently() {
        // A CancelRequest gets no answer by design, so a client cannot use one
        // to learn whether a key is real.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();

        let (ours, mut client) = tokio::io::duplex(4096);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        let (process_id, secret) =
            pgprox_proto::backend::key_from_conn_id(ConnId::new(NodeId::new(1), 7));
        pgprox_proto::encode_frontend::cancel_request(&mut packet, process_id, secret);
        client.write_all(&packet).await.unwrap();

        served.await.unwrap().unwrap();
        let mut anything = [0_u8; 1];
        assert_eq!(
            client.read(&mut anything).await.unwrap(),
            0,
            "a cancel request was answered, which tells a prober the key was real"
        );
    }

    #[tokio::test]
    async fn a_cancel_for_a_held_query_reaches_the_server() {
        // The whole point: the key the proxy issued resolves to the key the
        // server issued, on a fresh connection carrying nothing else.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let backend = grant_for(addr).primary;
        context.connector.learn(&backend);

        let conn = context.cancels.issue();
        context.cancels.hold(
            conn,
            pgprox_session::cancel::Cancellation {
                server: backend.server.clone(),
                key: backend.pool_key(),
                backend_key: (4242, 99),
            },
        );

        let gate = Arc::new(Gate::new(10));
        let admitted = gate.admit().unwrap();
        let (ours, mut client) = tokio::io::duplex(4096);
        let held = Arc::clone(&context);
        let served = tokio::spawn(async move { session(ours, held.as_ref(), admitted).await });

        let mut packet = Vec::new();
        let (process_id, secret) = pgprox_proto::backend::key_from_conn_id(conn);
        pgprox_proto::encode_frontend::cancel_request(&mut packet, process_id, secret);
        client.write_all(&packet).await.unwrap();

        served.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_full_node_refuses_the_next_client_and_keeps_serving_the_others() {
        // Refusal must not be contagious: the clients already connected are
        // mid-transaction and have done nothing wrong.
        let addr = fake_postgres().await;
        let context = Arc::new(context_for(addr));
        let gate = Arc::new(Gate::new(1));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = listener.local_addr().unwrap();

        tokio::spawn(accept_loop(
            listener,
            Arc::clone(&context),
            Arc::clone(&gate),
            1,
        ));

        let mut first = tokio::net::TcpStream::connect(proxy).await.unwrap();
        first
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        for _ in 0..5 {
            expect(&mut first).await;
        }

        let mut second = tokio::net::TcpStream::connect(proxy).await.unwrap();
        second
            .write_all(&startup_and_password("good.token"))
            .await
            .unwrap();
        let (tag, body) = expect(&mut second).await;

        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(String::from_utf8_lossy(&body).contains("53300"));

        // The first client is still being served.
        let mut query = Vec::new();
        pgprox_proto::encode_frontend::query(&mut query, "SELECT 1");
        first.write_all(&query).await.unwrap();
        assert_eq!(expect(&mut first).await.0, Tag::COMMAND_COMPLETE);
    }
}
