//! The replicas a node is watching, and the loops that watch them.
//!
//! # Why this is keyed by the primary
//!
//! A grant names a primary and its replicas together, and the set is a
//! property of the database rather than of the client that happened to
//! present the grant. Two tenants on the same primary therefore share one
//! watch and one poll loop, which is what stops a thousand sessions becoming a
//! thousand `pg_last_wal_replay_lsn()` queries per second.
//!
//! # The watch is created by the first grant that names it
//!
//! Nothing in the configuration document lists replicas: they arrive from the
//! sidecar with the credentials to reach them. So the set is learned, and the
//! poll loop for it starts the first time a session presents one.
//!
//! # A replica nobody has polled yet is not eligible
//!
//! `Replicas::new` starts every entry unhealthy, and a session routing before
//! the first poll completes therefore goes to the primary. That is the safe
//! direction and it is why the watch is registered before its loop is spawned
//! rather than after.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use pgprox_core::auth::{Backend, Grant};
use pgprox_core::clock::Clock;
use pgprox_core::ids::ServerId;
use pgprox_route::poller::ReplicaWatch;
use pgprox_route::replica::ReplicaConfig;
use pgprox_session::probe::SqlReplicaProbe;

use crate::dial::TcpUpstream;
use crate::run::Shutdown;

/// How often each replica is asked where it has got to.
///
/// A quarter second: the freshness window in `ReplicaConfig` is what decides
/// when a reading goes stale, and polling must be comfortably inside it or a
/// healthy replica drops out between polls.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Every replica set this node has been told about.
pub struct ReplicaSets {
    watches: Mutex<HashMap<ServerId, Arc<ReplicaWatch>>>,
    upstream: TcpUpstream,
    clock: Arc<dyn Clock>,
    shutdown: Shutdown,
    /// The node's buffer slab, which a probe's connection borrows from like
    /// any other.
    slab: Arc<pgprox_core::buf::BufferSlab>,
}

impl std::fmt::Debug for ReplicaSets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicaSets")
            .field("sets", &self.lock().len())
            .finish_non_exhaustive()
    }
}

impl ReplicaSets {
    /// A registry that has been told about nothing yet.
    #[must_use]
    pub fn new(
        upstream: TcpUpstream,
        clock: Arc<dyn Clock>,
        shutdown: Shutdown,
        slab: Arc<pgprox_core::buf::BufferSlab>,
    ) -> Self {
        Self {
            watches: Mutex::new(HashMap::new()),
            slab,
            upstream,
            clock,
            shutdown,
        }
    }

    /// How many sets are being watched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// The watch for this grant's replicas, starting its poll loop if this is
    /// the first grant to name them.
    ///
    /// Returns `None` for a grant with no replicas, which is most of them: a
    /// tenant with one database has nothing to route to.
    pub fn watch_for(&self, grant: &Grant) -> Option<Arc<ReplicaWatch>> {
        if grant.replicas.is_empty() {
            return None;
        }

        let key = grant.primary.server.clone();
        let mut watches = self.lock();
        if let Some(existing) = watches.get(&key) {
            return Some(Arc::clone(existing));
        }

        let watch = ReplicaWatch::new(
            grant.replicas.len(),
            ReplicaConfig::default(),
            Arc::clone(&self.clock),
        );
        watches.insert(key, Arc::clone(&watch));
        // Registered before the loop starts, so a session that routes in
        // between reads a watch where nothing is eligible yet and goes to the
        // primary.
        drop(watches);

        tokio::spawn(poll(
            Arc::clone(&watch),
            Arc::new(SqlReplicaProbe::new(
                self.upstream.clone(),
                grant.replicas.clone(),
                Arc::clone(&self.slab),
            )),
            self.shutdown.clone(),
        ));
        Some(watch)
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ServerId, Arc<ReplicaWatch>>> {
        self.watches.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Polls one replica set until the node stops.
///
/// A failed probe is a stale reading with an age rather than a silent zero:
/// `ReplicaWatch::poll_once` records the failure, and a replica whose reading
/// has aged out stops being eligible on its own.
async fn poll(
    watch: Arc<ReplicaWatch>,
    probe: Arc<SqlReplicaProbe<TcpUpstream>>,
    shutdown: Shutdown,
) {
    let mut ticks = tokio::time::interval(POLL_INTERVAL);
    loop {
        tokio::select! {
            () = shutdown.waited() => return,
            _ = ticks.tick() => {}
        }
        watch.poll_once(&probe).await;
    }
}

/// Which backend a routing decision names.
///
/// A `RouteTarget` is an index into the grant's replica list, and the pool is
/// keyed by backend. This is the one place the two meet, so a replica index
/// that does not exist resolves to the primary rather than to a panic.
#[must_use]
pub fn backend_for(grant: &Grant, target: pgprox_core::route::RouteTarget) -> &Backend {
    match target {
        pgprox_core::route::RouteTarget::Replica(index) => {
            grant.replicas.get(index).unwrap_or(&grant.primary)
        }
        _ => &grant.primary,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    /// A slab for a test wire. Small on purpose: the bound is what makes an
    /// exhausted slab reachable in a test at all.
    fn test_slab() -> std::sync::Arc<pgprox_core::buf::BufferSlab> {
        pgprox_core::buf::BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8)
    }
    use super::*;
    use pgprox_core::auth::{ClaimSet, PoolHints, TlsMode};
    use pgprox_core::clock::FakeClock;
    use pgprox_core::ids::TenantId;
    use pgprox_core::route::RouteTarget;
    use pgprox_core::secret::SecretString;

    fn backend(host: &str) -> Backend {
        Backend {
            server: ServerId::new(host, 5432),
            database: "acme".into(),
            user: "acme_app".into(),
            password: SecretString::new("hunter2"),
            tls: TlsMode::Disabled,
        }
    }

    fn grant(replicas: usize) -> Grant {
        Grant {
            tenant: TenantId::new("acme"),
            primary: backend("db-1"),
            replicas: (0..replicas)
                .map(|n| backend(&format!("db-r{n}")))
                .collect(),
            pool: PoolHints::default(),
            ttl: Duration::from_secs(60),
            claims: ClaimSet::default(),
        }
    }

    fn sets() -> ReplicaSets {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        ReplicaSets::new(
            TcpUpstream::new(tls),
            Arc::new(FakeClock::new()),
            Shutdown::new(),
            test_slab(),
        )
    }

    #[test]
    fn an_empty_set_registry_counts_zero_and_says_so() {
        // `M17.4`. `len` returning 1 and `is_empty` returning true both
        // survived: nothing asked either question of an empty registry, and a
        // registry that always claims to hold one set would start a poll loop
        // for replicas nobody configured.
        let sets = sets();
        assert_eq!(sets.len(), 0);
        assert!(sets.is_empty());

        // And `Debug` says how many rather than returning nothing, which is
        // what an operator reading a panic message needs.
        let rendered = format!("{sets:?}");
        assert!(
            rendered.contains("ReplicaSets"),
            "the debug output names nothing: {rendered}"
        );
    }

    #[tokio::test]
    async fn a_grant_with_no_replicas_gets_no_watch() {
        // Which is most grants. A watch with nothing in it would still cost a
        // poll loop.
        let sets = sets();
        assert!(sets.watch_for(&grant(0)).is_none());
        assert!(sets.is_empty());
    }

    #[tokio::test]
    async fn two_tenants_on_one_primary_share_one_watch() {
        // Otherwise a thousand sessions become a thousand
        // pg_last_wal_replay_lsn() queries a second, which is a login storm
        // the database sees as an incident.
        let sets = sets();
        let first = sets.watch_for(&grant(2)).unwrap();
        let second = sets
            .watch_for(&Grant {
                tenant: TenantId::new("globex"),
                ..grant(2)
            })
            .unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(sets.len(), 1);
        // `M17.4`: `is_empty` returning true survived, because the two tests
        // that call it both hold an empty registry. A registry that always
        // claims to be empty reports no replica polling on a node doing it.
        assert!(!sets.is_empty());
    }

    #[tokio::test]
    async fn a_replica_nobody_has_polled_is_not_eligible() {
        // The safe direction, and why the watch is registered before its loop
        // starts rather than after.
        let sets = sets();
        let watch = sets.watch_for(&grant(2)).unwrap();

        assert_eq!(watch.len(), 2);
        assert!(
            watch.states().iter().all(|state| !state.healthy),
            "a replica was eligible before anything had asked it anything"
        );
    }

    #[test]
    fn a_replica_index_the_grant_does_not_have_resolves_to_the_primary() {
        // The two sides of this were built by different milestones against
        // different types. The primary is the answer that cannot be wrong.
        let grant = grant(1);

        assert_eq!(
            backend_for(&grant, RouteTarget::Replica(0)).server,
            ServerId::new("db-r0", 5432)
        );
        assert_eq!(
            backend_for(&grant, RouteTarget::Replica(7)).server,
            ServerId::new("db-1", 5432)
        );
        assert_eq!(
            backend_for(&grant, RouteTarget::Primary).server,
            ServerId::new("db-1", 5432)
        );
    }
}
