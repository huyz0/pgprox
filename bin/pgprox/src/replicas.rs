//! The replicas a node is watching, and the loops that watch them.
//!
//! # Why this is keyed by the primary and the list together
//!
//! A grant names a primary and its replicas together, and the set is a
//! property of the database rather than of the client that happened to
//! present the grant. Two tenants on the same primary therefore share one
//! watch and one poll loop, which is what stops a thousand sessions becoming a
//! thousand `pg_last_wal_replay_lsn()` queries per second.
//!
//! The key is the primary **and the ordered replica list**, and the second half
//! is `M69.0`. It was the primary alone, so the first grant to name a primary
//! fixed its replica set for the life of the process: `watch_for` returned the
//! existing watch and discarded the new grant's list. A replica added later was
//! never polled and never routed to, and a set that came back in a different
//! order was worse than that.
//!
//! `RouteTarget::Replica` is an index. The eligibility check reads slot `i` of
//! the watch and [`backend_for`] resolves `i` against the session's own grant,
//! so the two agree only while both lists are the same. The proto is explicit
//! that they need not be: *"Read replicas, in no particular order."* Under a
//! reordering, the router would clear a read against one host's replay position
//! and then send it to another. Keying on the list makes a changed list a
//! different watch, so the pair a session holds is always one generation.
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
//!
//! # Generations are evicted, or keying on the list would be a leak
//!
//! One key per primary was self-limiting. One key per list is not: every
//! topology change adds a generation that nothing would ever remove, and its
//! poll loop would go on querying hosts no session can reach. So a watch that
//! no session holds and that no grant has asked for in `WATCH_GRACE` is
//! dropped, and its loop notices and stops.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

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

/// How long an unused generation is kept before it is dropped.
///
/// Long enough that a primary whose sessions all reconnect at once does not
/// rebuild its watch from cold and route to the primary for a poll interval
/// while it warms. Short enough that a topology that changes every few minutes
/// does not accumulate loops. It is not a correctness bound in either
/// direction: a watch rebuilt too eagerly starts unhealthy, which routes to the
/// primary, and one kept too long is only a query nobody reads.
const WATCH_GRACE: Duration = Duration::from_secs(60);

/// One generation of one primary's replica set.
///
/// The list is part of the key rather than of the value, so a grant naming a
/// different list cannot find this entry. See the module docs.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct WatchKey {
    primary: ServerId,
    replicas: Box<[ServerId]>,
}

impl WatchKey {
    /// The key a grant asks for.
    fn of(grant: &Grant) -> Self {
        Self {
            primary: grant.primary.server.clone(),
            replicas: grant
                .replicas
                .iter()
                .map(|replica| replica.server.clone())
                .collect(),
        }
    }
}

/// A watch and when a grant last asked for it.
struct Watched {
    watch: Arc<ReplicaWatch>,
    last_used: Instant,
}

/// Every replica set this node has been told about.
pub struct ReplicaSets {
    watches: Mutex<HashMap<WatchKey, Watched>>,
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

        let key = WatchKey::of(grant);
        let now = self.clock.now();
        let mut watches = self.lock();

        // Before the lookup rather than after, so a generation replaced by this
        // very call is gone by the time the map grows again.
        Self::evict_unused(&mut watches, now);

        if let Some(existing) = watches.get_mut(&key) {
            existing.last_used = now;
            return Some(Arc::clone(&existing.watch));
        }

        let watch = ReplicaWatch::new(
            grant.replicas.len(),
            ReplicaConfig::default(),
            Arc::clone(&self.clock),
        );
        watches.insert(
            key,
            Watched {
                watch: Arc::clone(&watch),
                last_used: now,
            },
        );
        // Registered before the loop starts, so a session that routes in
        // between reads a watch where nothing is eligible yet and goes to the
        // primary.
        drop(watches);

        tokio::spawn(poll(
            // Weak, so the map's own reference is the one that decides whether
            // this generation is still wanted. A loop holding a strong one
            // would keep every generation it ever polled alive and make the
            // eviction below unable to fire.
            Arc::downgrade(&watch),
            Arc::new(SqlReplicaProbe::new(
                self.upstream.clone(),
                grant.replicas.clone(),
                Arc::clone(&self.slab),
            )),
            self.shutdown.clone(),
        ));
        Some(watch)
    }

    /// The primary a server is a replica of, as some live grant named it.
    ///
    /// Exists so the quota loop can give a replica the cap of the primary it
    /// replicates. Nothing in the configuration document lists replicas, so
    /// without this a replica has no declared cap and no way to acquire one.
    ///
    /// Ambiguity resolves to nothing. A host that appears under two primaries
    /// is not a replica of either in any sense this can act on, and inheriting
    /// one of the two caps arbitrarily would be a cap chosen by iteration
    /// order.
    #[must_use]
    pub fn primary_of(&self, server: &ServerId) -> Option<ServerId> {
        let watches = self.lock();
        let mut found: Option<&ServerId> = None;
        for key in watches.keys() {
            if key.replicas.iter().any(|replica| replica == server) {
                if found.is_some_and(|primary| primary != &key.primary) {
                    return None;
                }
                found = Some(&key.primary);
            }
        }
        found.cloned()
    }

    /// Drops generations no session holds and no grant has asked for lately.
    ///
    /// Both conditions, not either. A session keeps its watch for its whole
    /// life and may sit idle far longer than the grace period, so the strong
    /// count is what says whether anybody would notice; the timestamp only
    /// stops a set from being rebuilt from cold between two sessions that
    /// arrive back to back.
    fn evict_unused(watches: &mut HashMap<WatchKey, Watched>, now: Instant) {
        watches.retain(|_, watched| {
            Arc::strong_count(&watched.watch) > 1
                || now.duration_since(watched.last_used) < WATCH_GRACE
        });
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<WatchKey, Watched>> {
        self.watches.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Polls one replica set until the node stops or the set is superseded.
///
/// A failed probe is a stale reading with an age rather than a silent zero:
/// `ReplicaWatch::poll_once` records the failure, and a replica whose reading
/// has aged out stops being eligible on its own.
///
/// The watch is held weakly and the loop ends when it can no longer be
/// upgraded, which is how a generation dropped by `evict_unused` stops
/// querying. Upgrading per tick rather than holding a strong reference across
/// the await is the whole mechanism: a strong one would mean no generation is
/// ever evictable and the map would grow with every topology change.
async fn poll(
    watch: Weak<ReplicaWatch>,
    probe: Arc<SqlReplicaProbe<TcpUpstream>>,
    shutdown: Shutdown,
) {
    let mut ticks = tokio::time::interval(POLL_INTERVAL);
    loop {
        tokio::select! {
            () = shutdown.waited() => return,
            _ = ticks.tick() => {}
        }
        let Some(watch) = watch.upgrade() else { return };
        watch.poll_once(&probe).await;
    }
}

/// Which backend a routing decision names.
///
/// A `RouteTarget` is an index into the grant's replica list, and the pool is
/// keyed by backend. This is the one place the two meet, so a replica index
/// that does not exist resolves to the primary rather than to a panic.
///
/// Owned rather than borrowed, since `M72.0`: the primary case may answer with
/// a refreshed backend `primaries` holds rather than with the grant's own, and
/// the two live in different places. `grant.primary` is checked first only in
/// the sense that `primaries` is asked about *it* — the lookup key is always
/// the grant's own primary, because that is the server every session's
/// `PrimaryWatches` entry was started against, whatever it currently maps to.
#[must_use]
pub fn backend_for(
    grant: &Grant,
    target: pgprox_core::route::RouteTarget,
    primaries: &crate::primary_watch::PrimaryWatches,
) -> Backend {
    match target {
        pgprox_core::route::RouteTarget::Replica(index) => grant
            .replicas
            .get(index)
            .cloned()
            .unwrap_or_else(|| primary_or_override(grant, primaries)),
        _ => primary_or_override(grant, primaries),
    }
}

/// The grant's primary, or the backend a topology refresh replaced it with.
fn primary_or_override(grant: &Grant, primaries: &crate::primary_watch::PrimaryWatches) -> Backend {
    primaries
        .current_backend(&grant.primary.server)
        .unwrap_or_else(|| grant.primary.clone())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    //! `M90.6`. ADR 0009 and `plan.md` both described `replica_poll_interval`
    //! as a configured setting with a stated default, and both described a
    //! `max_replica_lag` bounded-staleness opt-in for replica routing.
    //! Neither is true: the interval is `POLL_INTERVAL`, a constant with no
    //! config field behind it, and the opt-in mode does not exist anywhere in
    //! `pgprox-route`. These `include_str!` the two documents so the
    //! overclaim cannot come back silently, the same mechanism `M88.15` used
    //! for `pgprox-config`'s "three providers" claim.

    const ADR_0009: &str = include_str!(
        "../../../docs/internal/product/decisions/0009-replica-routing-with-lsn-watermarks.md"
    );
    const PLAN_MD: &str = include_str!("../../../docs/internal/product/plan.md");

    #[test]
    fn neither_document_calls_the_poll_interval_configured() {
        for (name, doc) in [("ADR 0009", ADR_0009), ("plan.md", PLAN_MD)] {
            assert!(
                !doc.contains("replica_poll_interval` (default"),
                "{name} still describes replica_poll_interval as a configured \
                 setting with a default; it is POLL_INTERVAL, a constant"
            );
        }
    }

    #[test]
    fn adr_0009_records_that_bounded_staleness_routing_is_not_built() {
        assert!(
            ADR_0009.contains("## Outstanding"),
            "ADR 0009 should record that its max_replica_lag opt-in was never \
             implemented"
        );
    }

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
        sets_with_clock().0
    }

    /// A registry and the clock it reads, for the tests about eviction.
    fn sets_with_clock() -> (ReplicaSets, Arc<FakeClock>) {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let clock = Arc::new(FakeClock::new());
        (
            ReplicaSets::new(
                TcpUpstream::new(tls),
                Arc::clone(&clock) as Arc<dyn Clock>,
                Shutdown::new(),
                test_slab(),
            ),
            clock,
        )
    }

    /// A grant naming exactly these replica hosts, in this order.
    fn grant_with(hosts: &[&str]) -> Grant {
        Grant {
            replicas: hosts.iter().map(|host| backend(host)).collect(),
            ..grant(0)
        }
    }

    /// A grant naming a specific primary and replica hosts.
    fn grant_from(primary: &str, replicas: &[&str]) -> Grant {
        Grant {
            primary: backend(primary),
            replicas: replicas.iter().map(|host| backend(host)).collect(),
            ..grant(0)
        }
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
    async fn a_reordered_replica_list_is_a_different_watch() {
        // The correctness case, and the reason the list is in the key at all.
        // `RouteTarget::Replica` is an index: the eligibility check reads slot
        // `i` of the watch and `backend_for` resolves `i` against the session's
        // grant. Sharing one watch across two orderings would clear a read
        // against one host's replay position and then send it to another. The
        // proto says the order means nothing, so this is a shape the sidecar is
        // allowed to produce.
        let sets = sets();
        let forward = sets.watch_for(&grant_with(&["db-r0", "db-r1"])).unwrap();
        let reversed = sets.watch_for(&grant_with(&["db-r1", "db-r0"])).unwrap();

        assert!(
            !Arc::ptr_eq(&forward, &reversed),
            "two orderings of one replica set shared a watch, so an index means two hosts"
        );
        assert_eq!(sets.len(), 2);
    }

    #[tokio::test]
    async fn a_replica_added_later_is_watched_rather_than_ignored() {
        // Until `M69.0` the first grant fixed the set for the life of the
        // process, so this returned the two-slot watch and the third replica
        // was never polled and never routed to.
        let sets = sets();
        let two = sets.watch_for(&grant(2)).unwrap();
        let three = sets.watch_for(&grant(3)).unwrap();

        assert!(!Arc::ptr_eq(&two, &three));
        assert_eq!(two.len(), 2);
        assert_eq!(
            three.len(),
            3,
            "the new replica has no slot to be polled in"
        );
    }

    #[tokio::test]
    async fn primary_of_answers_none_for_a_host_nobody_watches() {
        let sets = sets();
        sets.watch_for(&grant_from("db-1", &["db-r0"]));
        assert_eq!(sets.primary_of(&ServerId::new("db-r9", 5432)), None);
    }

    #[tokio::test]
    async fn primary_of_finds_the_one_primary_a_replica_belongs_to() {
        let sets = sets();
        sets.watch_for(&grant_from("db-1", &["db-r0"]));
        assert_eq!(
            sets.primary_of(&ServerId::new("db-r0", 5432)),
            Some(ServerId::new("db-1", 5432))
        );
    }

    #[tokio::test]
    async fn primary_of_is_not_confused_by_a_replica_seen_in_two_generations_of_the_same_primary() {
        // Two different `WatchKey`s (the replica lists differ), same primary,
        // both naming `db-r0`. Not ambiguous: every sighting agrees, and the
        // guard's job is to notice disagreement, not repetition.
        let sets = sets();
        sets.watch_for(&grant_from("db-1", &["db-r0"]));
        sets.watch_for(&grant_from("db-1", &["db-r0", "db-r1"]));

        assert_eq!(
            sets.primary_of(&ServerId::new("db-r0", 5432)),
            Some(ServerId::new("db-1", 5432)),
            "two generations agreeing on the primary were read as a conflict"
        );
    }

    #[tokio::test]
    async fn primary_of_refuses_to_guess_between_two_disagreeing_primaries() {
        // The actual ambiguity: the same host named as a replica of two
        // different primaries. Nothing here can act on that, so it answers
        // "unknown" rather than picking one.
        let sets = sets();
        sets.watch_for(&grant_from("db-1", &["db-r0"]));
        sets.watch_for(&grant_from("db-2", &["db-r0"]));

        assert_eq!(sets.primary_of(&ServerId::new("db-r0", 5432)), None);
    }

    #[tokio::test]
    async fn a_generation_a_session_still_holds_outlives_the_grace_period() {
        // A session keeps its watch for its whole life and may be idle for
        // hours. Evicting on age alone would take the set out from under it and
        // route its reads to the primary until something rebuilt the watch.
        let (sets, clock) = sets_with_clock();
        let held = sets.watch_for(&grant(2)).unwrap();

        clock.advance(WATCH_GRACE * 2);
        let other = sets.watch_for(&grant(3)).unwrap();

        assert_eq!(sets.len(), 2, "a generation in use was evicted");
        drop(held);
        drop(other);
    }

    #[tokio::test]
    async fn a_generation_nobody_holds_is_dropped_once_it_is_old() {
        // The other half. Keying on the list means a topology that changes
        // every few minutes mints a generation every few minutes, and without
        // this each one keeps a poll loop querying hosts no session can reach.
        let (sets, clock) = sets_with_clock();
        drop(sets.watch_for(&grant(2)).unwrap());
        assert_eq!(sets.len(), 1);

        // Not yet: inside the grace period, a set that is briefly unused is
        // kept so back-to-back sessions do not rebuild it from cold.
        clock.advance(WATCH_GRACE / 2);
        let kept = sets.watch_for(&grant(2)).unwrap();
        assert_eq!(sets.len(), 1);
        assert_eq!(kept.len(), 2);
        drop(kept);

        clock.advance(WATCH_GRACE * 2);
        drop(sets.watch_for(&grant(3)).unwrap());
        assert_eq!(sets.len(), 1, "the stale generation was not dropped");
    }

    #[tokio::test]
    async fn the_grace_period_is_exclusive_at_its_own_boundary() {
        // `evict_unused` guards on `now.duration_since(last_used) < WATCH_GRACE`.
        // The other two tests advance by half and by double, neither of which
        // can tell `<` apart from `<=` or `==`. `FakeClock` makes the boundary
        // itself reachable, unlike a real clock: the offset is arithmetic, so
        // advancing by exactly `WATCH_GRACE` produces a duration equal to it
        // on the nanosecond, not almost equal to it.
        let (sets, clock) = sets_with_clock();
        drop(sets.watch_for(&grant(2)).unwrap());
        assert_eq!(sets.len(), 1);

        clock.advance(WATCH_GRACE);
        drop(sets.watch_for(&grant(3)).unwrap());
        assert_eq!(
            sets.len(),
            1,
            "a generation exactly WATCH_GRACE old was kept rather than dropped"
        );
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

    /// A registry that has no override for anything, so `backend_for` always
    /// falls back to the grant's own primary.
    fn no_overrides() -> crate::primary_watch::PrimaryWatches {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        crate::primary_watch::PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            test_slab(),
            None,
            None,
        )
    }

    #[test]
    fn a_replica_index_the_grant_does_not_have_resolves_to_the_primary() {
        // The two sides of this were built by different milestones against
        // different types. The primary is the answer that cannot be wrong.
        let grant = grant(1);
        let primaries = no_overrides();

        assert_eq!(
            backend_for(&grant, RouteTarget::Replica(0), &primaries).server,
            ServerId::new("db-r0", 5432)
        );
        assert_eq!(
            backend_for(&grant, RouteTarget::Replica(7), &primaries).server,
            ServerId::new("db-1", 5432)
        );
        assert_eq!(
            backend_for(&grant, RouteTarget::Primary, &primaries).server,
            ServerId::new("db-1", 5432)
        );
    }
}
