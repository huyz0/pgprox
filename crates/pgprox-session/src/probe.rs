//! What to tell a client about a server it has not connected to yet.
//!
//! A client finishes authenticating and expects a `ParameterStatus` set before
//! its first `ReadyForQuery`: `server_version`, `client_encoding`, `DateStyle`
//! and the rest. Drivers read them. `server_version` in particular decides
//! which syntax a driver will use, so inventing a value here produces bugs
//! that look like the database is broken.
//!
//! But no upstream connection has been opened at that point, and opening one
//! per client would defeat the entire design: a client that connects and sits
//! idle is supposed to cost a socket and no database connection at all.
//!
//! So the values come from a probe connection, opened once per server and
//! database and then remembered. Every real connection refreshes the entry as
//! it opens, so the probe is only ever paid for the first client of a
//! database.
//!
//! # What is deliberately not passed on
//!
//! `application_name`. The proxy sets its own on upstream connections so a DBA
//! reading `pg_stat_activity` can see which process holds them. Reporting that
//! back to a client would tell it its application is called `pgprox`, silently
//! overwriting what it set in its own startup packet.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pgprox_core::auth::Backend;
use pgprox_core::ids::{Lsn, ServerId};
use pgprox_core::pool::PoolError;
use pgprox_proto::backend::{self, BackendMessage};
use pgprox_proto::encode_frontend;
use pgprox_proto::frame::{Frame, Tag};
use pgprox_route::poller::{Probe, ReplicaProbe};

use crate::connect::{PgConnector, Upstream, Upstreamed};
use crate::shell::Wire;

/// Parameters a client is told about, in the order the server sent them.
pub type Parameters = Arc<[(String, String)]>;

/// Parameters the proxy sets for its own purposes and does not pass on.
///
/// One entry, and it is here rather than inline so the reason travels with the
/// list if it ever grows.
const NOT_THE_CLIENTS: [&str; 1] = ["application_name"];

/// One `ParameterStatus` set per server and database.
#[derive(Debug, Default)]
pub struct ParameterCache {
    entries: Mutex<HashMap<(ServerId, String), Parameters>>,
    probes: AtomicU64,
}

impl ParameterCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many probe connections have been opened.
    ///
    /// Exposed because "the cache works" is not observable any other way, and
    /// a cache that silently stopped hitting would show up as latency rather
    /// than as an error.
    #[must_use]
    pub fn probes(&self) -> u64 {
        self.probes.load(Ordering::Relaxed)
    }

    /// How many server-and-database pairs are known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether nothing is known yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// What a client of this database should be told, if it is known.
    #[must_use]
    pub fn get(&self, server: &ServerId, database: &str) -> Option<Parameters> {
        self.lock()
            .get(&(server.clone(), database.to_owned()))
            .cloned()
    }

    /// Records what a server said, from any connection.
    ///
    /// Called for every connection the connector opens, not only for probes,
    /// so the first real client of a database usually makes the probe
    /// unnecessary for the second.
    pub fn record(&self, server: &ServerId, database: &str, parameters: &[(String, String)]) {
        let kept: Parameters = parameters
            .iter()
            .filter(|(name, _)| !NOT_THE_CLIENTS.contains(&name.as_str()))
            .cloned()
            .collect();
        self.lock()
            .insert((server.clone(), database.to_owned()), kept);
    }

    /// What to tell a client, opening a probe connection if nothing is known.
    ///
    /// # Errors
    ///
    /// Fails when the probe connection cannot be opened, which is the same
    /// failure the client's first query would hit anyway, reported earlier.
    pub async fn ensure<U: Upstream + 'static>(
        &self,
        connector: &PgConnector<U>,
        backend: &Backend,
    ) -> Result<Parameters, PoolError> {
        if let Some(known) = self.get(&backend.server, &backend.database) {
            return Ok(known);
        }

        self.probes.fetch_add(1, Ordering::Relaxed);
        let opened = connector.open(&backend.pool_key()).await?;
        self.record(&backend.server, &backend.database, &opened.parameters);

        // The connection is dropped here rather than kept. It was opened to
        // ask one question, and holding it would mean the first client of a
        // database costs an upstream connection it never uses, which is what
        // this whole module exists to avoid.
        drop(opened);

        self.get(&backend.server, &backend.database)
            .ok_or_else(|| PoolError::ConnectFailed {
                server: backend.server.clone(),
                reason: "the probe connection reported no parameters".to_owned(),
            })
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<(ServerId, String), Parameters>> {
        // Recovered rather than propagated: a poisoned lock here means another
        // thread panicked holding a map of version strings, and taking the
        // node down over that would be the larger failure.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The query a replica is asked, once per poll.
///
/// Two functions in one round trip. `pg_last_wal_replay_lsn()` answers how far
/// it has replayed, and `pg_is_in_recovery()` answers whether it is still a
/// replica at all: a promoted one keeps answering queries and its replay
/// position stops moving, which is the shape of a stale read that never
/// recovers.
pub const REPLICA_QUERY: &str = "SELECT pg_last_wal_replay_lsn(), pg_is_in_recovery()";

/// Asks replicas where they have got to, over a real connection.
///
/// Holds one connection per replica rather than dialing per poll. At a quarter
/// second per poll a fresh TCP, TLS and authentication handshake each time
/// would cost more than the question, and would show up on the database as a
/// login storm from the proxy.
pub struct SqlReplicaProbe<U: Upstream> {
    connector: PgConnector<U>,
    replicas: Vec<Backend>,
    /// One live connection per replica, opened lazily and dropped on failure.
    held: Vec<tokio::sync::Mutex<Option<Upstreamed<U::Stream>>>>,
}

impl<U: Upstream> fmt::Debug for SqlReplicaProbe<U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SqlReplicaProbe")
            .field("replicas", &self.replicas.len())
            .finish_non_exhaustive()
    }
}

impl<U: Upstream + 'static> SqlReplicaProbe<U> {
    /// A prober for the replicas in a grant, in the order the grant lists them.
    ///
    /// The order is the index the router uses, so it has to be preserved
    /// exactly: probing them in a different order would attribute one
    /// replica's position to another and route reads at a replica that never
    /// replayed them.
    #[must_use]
    pub fn new(upstream: U, replicas: Vec<Backend>) -> Self {
        let connector = PgConnector::new(upstream);
        for replica in &replicas {
            connector.learn(replica);
        }
        let held = replicas
            .iter()
            .map(|_| tokio::sync::Mutex::new(None))
            .collect();
        Self {
            connector,
            replicas,
            held,
        }
    }

    /// How many replicas are watched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.replicas.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.replicas.is_empty()
    }

    /// Runs the query on `index`, opening a connection if there is none.
    async fn ask(&self, index: usize) -> Result<Probe, String> {
        let backend = self
            .replicas
            .get(index)
            .ok_or_else(|| format!("no replica at index {index}"))?;
        let slot = self
            .held
            .get(index)
            .ok_or_else(|| format!("no replica at index {index}"))?;
        let mut held = slot.lock().await;

        if held.is_none() {
            *held = Some(
                self.connector
                    .open(&backend.pool_key())
                    .await
                    .map_err(|err| err.to_string())?,
            );
        }
        let connection = held.as_mut().ok_or("the connection vanished")?;

        // A failure drops the connection rather than keeping it, because the
        // most likely reason a query failed is that the connection is no
        // longer usable, and reusing it would fail every poll from here on.
        match run_replica_query(&mut connection.wire).await {
            Ok(probe) => Ok(probe),
            Err(reason) => {
                *held = None;
                Err(reason)
            }
        }
    }
}

#[async_trait::async_trait]
impl<U: Upstream + 'static> ReplicaProbe for SqlReplicaProbe<U> {
    async fn probe(&self, index: usize) -> Result<Probe, String> {
        self.ask(index).await
    }
}

/// Sends the query and reads its answer.
async fn run_replica_query<S>(wire: &mut Wire<S>) -> Result<Probe, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    wire.queue(|out| encode_frontend::query(out, REPLICA_QUERY));
    wire.flush().await.map_err(|err| err.to_string())?;

    let mut body = Vec::new();
    let mut row: Option<Vec<Option<String>>> = None;

    loop {
        let tag = wire
            .read_tagged(&mut body)
            .await
            .map_err(|err| err.to_string())?;

        if tag == Tag::DATA_ROW {
            // The one place this project parses a DataRow, and it is parsing
            // its own query's answer rather than a tenant's result. Relayed
            // rows stay opaque; see pgprox-proto's module docs.
            row = Some(text_row(&body).ok_or("the replica sent an unreadable row")?);
        } else if tag == Tag::ERROR_RESPONSE {
            let frame = Frame::new(tag, &body);
            let reason = match backend::decode(&frame) {
                Ok(BackendMessage::ErrorResponse(error)) => {
                    format!("{} ({})", error.message, error.code)
                }
                _ => "the replica refused the query".to_owned(),
            };
            return Err(reason);
        } else if tag == Tag::READY_FOR_QUERY {
            break;
        }
    }

    let row = row.ok_or("the replica answered with no rows")?;
    let replayed = match row.first().and_then(Option::as_deref) {
        // NULL, which is what a server that is not replaying answers. Not an
        // error: pg_is_in_recovery below is what decides whether this is still
        // a replica, and reporting the pair honestly lets the router take it
        // out of service for the right reason.
        None => Lsn::ZERO,
        Some(text) => text
            .parse()
            .map_err(|_| format!("the replica reported an unparseable LSN: {text}"))?,
    };
    let in_recovery = matches!(row.get(1).and_then(Option::as_deref), Some("t"));

    Ok(Probe {
        replayed,
        in_recovery,
    })
}

/// What the proxy asks the primary after a write.
///
/// `pg_current_wal_insert_lsn()` rather than `pg_current_wal_lsn()`: the
/// insert position is at or ahead of the flush position, so a watermark taken
/// from it can only be conservative, and conservative here means a replica is
/// held ineligible slightly too long rather than serving a read that predates
/// the write.
pub const PRIMARY_LSN_QUERY: &str = "SELECT pg_current_wal_insert_lsn()";

/// Asks the primary where the write it just ran landed.
///
/// One round trip on the connection the session already holds, run only for a
/// transaction that wrote. A session that only reads never pays it.
///
/// # Errors
///
/// Fails when the connection does or the answer cannot be read. The caller
/// treats that as "the position is unknown", which leaves the watermark where
/// it was: the session keeps reading from the primary until it learns
/// otherwise, which is the safe direction.
pub async fn primary_lsn<S>(wire: &mut Wire<S>) -> Result<Lsn, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    wire.queue(|out| encode_frontend::query(out, PRIMARY_LSN_QUERY));
    wire.flush().await.map_err(|err| err.to_string())?;

    let mut body = Vec::new();
    let mut row: Option<Vec<Option<String>>> = None;

    loop {
        let tag = wire
            .read_tagged(&mut body)
            .await
            .map_err(|err| err.to_string())?;

        if tag == Tag::DATA_ROW {
            row = Some(text_row(&body).ok_or("the primary sent an unreadable row")?);
        } else if tag == Tag::ERROR_RESPONSE {
            return Err("the primary refused the position query".to_owned());
        } else if tag == Tag::READY_FOR_QUERY {
            break;
        }
    }

    match row.and_then(|row| row.into_iter().next()).flatten() {
        Some(text) => text
            .parse()
            .map_err(|_| format!("the primary reported an unparseable LSN: {text}")),
        // NULL, which a server not writing WAL answers. Unknown rather than
        // zero: a watermark of zero admits every replica, which is the exact
        // opposite of what an unknown position should mean.
        None => Err("the primary reported no position".to_owned()),
    }
}

/// Splits a `DataRow` body into its text fields.
///
/// `None` is a SQL NULL, which the wire encodes as a length of -1 rather than
/// as an empty value. Treating the two the same would read a NULL replay
/// position as LSN zero, and a replica at zero is a replica that has replayed
/// nothing, which is a very different claim.
fn text_row(body: &[u8]) -> Option<Vec<Option<String>>> {
    let count = i16::from_be_bytes(body.get(..2)?.try_into().ok()?);
    let mut at = 2;
    let mut fields = Vec::with_capacity(usize::try_from(count.max(0)).ok()?);

    for _ in 0..count.max(0) {
        let len = i32::from_be_bytes(body.get(at..at + 4)?.try_into().ok()?);
        at += 4;
        if len < 0 {
            fields.push(None);
            continue;
        }
        let len = usize::try_from(len).ok()?;
        let value = body.get(at..at + len)?;
        at += len;
        fields.push(Some(String::from_utf8(value.to_vec()).ok()?));
    }

    Some(fields)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_core::auth::TlsMode;
    use pgprox_core::secret::SecretString;

    use crate::connect::UpstreamScram;
    use pgprox_proto::encode;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, duplex};

    fn server() -> ServerId {
        ServerId::new("db-1", 5432)
    }

    fn backend(database: &str) -> Backend {
        Backend {
            server: server(),
            database: database.into(),
            user: "acme_app".into(),
            password: SecretString::new("hunter2"),
            tls: TlsMode::Disabled,
        }
    }

    /// A dialer that answers every connection with a scripted startup.
    ///
    /// It speaks the real protocol rather than pretending to: the connector
    /// under test decodes every byte of it.
    #[derive(Debug, Default)]
    struct Scripted {
        version: &'static str,
        dials: AtomicU64,
    }

    #[async_trait::async_trait]
    impl Upstream for Scripted {
        type Stream = DuplexStream;

        async fn dial(&self, _backend: &Backend) -> Result<Self::Stream, PoolError> {
            self.dials.fetch_add(1, Ordering::Relaxed);
            let (ours, mut theirs) = duplex(4096);
            let version = self.version.to_owned();

            tokio::spawn(async move {
                // The startup packet, whatever it is.
                let mut len = [0_u8; 4];
                theirs.read_exact(&mut len).await.unwrap();
                let mut body = vec![0; u32::from_be_bytes(len) as usize - 4];
                theirs.read_exact(&mut body).await.unwrap();

                let mut out = Vec::new();
                encode::authentication_ok(&mut out);
                encode::parameter_status(&mut out, "server_version", &version);
                encode::parameter_status(&mut out, "application_name", "pgprox");
                encode::ready_for_query(&mut out, pgprox_proto::backend::TxStatus::Idle);
                theirs.write_all(&out).await.unwrap();
                // Held open until the connector drops its end.
                let mut sink = Vec::new();
                let _ = theirs.read_to_end(&mut sink).await;
            });

            Ok(ours)
        }

        fn scram(&self) -> Box<dyn UpstreamScram> {
            unreachable!("this server never asks for SASL")
        }
    }

    /// A dialer that refuses.
    #[derive(Debug)]
    struct Unreachable;

    #[async_trait::async_trait]
    impl Upstream for Unreachable {
        type Stream = DuplexStream;

        async fn dial(&self, backend: &Backend) -> Result<Self::Stream, PoolError> {
            Err(PoolError::ConnectFailed {
                server: backend.server.clone(),
                reason: "connection refused".to_owned(),
            })
        }

        fn scram(&self) -> Box<dyn UpstreamScram> {
            unreachable!("this server is never reached")
        }
    }

    fn connector(version: &'static str) -> PgConnector<Scripted> {
        let connector = PgConnector::new(Scripted {
            version,
            dials: AtomicU64::new(0),
        });
        for database in ["acme", "globex"] {
            connector.learn(&backend(database));
        }
        connector
    }

    #[tokio::test]
    async fn the_first_client_of_a_database_pays_for_a_probe() {
        let cache = ParameterCache::new();
        let connector = connector("17.2");

        let parameters = cache.ensure(&connector, &backend("acme")).await.unwrap();
        assert_eq!(cache.probes(), 1);
        assert!(
            parameters
                .iter()
                .any(|(name, value)| name == "server_version" && value == "17.2"),
            "the probe reported no server version: {parameters:?}"
        );
    }

    #[tokio::test]
    async fn a_second_pool_for_the_same_database_opens_no_second_probe() {
        // The acceptance criterion. Without the cache every new pool would
        // open a connection to ask a question whose answer has not changed.
        let cache = ParameterCache::new();
        let connector = connector("17.2");

        cache.ensure(&connector, &backend("acme")).await.unwrap();
        cache.ensure(&connector, &backend("acme")).await.unwrap();

        assert_eq!(cache.probes(), 1, "the second pool opened its own probe");
    }

    #[tokio::test]
    async fn a_different_database_on_the_same_server_is_probed_separately() {
        // Two databases on one host can report different values, notably
        // client_encoding and DateStyle, so the key is the pair.
        let cache = ParameterCache::new();
        let connector = connector("17.2");

        cache.ensure(&connector, &backend("acme")).await.unwrap();
        cache.ensure(&connector, &backend("globex")).await.unwrap();

        assert_eq!(cache.probes(), 2);
        assert_eq!(cache.len(), 2);
    }

    #[tokio::test]
    async fn a_real_connection_saves_the_next_client_a_probe() {
        // Every connection the connector opens reports its parameters, so the
        // probe is only ever paid for a database nothing has reached yet.
        let cache = ParameterCache::new();
        let connector = connector("17.2");

        let opened = connector.open(&backend("acme").pool_key()).await.unwrap();
        cache.record(&server(), "acme", &opened.parameters);

        cache.ensure(&connector, &backend("acme")).await.unwrap();
        assert_eq!(cache.probes(), 0, "a known database was probed anyway");
    }

    #[tokio::test]
    async fn the_proxys_own_application_name_is_not_reported_to_the_client() {
        // The proxy names its upstream connections so a DBA can see them in
        // pg_stat_activity. Passing that back would tell the client its
        // application is called pgprox, overwriting what it set itself.
        let cache = ParameterCache::new();
        let connector = connector("17.2");

        let parameters = cache.ensure(&connector, &backend("acme")).await.unwrap();
        assert!(
            !parameters
                .iter()
                .any(|(name, _)| name == "application_name"),
            "the proxy told the client its own application name: {parameters:?}"
        );
    }

    #[tokio::test]
    async fn a_probe_that_cannot_connect_reports_it_rather_than_caching_nothing() {
        // Caching an empty answer would mean every later client of this
        // database silently gets no parameters at all, which presents as a
        // driver misbehaving rather than as a server being down.
        let cache = ParameterCache::new();
        let connector = PgConnector::new(Unreachable);
        connector.learn(&backend("acme"));

        assert!(cache.ensure(&connector, &backend("acme")).await.is_err());
        assert!(cache.is_empty(), "a failed probe left an entry behind");
    }

    #[test]
    fn recording_the_same_database_twice_replaces_rather_than_grows() {
        // A server can be upgraded under a running proxy, and the newer answer
        // is the true one.
        let cache = ParameterCache::new();
        cache.record(
            &server(),
            "acme",
            &[("server_version".into(), "17.2".into())],
        );
        cache.record(
            &server(),
            "acme",
            &[("server_version".into(), "18.0".into())],
        );

        let parameters = cache.get(&server(), "acme").unwrap();
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].1, "18.0");
    }

    /// A replica that answers the probe query with a scripted row.
    #[derive(Debug)]
    struct Replica {
        /// The replay position, or None for a SQL NULL.
        replayed: Option<&'static str>,
        in_recovery: bool,
        /// Answer the first query with an error instead.
        refuse: bool,
    }

    fn data_row(values: &[Option<&str>]) -> Vec<u8> {
        let mut body = i16::try_from(values.len()).unwrap().to_be_bytes().to_vec();
        for value in values {
            match value {
                None => body.extend_from_slice(&(-1_i32).to_be_bytes()),
                Some(text) => {
                    body.extend_from_slice(&i32::try_from(text.len()).unwrap().to_be_bytes());
                    body.extend_from_slice(text.as_bytes());
                }
            }
        }
        let mut out = vec![Tag::DATA_ROW.get()];
        out.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[async_trait::async_trait]
    impl Upstream for Replica {
        type Stream = DuplexStream;

        async fn dial(&self, _backend: &Backend) -> Result<Self::Stream, PoolError> {
            let (ours, mut theirs) = duplex(4096);
            let replayed = self.replayed;
            let in_recovery = self.in_recovery;
            let refuse = self.refuse;

            tokio::spawn(async move {
                // The startup packet.
                let mut len = [0_u8; 4];
                theirs.read_exact(&mut len).await.unwrap();
                let mut body = vec![0; u32::from_be_bytes(len) as usize - 4];
                theirs.read_exact(&mut body).await.unwrap();

                let mut out = Vec::new();
                encode::authentication_ok(&mut out);
                encode::ready_for_query(&mut out, pgprox_proto::backend::TxStatus::Idle);
                theirs.write_all(&out).await.unwrap();

                // Then answer every query the same way.
                loop {
                    let mut header = [0_u8; 5];
                    if theirs.read_exact(&mut header).await.is_err() {
                        return;
                    }
                    let len = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
                    let mut body = vec![0; len - 4];
                    if theirs.read_exact(&mut body).await.is_err() {
                        return;
                    }

                    let mut out = Vec::new();
                    if refuse {
                        out.push(Tag::ERROR_RESPONSE.get());
                        let fields = b"SERROR\0C57P03\0Mthe database system is starting up\0\0";
                        out.extend_from_slice(
                            &u32::try_from(fields.len() + 4).unwrap().to_be_bytes(),
                        );
                        out.extend_from_slice(fields);
                    } else {
                        out.extend_from_slice(&data_row(&[
                            replayed,
                            Some(if in_recovery { "t" } else { "f" }),
                        ]));
                    }
                    encode::ready_for_query(&mut out, pgprox_proto::backend::TxStatus::Idle);
                    if theirs.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });

            Ok(ours)
        }

        fn scram(&self) -> Box<dyn UpstreamScram> {
            unreachable!("this replica never asks for SASL")
        }
    }

    fn prober(replica: Replica, count: usize) -> SqlReplicaProbe<Replica> {
        let replicas = (0..count).map(|n| backend(&format!("r{n}"))).collect();
        SqlReplicaProbe::new(replica, replicas)
    }

    #[tokio::test]
    async fn a_replica_reports_its_replay_position() {
        let prober = prober(
            Replica {
                replayed: Some("16/B374D848"),
                in_recovery: true,
                refuse: false,
            },
            1,
        );

        assert_eq!(
            prober.probe(0).await.unwrap(),
            Probe {
                replayed: "16/B374D848".parse().unwrap(),
                in_recovery: true,
            }
        );
    }

    #[tokio::test]
    async fn a_promoted_replica_reports_that_it_is_no_longer_one() {
        // The failure this query's second half exists for. A promoted replica
        // keeps answering and its replay position stops moving, which is a
        // stale read that never recovers.
        let prober = prober(
            Replica {
                replayed: None,
                in_recovery: false,
                refuse: false,
            },
            1,
        );

        let probe = prober.probe(0).await.unwrap();
        assert!(!probe.in_recovery);
        assert_eq!(probe.replayed, Lsn::ZERO);
    }

    #[tokio::test]
    async fn a_replica_that_refuses_the_query_is_a_failure_rather_than_a_reading() {
        let prober = prober(
            Replica {
                replayed: Some("16/B374D848"),
                in_recovery: true,
                refuse: true,
            },
            1,
        );

        let err = prober.probe(0).await.unwrap_err();
        assert!(err.contains("57P03"), "{err}");
    }

    #[tokio::test]
    async fn an_index_that_names_no_replica_fails_rather_than_reading_another() {
        // Attributing one replica's position to another routes reads at a
        // replica that never replayed them.
        let prober = prober(
            Replica {
                replayed: Some("0/1"),
                in_recovery: true,
                refuse: false,
            },
            1,
        );

        assert!(prober.probe(7).await.is_err());
        assert_eq!(prober.len(), 1);
        assert!(!prober.is_empty());
    }

    #[tokio::test]
    async fn polling_twice_reuses_one_connection() {
        // At four polls a second per replica, dialing each time would cost a
        // TCP, TLS and authentication handshake per question and would look
        // like a login storm from the database's side.
        let prober = prober(
            Replica {
                replayed: Some("0/10"),
                in_recovery: true,
                refuse: false,
            },
            1,
        );

        prober.probe(0).await.unwrap();
        prober.probe(0).await.unwrap();
        assert_eq!(
            prober.connector.known(),
            1,
            "the prober learned a second backend for one replica"
        );
    }

    #[test]
    fn a_null_field_is_not_an_empty_one() {
        // The wire encodes NULL as a length of -1. Reading it as an empty
        // string would make a NULL replay position parse as LSN zero, which
        // claims the replica has replayed nothing rather than that it is not
        // replaying at all.
        let row = data_row(&[None, Some("t")]);
        let fields = text_row(&row[5..]).unwrap();

        assert_eq!(fields, vec![None, Some("t".to_owned())]);
    }

    #[test]
    fn a_truncated_row_is_rejected_rather_than_panicking() {
        // These bytes come from the network like any others.
        let row = data_row(&[Some("16/B374D848"), Some("t")]);
        for cut in 5..row.len() {
            assert!(
                text_row(&row[5..cut]).is_none(),
                "a row truncated at {cut} was read as complete"
            );
        }
    }

    #[test]
    fn a_row_with_no_fields_reads_as_no_fields() {
        assert_eq!(text_row(&[0, 0]), Some(Vec::new()));
    }

    /// Answers one query with the given frames, as a server would.
    ///
    /// Returns what `primary_lsn` made of them, over a real `Wire`.
    async fn primary_answering(frames: Vec<u8>) -> Result<Lsn, String> {
        let (ours, mut theirs) = duplex(4096);
        tokio::spawn(async move {
            // The query itself, which is read and discarded: what is under
            // test is the reading of the answer.
            let mut header = [0_u8; 5];
            if theirs.read_exact(&mut header).await.is_err() {
                return;
            }
            let len = u32::from_be_bytes(header[1..].try_into().unwrap_or([0; 4])) as usize;
            let mut body = vec![0; len.saturating_sub(4)];
            let _ = theirs.read_exact(&mut body).await;
            let _ = theirs.write_all(&frames).await;
        });

        primary_lsn(&mut Wire::new(ours)).await
    }

    fn ready() -> Vec<u8> {
        let mut out = Vec::new();
        encode::ready_for_query(&mut out, pgprox_proto::backend::TxStatus::Idle);
        out
    }

    #[tokio::test]
    async fn the_primary_s_position_is_read_back() {
        let mut frames = data_row(&[Some("16/B374D848")]);
        frames.extend_from_slice(&ready());

        assert_eq!(
            primary_answering(frames).await,
            Ok("16/B374D848".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn a_null_position_is_unknown_rather_than_zero() {
        // A watermark of zero admits every replica, which is the exact
        // opposite of what an unknown position should mean.
        let mut frames = data_row(&[None]);
        frames.extend_from_slice(&ready());

        assert!(primary_answering(frames).await.is_err());
    }

    #[tokio::test]
    async fn a_position_that_does_not_parse_is_an_error() {
        let mut frames = data_row(&[Some("not an lsn")]);
        frames.extend_from_slice(&ready());

        let err = primary_answering(frames).await.unwrap_err();
        assert!(err.contains("not an lsn"), "{err}");
    }

    #[tokio::test]
    async fn a_refused_position_query_is_an_error_rather_than_a_hang() {
        // A primary that refuses the query still has to end the exchange, or
        // the session waits on it holding a connection.
        let mut frames = vec![Tag::ERROR_RESPONSE.get()];
        let fields = b"SERROR\0C42883\0Mfunction does not exist\0\0";
        frames.extend_from_slice(&u32::try_from(fields.len() + 4).unwrap().to_be_bytes());
        frames.extend_from_slice(fields);
        frames.extend_from_slice(&ready());

        assert!(primary_answering(frames).await.is_err());
    }

    #[tokio::test]
    async fn an_answer_with_no_row_is_an_error() {
        assert!(primary_answering(ready()).await.is_err());
    }
}
