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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pgprox_core::auth::Backend;
use pgprox_core::ids::ServerId;
use pgprox_core::pool::PoolError;

use crate::connect::{PgConnector, Upstream};

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
}
