//! The socket that carries gossip between nodes.
//!
//! `pgprox-cluster` owns every rule about what a digest means and needs no
//! socket to prove any of them. This is the socket, and it holds no rule: a
//! message arrives, it is handed to [`GossipCoordinator::gossip`], and what
//! comes back is this node's own digest.
//!
//! # The wire format is JSON, on purpose
//!
//! One message per line. A fleet gossips once per node per second, so the
//! encoding costs nothing measurable, and the thing it buys is that an
//! operator debugging a cluster that will not converge can read the traffic
//! with `nc`. The digest schema is already a public interface (see
//! `pgprox-cluster`'s notes), so a self-describing encoding matches what it
//! already is.
//!
//! The wire types are declared here rather than by deriving `Serialize` on the
//! `pgprox-core` DTOs, for the same reason `pgprox-admin` declares its own
//! response bodies: a field can be renamed in core without silently changing
//! what a running fleet speaks.
//!
//! # Peers are names, resolved per connection
//!
//! A peer is kept as it was written and handed to `connect` each time, rather
//! than parsed into a `SocketAddr` at startup. Two reasons, and either alone
//! would decide it: a fleet is addressed by service name, which is not an IP
//! and cannot be parsed as one, and the address behind that name changes when
//! a pod is replaced. A node that resolved once at startup would gossip at a
//! peer that has moved.
//!
//! # An exchange, not a broadcast
//!
//! A round opens a connection, sends this node's digest, and reads the peer's
//! back. Two nodes therefore converge in one round trip rather than two
//! one-way messages, and a peer that is down costs one failed connect rather
//! than a message queued for something that will never read it.
//!
//! # What is refused
//!
//! More than [`MAX_INCOMING`] bytes on one connection, refused by reading no
//! further rather than by checking a length after buffering it. A gossip port
//! is reachable from inside the cluster network, and a peer that is really an
//! attacker must not be able to make a node allocate without limit.
//!
//! The budget is per connection rather than per message, which is what makes
//! it enforceable by construction: a client opens one connection per round, so
//! a peer that keeps talking on one connection is doing something a peer does
//! not do.

use std::sync::Arc;
use std::time::Duration;

use pgprox_cluster::digest::VersionedDigest;
use pgprox_cluster::service::GossipCoordinator;
use pgprox_core::admin::{ClientState, ClientView};
use pgprox_core::cluster::{ClusterDigest, NodeMode, QuotaError, QuotaLease};
use pgprox_core::ids::{ConnId, NodeId, ServerId, TenantId};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// How much a node will read from one gossip connection.
///
/// A digest carries one entry per server and one per homed tenant, and a node
/// homes at most a few thousand tenants, so 1 MiB is far above anything
/// legitimate and far below anything that hurts. A connection that reaches it
/// is closed with whatever it had.
pub const MAX_INCOMING: u64 = 1024 * 1024;

/// How long a round waits on a peer.
///
/// Short: a peer that has not answered within this is either gone or too busy
/// to gossip, and both mean the same thing to the failure detector. Waiting
/// longer would let one unreachable node slow the whole round.
pub const PEER_TIMEOUT: Duration = Duration::from_secs(2);

/// One message on the gossip socket.
///
/// Tagged, so a message a node has not been taught about is recognisable as
/// one rather than as a malformed digest. That matters during a rolling
/// upgrade, which is the only time it happens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Message {
    /// What a node is, for its peers to merge.
    Digest(DigestWire),
    /// A node asking the leader for a lease it cannot grant itself.
    QuotaRequest {
        /// Which upstream server, as `host:port`.
        server: String,
        /// Which node will hold the lease. Not necessarily the sender, though
        /// today it always is.
        holder: u16,
        /// How many connections it wants.
        want: u32,
    },
    /// The leader granting one.
    ///
    /// The lifetime travels as a duration rather than as an instant, because
    /// two nodes share no clock and an `Instant` means nothing off the machine
    /// that made it.
    QuotaGrant {
        /// Which server.
        server: String,
        /// How many connections.
        count: u32,
        /// How long it lasts, from the moment the leader granted it.
        ttl_ms: u64,
    },
    /// Asking a peer to list the clients it is serving.
    ///
    /// The one read that fans out. Aggregates answer from the digest every
    /// node already holds, because a total is a number; a client list is one
    /// row per connection and gossiping those every second would put a hundred
    /// thousand rows on the wire. So it is asked for when somebody asks.
    ClientsRequest,
    /// A peer's answer to [`Message::ClientsRequest`].
    Clients {
        /// One entry per client that peer is serving.
        clients: Vec<ClientWire>,
    },
    /// A cancel for a connection another node owns.
    ///
    /// Forwarded rather than answered: the node that issued a cancel key is
    /// the only one that knows which upstream connection it names, and a
    /// client's cancel lands on whichever pod its second connection reached.
    Cancel {
        /// The node that owns the connection.
        node: u16,
        /// The rest of the key it issued.
        secret: u64,
    },
    /// The leader refusing.
    QuotaRefused {
        /// `exhausted` or `no_leader`, which the asker treats differently: the
        /// first means wait, the second means this node was wrong about who
        /// the leader is.
        reason: String,
    },
}

/// One client, as it travels between nodes.
///
/// Its own type for the same reason `DigestWire` is: a field renamed in
/// `pgprox-core` must not silently change what a running fleet speaks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientWire {
    /// The node that issued the connection id, and the rest of it.
    pub node: u16,
    /// The connection's own number.
    pub conn: u64,
    /// Which tenant.
    pub tenant: String,
    /// `idle`, `active` or `waiting`.
    pub state: String,
    /// How long it has been in that state, in milliseconds.
    pub since_ms: u64,
    /// Why it is pinned, if it is.
    pub pinned: Option<String>,
}

impl From<&ClientView> for ClientWire {
    fn from(view: &ClientView) -> Self {
        Self {
            node: view.node.get(),
            conn: view.conn.secret(),
            tenant: view.tenant.as_str().to_owned(),
            state: match view.state {
                ClientState::Active => "active".to_owned(),
                ClientState::Waiting => "waiting".to_owned(),
                _ => "idle".to_owned(),
            },
            since_ms: u64::try_from(view.since.as_millis()).unwrap_or(u64::MAX),
            pinned: view.pinned.clone(),
        }
    }
}

impl ClientWire {
    /// Reads a peer's client back into what a report renders.
    #[must_use]
    pub fn parse(&self) -> ClientView {
        ClientView {
            conn: ConnId::new(NodeId::new(self.node), self.conn),
            tenant: TenantId::new(&self.tenant),
            node: NodeId::new(self.node),
            state: match self.state.as_str() {
                "active" => ClientState::Active,
                "waiting" => ClientState::Waiting,
                _ => ClientState::Idle,
            },
            since: Duration::from_millis(self.since_ms),
            pinned: self.pinned.clone(),
        }
    }
}

/// One node's digest, as it travels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DigestWire {
    /// Which node.
    pub node: u16,
    /// `active` or `draining`.
    pub mode: String,
    /// Its version, which is how a peer orders it against what it holds.
    pub version: u64,
    /// Client connections it is serving.
    pub client_conns: u32,
    /// Upstream connections it holds, per server, as `host:port`.
    pub upstream_conns: Vec<(String, u32)>,
    /// Per-tenant usage, for the tenants this node homes.
    pub tenant_usage: Vec<(String, u32)>,
}

impl From<&VersionedDigest> for DigestWire {
    fn from(versioned: &VersionedDigest) -> Self {
        Self {
            node: versioned.digest.node.get(),
            mode: match versioned.digest.mode {
                NodeMode::Draining => "draining".to_owned(),
                _ => "active".to_owned(),
            },
            version: versioned.version,
            client_conns: versioned.digest.client_conns,
            upstream_conns: versioned
                .digest
                .upstream_conns
                .iter()
                .map(|(server, count)| (server.to_string(), *count))
                .collect(),
            tenant_usage: versioned
                .digest
                .tenant_usage
                .iter()
                .map(|(tenant, count)| (tenant.as_str().to_owned(), *count))
                .collect(),
        }
    }
}

impl DigestWire {
    /// Reads a message back into what the cluster layer understands.
    ///
    /// Returns `None` when a field cannot be understood, which is refused
    /// rather than defaulted: a server address that failed to parse would
    /// otherwise become a different server, and its usage would be counted
    /// against the wrong cap.
    #[must_use]
    pub fn parse(&self) -> Option<VersionedDigest> {
        let mut upstream_conns = Vec::with_capacity(self.upstream_conns.len());
        for (server, count) in &self.upstream_conns {
            upstream_conns.push((ServerId::parse(server)?, *count));
        }

        Some(VersionedDigest {
            digest: ClusterDigest {
                node: NodeId::new(self.node),
                mode: if self.mode == "draining" {
                    NodeMode::Draining
                } else {
                    NodeMode::Active
                },
                client_conns: self.client_conns,
                upstream_conns,
                tenant_usage: self
                    .tenant_usage
                    .iter()
                    .map(|(tenant, count)| (TenantId::new(tenant), *count))
                    .collect(),
            },
            version: self.version,
        })
    }
}

/// Serves gossip until the shutdown future resolves.
///
/// # Errors
///
/// Fails when the listening socket does. A peer that misbehaves costs that one
/// connection and nothing else: a node whose gossip listener died would stop
/// hearing from the fleet without stopping serving clients, which is the worst
/// of both.
pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    coordinator: Arc<GossipCoordinator>,
    cancels: Arc<dyn CancelSink>,
    shutdown: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send,
{
    tokio::pin!(shutdown);
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted?,
            () = &mut shutdown => return Ok(()),
        };

        let coordinator = Arc::clone(&coordinator);
        let cancels = Arc::clone(&cancels);
        tokio::spawn(async move {
            let _ = answer(accepted.0, &coordinator, cancels.as_ref()).await;
        });
    }
}

/// Merges what a peer sent and answers with this node's own digest.
async fn answer<S>(
    stream: S,
    coordinator: &GossipCoordinator,
    cancels: &dyn CancelSink,
) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read.take(MAX_INCOMING)).lines();

    while let Some(line) = lines.next_line().await? {
        // A line that does not parse is answered with this node's digest
        // rather than closing the connection: gossip from a node running a
        // newer build is expected during a rolling upgrade, and refusing to
        // talk to it at all would partition the fleet along version lines.
        let reply = match serde_json::from_str::<Message>(&line) {
            Ok(Message::QuotaRequest {
                server,
                holder,
                want,
            }) => grant(coordinator, &server, holder, want),
            Ok(Message::ClientsRequest) => Message::Clients {
                clients: cancels.clients().iter().map(ClientWire::from).collect(),
            },
            Ok(Message::Cancel { node, secret }) => {
                // Answered with this node's digest whatever happens. A cancel
                // gets no acknowledgement by design, so a peer relaying one for
                // a client cannot learn from the answer whether the key was
                // real.
                cancels.cancel(ConnId::new(NodeId::new(node), secret)).await;
                Message::Digest(DigestWire::from(&coordinator.outgoing()))
            }
            Ok(Message::Digest(wire)) => {
                if let Some(incoming) = wire.parse() {
                    coordinator.gossip(incoming);
                }
                Message::Digest(DigestWire::from(&coordinator.outgoing()))
            }
            // A grant or a refusal arriving unasked for, or something this
            // build does not know. Answered with what this node is, which is
            // the only useful thing it has to say.
            _ => Message::Digest(DigestWire::from(&coordinator.outgoing())),
        };

        write.write_all(&encode(&reply)).await?;
        write.flush().await?;
    }
    Ok(())
}

/// Answers a quota request from the local ledger.
///
/// Only the leader has a free pool to grant from, and `serve_request` is what
/// knows whether this node is it. Nothing here decides: a transport that
/// second-guessed the ledger would be a second place the cap is enforced.
fn grant(coordinator: &GossipCoordinator, server: &str, holder: u16, want: u32) -> Message {
    let Some(server) = ServerId::parse(server) else {
        return Message::QuotaRefused {
            reason: "exhausted".to_owned(),
        };
    };

    match coordinator.serve_request(&server, NodeId::new(holder), want) {
        Ok(lease) => Message::QuotaGrant {
            server: lease.server().to_string(),
            count: lease.nominal_count(),
            ttl_ms: u64::try_from(
                lease
                    .expires_at()
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis(),
            )
            .unwrap_or(u64::MAX),
        },
        Err(QuotaError::NoLeader) => Message::QuotaRefused {
            reason: "no_leader".to_owned(),
        },
        Err(_) => Message::QuotaRefused {
            reason: "exhausted".to_owned(),
        },
    }
}

/// One message, as a line.
fn encode(message: &Message) -> Vec<u8> {
    // Serialising these types cannot fail: every field is a string or a
    // number. An empty line rather than a panic if that ever stops being true,
    // because the peer treats an unreadable line as one it does not understand.
    let mut out = serde_json::to_vec(message).unwrap_or_default();
    out.push(b'\n');
    out
}

/// One gossip round against every peer.
///
/// Peers are contacted concurrently: a round that walked them in sequence
/// would take the sum of the timeouts when several were down, and the failure
/// detector would start suspecting nodes that are perfectly healthy.
pub async fn round(peers: &[String], coordinator: &Arc<GossipCoordinator>) -> usize {
    let mut reached = Vec::new();
    for peer in peers {
        reached.push(tokio::spawn({
            let coordinator = Arc::clone(coordinator);
            let peer = peer.clone();
            async move {
                tokio::time::timeout(PEER_TIMEOUT, exchange(&peer, &coordinator))
                    .await
                    .is_ok_and(|result| result.is_ok())
            }
        }));
    }

    let mut count = 0;
    for handle in reached {
        if handle.await.unwrap_or(false) {
            count += 1;
        }
    }
    count
}

/// Sends this node's digest to one peer and merges the answer.
///
/// # Errors
///
/// Fails when the peer cannot be reached or does not answer. Both are ordinary:
/// a node that is restarting is unreachable for a few seconds and the failure
/// detector is what decides that it matters.
pub async fn exchange(peer: &str, coordinator: &GossipCoordinator) -> std::io::Result<()> {
    let stream = tokio::net::TcpStream::connect(peer).await?;
    speak(stream, coordinator).await
}

/// The client half of one exchange, over any stream.
async fn speak<S>(stream: S, coordinator: &GossipCoordinator) -> std::io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);

    let outgoing = Message::Digest(DigestWire::from(&coordinator.outgoing()));
    write.write_all(&encode(&outgoing)).await?;
    write.flush().await?;

    let mut lines = BufReader::new(read.take(MAX_INCOMING)).lines();
    if let Some(line) = lines.next_line().await?
        && let Ok(Message::Digest(wire)) = serde_json::from_str::<Message>(&line)
        && let Some(incoming) = wire.parse()
    {
        coordinator.gossip(incoming);
    }
    Ok(())
}

/// Asks one peer for a lease, over its own connection.
///
/// # Errors
///
/// [`QuotaError::NoLeader`] when the peer cannot be reached or does not answer
/// in time, which is the same answer the caller gets when there is no leader
/// at all: both mean falling back to the guaranteed share, and the guaranteed
/// share cannot breach the cap.
pub async fn ask(
    peer: &str,
    server: &ServerId,
    holder: NodeId,
    want: u32,
) -> Result<QuotaLease, QuotaError> {
    let asked = tokio::time::timeout(PEER_TIMEOUT, ask_over(peer, server, holder, want))
        .await
        .map_err(|_| QuotaError::NoLeader)?;
    asked.unwrap_or(Err(QuotaError::NoLeader))
}

/// The request itself, split out so a test can drive it over a duplex.
async fn ask_over(
    peer: &str,
    server: &ServerId,
    holder: NodeId,
    want: u32,
) -> Option<Result<QuotaLease, QuotaError>> {
    let stream = tokio::net::TcpStream::connect(peer).await.ok()?;
    request_over(stream, server, holder, want).await
}

/// Sends a quota request on an open stream and reads the answer.
async fn request_over<S>(
    stream: S,
    server: &ServerId,
    holder: NodeId,
    want: u32,
) -> Option<Result<QuotaLease, QuotaError>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let request = Message::QuotaRequest {
        server: server.to_string(),
        holder: holder.get(),
        want,
    };

    // Timed across the round trip, because the lease's lifetime starts when the
    // leader granted it and this node only learns of it afterwards. Counting
    // from arrival would hold the lease for the transit time past its expiry,
    // and over-holding is the one direction the cap has no tolerance for.
    let sent = std::time::Instant::now();
    write.write_all(&encode(&request)).await.ok()?;
    write.flush().await.ok()?;

    let mut lines = BufReader::new(read.take(MAX_INCOMING)).lines();
    let line = lines.next_line().await.ok()??;
    let elapsed = sent.elapsed();

    match serde_json::from_str::<Message>(&line).ok()? {
        Message::QuotaGrant {
            server,
            count,
            ttl_ms,
        } => {
            let granted = ServerId::parse(&server)?;
            let ttl = Duration::from_millis(ttl_ms).saturating_sub(elapsed);
            Some(Ok(QuotaLease::new(
                granted,
                count,
                std::time::Instant::now() + ttl,
            )))
        }
        Message::QuotaRefused { reason } if reason == "exhausted" => {
            Some(Err(QuotaError::Exhausted {
                server: server.clone(),
            }))
        }
        _ => Some(Err(QuotaError::NoLeader)),
    }
}

/// Where a forwarded cancel goes once it has arrived.
///
/// A trait rather than a direct call, because the registry that knows which
/// upstream connection a key names lives with the sessions, and this module
/// knows only how to get a message across.
#[async_trait::async_trait]
pub trait CancelSink: Send + Sync + std::fmt::Debug {
    /// Cancels the query this key names, if this node is running one.
    async fn cancel(&self, conn: ConnId);

    /// The clients this node is serving, for a peer's fan-out.
    ///
    /// Defaulted to none, because a node with no sessions has none and a test
    /// that only cares about cancels should not have to say so.
    fn clients(&self) -> Vec<ClientView> {
        Vec::new()
    }
}

/// A sink that does nothing, for a node with no sessions to cancel.
#[derive(Debug, Default)]
pub struct NoCancels;

#[async_trait::async_trait]
impl CancelSink for NoCancels {
    async fn cancel(&self, _conn: ConnId) {}
}

/// Forwards a cancel to the node that owns the connection.
///
/// Nothing comes back and nothing is awaited beyond the send: a `CancelRequest`
/// is unacknowledged in the protocol itself, and making this one answer would
/// give a prober an oracle the real thing does not have.
pub async fn forward(peer: &str, conn: ConnId) {
    let message = Message::Cancel {
        node: conn.node().get(),
        secret: conn.secret(),
    };

    let _ = tokio::time::timeout(PEER_TIMEOUT, async {
        let mut stream = tokio::net::TcpStream::connect(peer).await.ok()?;
        stream.write_all(&encode(&message)).await.ok()?;
        stream.flush().await.ok()
    })
    .await;
}

/// Asks one peer for the clients it is serving.
///
/// # Errors
///
/// Fails when the peer cannot be reached or does not answer in time. The
/// caller reports that as a partial answer rather than as an empty one: an
/// operator seeing no clients concludes there are none.
pub async fn clients_of(peer: &str) -> Result<Vec<ClientView>, String> {
    tokio::time::timeout(PEER_TIMEOUT, async {
        let stream = tokio::net::TcpStream::connect(peer)
            .await
            .map_err(|err| err.to_string())?;
        let (read, mut write) = tokio::io::split(stream);

        write
            .write_all(&encode(&Message::ClientsRequest))
            .await
            .map_err(|err| err.to_string())?;
        write.flush().await.map_err(|err| err.to_string())?;

        let mut lines = BufReader::new(read.take(MAX_INCOMING)).lines();
        let line = lines
            .next_line()
            .await
            .map_err(|err| err.to_string())?
            .ok_or_else(|| "the peer said nothing".to_owned())?;

        match serde_json::from_str::<Message>(&line) {
            Ok(Message::Clients { clients }) => Ok(clients.iter().map(ClientWire::parse).collect()),
            _ => Err("the peer did not answer with a client list".to_owned()),
        }
    })
    .await
    .unwrap_or_else(|_| Err("the peer did not answer in time".to_owned()))
}

/// The `QuotaTransport` the composition root fills in.
///
/// Holds the peer table and nothing else. Every rule about whether a lease may
/// be granted lives in `pgprox-cluster`, on the node that answers.
#[derive(Debug)]
pub struct GossipTransport {
    /// Where the peer table comes from, rather than a copy of one.
    ///
    /// A quota request is sent to whoever leads *now*, and the leader is the
    /// lowest active node in a view that changes. Holding a table taken at
    /// startup meant a node that joined later could lead and be unreachable,
    /// which reads as `NoLeader` and drops the whole fleet to its guaranteed
    /// shares. `M19.3`.
    peers: std::sync::Arc<dyn pgprox_core::cluster::PeerSource>,
}

impl GossipTransport {
    /// A transport that asks `peers` where the leader is, each time.
    #[must_use]
    pub fn new(peers: std::sync::Arc<dyn pgprox_core::cluster::PeerSource>) -> Self {
        Self { peers }
    }
}

#[async_trait::async_trait]
impl pgprox_cluster::service::QuotaTransport for GossipTransport {
    async fn request(
        &self,
        leader: NodeId,
        server: &ServerId,
        holder: NodeId,
        want: u32,
    ) -> Result<QuotaLease, QuotaError> {
        // A leader this node has no address for is the same as no leader: it
        // falls back to its guaranteed share, which needs no coordination and
        // cannot breach the cap.
        let peers = self.peers.peers();
        let peer = peers.get(&leader).ok_or(QuotaError::NoLeader)?;
        ask(peer, server, holder, want).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_cluster::coordinator::CoordinatorConfig;
    use pgprox_core::clock::FakeClock;

    fn coordinator(node: u16) -> Arc<GossipCoordinator> {
        GossipCoordinator::new(
            NodeId::new(node),
            CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        )
    }

    #[test]
    fn a_digest_round_trips_through_the_wire_format() {
        let source = coordinator(3);
        source.report(11, vec![(ServerId::new("db-1", 5432), 4)]);
        source.report_tenants(vec![(TenantId::new("acme"), 2)]);
        source.set_mode(NodeMode::Draining);

        let outgoing = source.outgoing();
        let encoded = serde_json::to_string(&Message::Digest(DigestWire::from(&outgoing))).unwrap();
        let Ok(Message::Digest(wire)) = serde_json::from_str::<Message>(&encoded) else {
            panic!("a digest did not decode as one");
        };
        let decoded = wire.parse().unwrap();

        assert_eq!(decoded, outgoing);
    }

    #[test]
    fn a_server_address_that_does_not_parse_is_refused() {
        // Rather than defaulted. A server that became a different server would
        // have its usage counted against the wrong cap, which is the one
        // failure the quota layer has no graceful degradation for.
        let wire = DigestWire {
            node: 1,
            mode: "active".to_owned(),
            version: 1,
            client_conns: 0,
            upstream_conns: vec![("not-an-address".to_owned(), 3)],
            tenant_usage: Vec::new(),
        };

        assert!(wire.parse().is_none());
    }

    #[tokio::test]
    async fn two_nodes_learn_about_each_other_in_one_exchange() {
        // The property the whole transport exists for: before it, every node
        // believed it was alone.
        let (one, two) = (coordinator(1), coordinator(2));
        one.report(5, Vec::new());
        two.report(9, Vec::new());

        let (theirs, ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let two = Arc::clone(&two);
            async move { answer(theirs, &two, &NoCancels).await }
        });
        speak(ours, &one).await.unwrap();

        assert_eq!(
            one.digests()
                .iter()
                .find(|digest| digest.node == NodeId::new(2))
                .map(|digest| digest.client_conns),
            Some(9),
            "the caller did not learn about the peer"
        );
        assert_eq!(
            two.digests()
                .iter()
                .find(|digest| digest.node == NodeId::new(1))
                .map(|digest| digest.client_conns),
            Some(5),
            "the peer did not learn about the caller"
        );
        drop(serving);
    }

    #[tokio::test]
    async fn a_digest_from_a_node_homing_thousands_of_tenants_still_fits() {
        // `M17.4`: `MAX_INCOMING`'s `1024 * 1024` could become `1024 + 1024`
        // and no test noticed, because every digest crossing the wire in a
        // test is a few hundred bytes. A 2 KiB cap truncates the line, the
        // JSON no longer parses, and the peer is answered with a digest while
        // its own is silently dropped: the failure detector then reports a
        // node that is talking to it as one that has gone. The comment above
        // the constant says a node homes at most a few thousand tenants, so
        // that is what this sends.
        let (one, two) = (coordinator(1), coordinator(2));
        let tenants: Vec<(TenantId, u32)> = (0..2_000)
            .map(|i| (TenantId::new(format!("tenant-{i:06}")), 1))
            .collect();
        one.report(5, Vec::new());
        one.report_tenants(tenants);
        assert!(
            encode(&Message::Digest(DigestWire::from(&one.outgoing()))).len() > 4 * 1024,
            "the test's digest is not large enough to prove anything"
        );

        let (theirs, ours) = tokio::io::duplex(1024 * 1024);
        let serving = tokio::spawn({
            let two = Arc::clone(&two);
            async move { answer(theirs, &two, &NoCancels).await }
        });
        speak(ours, &one).await.unwrap();

        assert_eq!(
            two.digests()
                .iter()
                .find(|digest| digest.node == NodeId::new(1))
                .map(|digest| digest.tenant_usage.len()),
            Some(2_000),
            "a large digest was truncated rather than read"
        );
        drop(serving);
    }

    #[tokio::test]
    async fn a_stale_digest_does_not_overwrite_a_newer_one() {
        // Gossip reorders. A node that applied whatever arrived last would
        // flap between two states forever.
        let listener = coordinator(1);
        let peer = coordinator(2);
        peer.report(1, Vec::new());
        let first = peer.outgoing();
        peer.report(2, Vec::new());
        let second = peer.outgoing();

        listener.gossip(second);
        listener.gossip(first);

        assert_eq!(
            listener
                .digests()
                .iter()
                .find(|digest| digest.node == NodeId::new(2))
                .map(|digest| digest.client_conns),
            Some(2),
            "a replayed older digest was applied"
        );
    }

    #[tokio::test]
    async fn a_peer_that_talks_past_the_budget_is_cut_off() {
        // A gossip port is reachable from inside the cluster network, and a
        // peer that is really an attacker must not be able to make a node
        // allocate without limit.
        let node = coordinator(1);
        let (theirs, mut ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let node = Arc::clone(&node);
            async move { answer(theirs, &node, &NoCancels).await }
        });

        // No newline, ever: the reader must stop at the cap rather than buffer.
        let junk = vec![b'x'; 256 * 1024];
        for _ in 0..8 {
            if ours.write_all(&junk).await.is_err() {
                break;
            }
        }
        drop(ours);

        let outcome = tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .expect("the reader never stopped");
        assert!(
            outcome.unwrap().is_err() || node.digests().is_empty(),
            "an oversized message was accepted"
        );
    }

    #[tokio::test]
    async fn a_round_reports_how_many_peers_answered() {
        // A node that could not tell a reached peer from an unreachable one
        // could not report why it is not converging.
        let node = coordinator(1);
        let peer = coordinator(2);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(serve(
            listener,
            Arc::clone(&peer),
            Arc::new(NoCancels),
            async {
                let _ = stopped.await;
            },
        ));

        // One that answers and one that is not there.
        let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_addr = dead.local_addr().unwrap();
        drop(dead);

        assert_eq!(
            round(&[addr.to_string(), dead_addr.to_string()], &node).await,
            1
        );
        assert!(
            node.digests()
                .iter()
                .any(|digest| digest.node == NodeId::new(2))
        );

        stop.send(()).unwrap();
        serving.await.unwrap().unwrap();
    }

    /// A coordinator that is the leader and has served its takeover wait.
    ///
    /// Built the way the real one gets there, by gossiping every second while
    /// the wait elapses: advancing the clock in one jump costs the node its
    /// quorum, which is correct behaviour and the wrong setup.
    fn leading() -> (Arc<GossipCoordinator>, FakeClock) {
        use pgprox_cluster::digest::VersionedDigest;

        let clock = FakeClock::new();
        let coordinator = GossipCoordinator::new(
            NodeId::new(1),
            CoordinatorConfig {
                fleet_size: 3,
                ..CoordinatorConfig::default()
            },
            Arc::new(clock.clone()),
        );
        coordinator.set_cap(ServerId::new("db-1", 5432), 100);

        for round in 1..=12 {
            for node in 1..=3 {
                coordinator.gossip(VersionedDigest {
                    digest: ClusterDigest {
                        node: NodeId::new(node),
                        ..ClusterDigest::default()
                    },
                    version: round,
                });
            }
            clock.advance(Duration::from_secs(1));
        }
        coordinator.tick();
        (coordinator, clock)
    }

    #[tokio::test]
    async fn a_non_leader_obtains_a_lease_through_the_leader() {
        // M6.16 built the rule and left the socket to the composition root,
        // which never wrote one. Until this, every node fell back to its
        // guaranteed share and the free pool was unreachable.
        let (leader, _clock) = leading();
        let server = ServerId::new("db-1", 5432);

        let (theirs, ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let leader = Arc::clone(&leader);
            async move { answer(theirs, &leader, &NoCancels).await }
        });

        let lease = request_over(ours, &server, NodeId::new(2), 3)
            .await
            .expect("the leader said nothing")
            .expect("the leader refused a request it had room for");

        assert_eq!(lease.server(), &server);
        assert_eq!(lease.nominal_count(), 3);
        assert!(
            !lease.is_expired(std::time::Instant::now()),
            "the lease arrived already expired"
        );
        drop(serving);
    }

    #[tokio::test]
    async fn a_node_that_is_not_the_leader_refuses_rather_than_granting() {
        // Two nodes granting from one free pool is the one failure the quota
        // layer has no graceful degradation for.
        let follower = coordinator(2);
        follower.set_cap(ServerId::new("db-1", 5432), 100);

        let (theirs, ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let follower = Arc::clone(&follower);
            async move { answer(theirs, &follower, &NoCancels).await }
        });

        let answered = request_over(ours, &ServerId::new("db-1", 5432), NodeId::new(3), 3)
            .await
            .expect("nothing came back");

        assert_eq!(answered, Err(QuotaError::NoLeader));
        drop(serving);
    }

    #[tokio::test]
    async fn a_quota_request_goes_to_a_leader_whose_address_arrived_late() {
        // `M19.3`. The transport used to hold a table taken at startup, so a
        // leader this node learned about afterwards was a leader it had no
        // address for, which reads as `NoLeader` and drops the node to its
        // guaranteed share. That is the safe direction and it is still wrong:
        // the free pool becomes unreachable for a fleet that is working.
        use pgprox_cluster::service::QuotaTransport as _;

        let (leader, _clock) = leading();
        let server = ServerId::new("db-1", 5432);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(serve(
            listener,
            Arc::clone(&leader),
            Arc::new(NoCancels),
            async {
                let _ = stopped.await;
            },
        ));

        // Built knowing nobody, which is what a node that started first sees.
        let source = pgprox_core::cluster::FakePeerSource::new(std::collections::BTreeMap::new());
        let transport = GossipTransport::new(source.clone());
        assert_eq!(
            transport
                .request(NodeId::new(1), &server, NodeId::new(2), 3)
                .await,
            Err(QuotaError::NoLeader),
            "a leader with no address should read as no leader"
        );

        // The leader's address arrives. Nothing is rebuilt.
        source.publish(std::collections::BTreeMap::from([(
            NodeId::new(1),
            addr.to_string(),
        )]));

        let lease = transport
            .request(NodeId::new(1), &server, NodeId::new(2), 3)
            .await
            .expect("the leader was reachable and refused anyway");
        assert_eq!(lease.server(), &server);

        stop.send(()).unwrap();
        serving.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_leader_with_no_address_is_the_same_as_no_leader() {
        // The safe direction: the asker falls back to its guaranteed share,
        // which needs no coordination and cannot breach the cap.
        use pgprox_cluster::service::QuotaTransport as _;

        let transport = GossipTransport::new(pgprox_core::cluster::StaticPeers::new(
            std::collections::BTreeMap::new(),
        ));
        let refused = transport
            .request(
                NodeId::new(9),
                &ServerId::new("db-1", 5432),
                NodeId::new(2),
                1,
            )
            .await;

        assert_eq!(refused, Err(QuotaError::NoLeader));
    }

    #[tokio::test]
    async fn a_leader_that_does_not_answer_is_the_same_as_no_leader() {
        // A node blocked on an unreachable leader would hold its clients
        // waiting for a connection it could have opened from its own share.
        let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = dead.local_addr().unwrap();
        drop(dead);

        let refused = ask(
            &addr.to_string(),
            &ServerId::new("db-1", 5432),
            NodeId::new(2),
            1,
        )
        .await;
        assert_eq!(refused, Err(QuotaError::NoLeader));
    }

    #[tokio::test]
    async fn a_peer_answers_with_the_clients_it_is_serving() {
        // The one read that fans out. Aggregates come from the digest every
        // node already holds; a client list is one row per connection, so it
        // is asked for when somebody asks rather than gossiped every second.
        #[derive(Debug)]
        struct Serving(Vec<ClientView>);

        #[async_trait::async_trait]
        impl CancelSink for Serving {
            async fn cancel(&self, _conn: ConnId) {}

            fn clients(&self) -> Vec<ClientView> {
                self.0.clone()
            }
        }

        // One of each state. `M17.4`: both halves of the `waiting` arm, the
        // encode and the decode, survived because the only client that ever
        // crossed the wire was active, so `waiting` could have been dropped to
        // `idle` in both directions and this test would still pass. Waiting is
        // the state an operator looks for when the pool is exhausted, which is
        // the one moment the client list is worth asking a peer for.
        let view = ClientView {
            conn: ConnId::new(NodeId::new(2), 9),
            tenant: TenantId::new("acme"),
            node: NodeId::new(2),
            state: ClientState::Active,
            since: Duration::from_millis(1500),
            pinned: Some("listen".to_owned()),
        };
        let waiting = ClientView {
            conn: ConnId::new(NodeId::new(2), 10),
            state: ClientState::Waiting,
            pinned: None,
            ..view.clone()
        };
        let idle = ClientView {
            conn: ConnId::new(NodeId::new(2), 11),
            state: ClientState::Idle,
            pinned: None,
            ..view.clone()
        };
        let served = vec![view.clone(), waiting.clone(), idle.clone()];

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(serve(
            listener,
            coordinator(2),
            Arc::new(Serving(served.clone())),
            async {
                let _ = stopped.await;
            },
        ));

        let answered = clients_of(&addr.to_string()).await.unwrap();

        assert_eq!(answered, served, "a client changed shape on the wire");

        stop.send(()).unwrap();
        serving.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_peer_that_is_not_there_is_an_error_rather_than_an_empty_list() {
        // An operator seeing an empty list concludes there are no clients.
        let dead = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = dead.local_addr().unwrap();
        drop(dead);

        assert!(clients_of(&addr.to_string()).await.is_err());
    }

    #[tokio::test]
    async fn a_message_that_is_not_a_digest_does_not_end_the_conversation() {
        // A rolling upgrade means talking to a node running a different build.
        // Refusing to speak to it at all would partition the fleet by version.
        let node = coordinator(1);
        let (theirs, ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let node = Arc::clone(&node);
            async move { answer(theirs, &node, &NoCancels).await }
        });

        let (read, mut write) = tokio::io::split(ours);
        write
            .write_all(b"{\"something\":\"else\"}\n")
            .await
            .unwrap();
        write.flush().await.unwrap();

        let mut lines = BufReader::new(read).lines();
        let answered = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("nothing came back")
            .unwrap();

        assert!(
            answered.is_some_and(|line| matches!(
                serde_json::from_str::<Message>(&line),
                Ok(Message::Digest(_))
            )),
            "a node that sent something unrecognised got no digest back"
        );
        drop(serving);
    }

    #[tokio::test]
    async fn a_server_at_its_cap_is_told_apart_from_a_fleet_with_no_leader() {
        // `M17.4`: the `reason == "exhausted"` guard could be `false` and no
        // test noticed, which collapses both refusals into `NoLeader`. They
        // are opposite diagnoses. `Exhausted` means the database is at the cap
        // an operator configured and the fix is more capacity; `NoLeader`
        // means gossip is broken and the fix is the network. A node reporting
        // the second while the first is happening sends an operator looking
        // for a partition that is not there.
        let (leader, _clock) = leading();
        let server = ServerId::new("db-1", 5432);

        let (theirs, ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let leader = Arc::clone(&leader);
            async move { answer(theirs, &leader, &NoCancels).await }
        });

        // The free pool is granted down to nothing first: a request larger
        // than what is left is clamped to what is there rather than refused,
        // so exhaustion is the *second* ask, not a large one.
        let taken = request_over(ours, &server, NodeId::new(2), 10_000)
            .await
            .expect("nothing came back")
            .expect("the leader refused a request it had room for");
        assert!(taken.nominal_count() > 0);
        drop(serving);

        let (theirs, ours) = tokio::io::duplex(64 * 1024);
        let serving = tokio::spawn({
            let leader = Arc::clone(&leader);
            async move { answer(theirs, &leader, &NoCancels).await }
        });
        let answered = request_over(ours, &server, NodeId::new(3), 1)
            .await
            .expect("nothing came back");
        assert_eq!(answered, Err(QuotaError::Exhausted { server }));
        drop(serving);
    }

    #[tokio::test]
    async fn a_lease_is_asked_for_over_a_real_socket() {
        // `M17.4`: `ask_over` returning `None` survived, because every quota
        // test drove `request_over` over a duplex and nothing ever opened the
        // connection. `None` is what the caller reads as "no leader", so a
        // node whose dial silently failed would sit on its guaranteed share
        // with the free pool unreachable, which is the exact defect `M6.16`
        // left and this transport exists to fix.
        let (leader, _clock) = leading();
        let server = ServerId::new("db-1", 5432);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(serve(
            listener,
            Arc::clone(&leader),
            Arc::new(NoCancels),
            async {
                let _ = stopped.await;
            },
        ));

        let lease = ask_over(&addr.to_string(), &server, NodeId::new(2), 3)
            .await
            .expect("the dial produced nothing at all")
            .expect("the leader refused a request it had room for");
        assert_eq!(lease.server(), &server);
        assert_eq!(lease.nominal_count(), 3);

        stop.send(()).unwrap();
        serving.await.unwrap().unwrap();
    }
}
