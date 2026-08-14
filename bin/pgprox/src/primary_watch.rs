//! Detecting a primary's own demotion, fast and locally.
//!
//! # What this is for
//!
//! `features.md` says automatic failover of a primary is decided against: an
//! upstream that goes away is reported to the client rather than silently
//! retried against something else. That stands. What this closes is a
//! narrower gap next to it: the grant cache can serve a client a primary that
//! has *already been* demoted for up to `grant_ttl_cap`, 300 seconds by
//! default, because nothing told it to stop.
//!
//! A demoted primary is a replica that answers `pg_is_in_recovery()` with
//! `true`, which is the same signal [`crate::replicas`] already polls every
//! replica for. This asks the same question of the primary, on the same
//! interval, and on the transition to `true` invalidates every cached grant
//! naming it, through [`pgprox_core::auth::GrantInvalidation`].
//!
//! # What this does not do
//!
//! It does not discover the new primary. Nothing here or in the sidecar
//! contract names one; that is the control plane's to know. Invalidating
//! forces the *next* client presenting an affected token to ask the sidecar
//! again, which is where the new primary comes from.
//!
//! A session already connected to the demoted host is moved, but not
//! instantly and not by force. `M72.0`, ADR 0028: on the same edge that
//! invalidates the cache, this also asks the sidecar's `RefreshTopology` RPC
//! where the primary is now — a second RPC that needs no token, because an
//! established session holds a `Grant` rather than one. A successful answer
//! is stored keyed by the *original* primary, and `crate::replicas::backend_for` checks it
//! before falling back to the grant's own value. A session picks up the
//! correction at its next connection acquire, which is the next transaction
//! boundary for a well-behaved client, without needing a new grant at all.
//!
//! The refresh is best-effort and is never the only thing that happens: if it
//! fails, invalidation still ran, so a new client is no worse off than under
//! `M71.0` alone, and an already-connected session simply keeps failing
//! writes until it reconnects, exactly as it would have before this existed.
//!
//! # Why this stays a boolean rather than reusing `Replicas`
//!
//! `pgprox_route::replica::Replicas` answers "is this replica eligible right
//! now", aged out on a freshness window because a stale reading must not look
//! healthy. Demotion has no such window: once `pg_is_in_recovery()` has
//! answered `true` for a primary, that answer does not need refreshing to stay
//! true, and there is no route decision reading this that a staleness bug
//! could corrupt. A plain flag says everything a probe needs to say.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use pgprox_core::auth::{Backend, GrantInvalidation, TopologyRefresh};
use pgprox_core::buf::BufferSlab;
use pgprox_core::ids::ServerId;
use pgprox_route::poller::{Probe, ReplicaProbe};
use pgprox_session::probe::SqlReplicaProbe;

use crate::dial::TcpUpstream;
use crate::run::Shutdown;

/// The same cadence [`crate::replicas`] polls at. Demotion is not chasing a
/// freshness window the way replica eligibility is, but a slower probe would
/// mean a slower `M69`-style budget on how fast this can possibly notice.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// One primary's last known status.
struct Watched {
    /// Set once, on the transition into recovery, and never cleared.
    ///
    /// A primary that comes back is a new grant with a new primary as far as
    /// this process is concerned; nothing here un-demotes an entry, because
    /// nothing here is qualified to decide a demoted host is trustworthy
    /// again.
    demoted: Arc<AtomicBool>,
}

/// Every primary this node has been told about, and whether it has demoted.
pub struct PrimaryWatches {
    watched: Mutex<HashMap<ServerId, Watched>>,
    /// Accepted refreshes, keyed by the *original* primary a grant still
    /// names. `crate::replicas::backend_for` is the one reader.
    overrides: Arc<Mutex<HashMap<ServerId, Backend>>>,
    upstream: TcpUpstream,
    shutdown: Shutdown,
    slab: Arc<BufferSlab>,
    invalidation: Option<Arc<dyn GrantInvalidation>>,
    topology: Option<Arc<dyn TopologyRefresh>>,
}

impl std::fmt::Debug for PrimaryWatches {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrimaryWatches")
            .field("watched", &self.lock().len())
            .finish_non_exhaustive()
    }
}

impl PrimaryWatches {
    /// A registry that has been told about nothing yet.
    ///
    /// `invalidation` and `topology` are each `None` for a node built with a
    /// resolver that is not a caching one, which today is only a test
    /// fixture: `entry.rs` always wraps the real sidecar client in
    /// `CachingResolver`, which is also the `SidecarResolver` implementing
    /// `TopologyRefresh`. A `None` here means probing still runs; only the
    /// action taken on what it finds is skipped.
    #[must_use]
    pub fn new(
        upstream: TcpUpstream,
        shutdown: Shutdown,
        slab: Arc<BufferSlab>,
        invalidation: Option<Arc<dyn GrantInvalidation>>,
        topology: Option<Arc<dyn TopologyRefresh>>,
    ) -> Self {
        Self {
            watched: Mutex::new(HashMap::new()),
            overrides: Arc::new(Mutex::new(HashMap::new())),
            upstream,
            shutdown,
            slab,
            invalidation,
            topology,
        }
    }

    /// The backend to use for `original` now, if a refresh has replaced it.
    ///
    /// `None` means nothing has overridden it, which is what every primary
    /// answers until its first demotion; `crate::replicas::backend_for` falls back to the
    /// grant's own value in that case.
    #[must_use]
    pub fn current_backend(&self, original: &ServerId) -> Option<Backend> {
        self.overrides
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(original)
            .cloned()
    }

    /// How many primaries are being watched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Whether `server` has been seen in recovery since it was first watched.
    ///
    /// For tests and for `SHOW`-style introspection later; nothing on the
    /// route decision path reads this.
    #[must_use]
    pub fn is_demoted(&self, server: &ServerId) -> bool {
        self.lock()
            .get(server)
            .is_some_and(|watched| watched.demoted.load(Ordering::Acquire))
    }

    /// Starts watching `primary`, unless it already is.
    ///
    /// Idempotent and cheap to call on every session: the common case is a
    /// primary this node already watches, which costs one lookup under the
    /// lock.
    pub fn ensure_watched(&self, primary: &Backend) {
        let key = primary.server.clone();
        let mut watched = self.lock();
        if watched.contains_key(&key) {
            return;
        }

        let demoted = Arc::new(AtomicBool::new(false));
        watched.insert(
            key,
            Watched {
                demoted: Arc::clone(&demoted),
            },
        );
        drop(watched);

        tokio::spawn(poll(
            primary.server.clone(),
            demoted,
            Arc::new(SqlReplicaProbe::new(
                self.upstream.clone(),
                vec![primary.clone()],
                Arc::clone(&self.slab),
            )),
            self.invalidation.clone(),
            self.topology.clone(),
            Arc::clone(&self.overrides),
            self.shutdown.clone(),
        ));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ServerId, Watched>> {
        self.watched.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Polls one primary until the node stops.
///
/// No eviction, unlike [`crate::replicas::ReplicaSets`]. That registry keys on
/// the replica list and mints a generation on every topology change, which
/// makes eviction load-bearing rather than tidiness. A primary is one
/// `ServerId`, and the number of distinct primaries a node's tenants use is
/// the number of upstream databases in the fleet: bounded by the operator, not
/// by session count or by how often a replica set is reshuffled. If that stops
/// being true for some deployment, the fix is here rather than in every
/// session paying for eviction machinery today's doesn't need.
async fn poll(
    server: ServerId,
    demoted: Arc<AtomicBool>,
    probe: Arc<SqlReplicaProbe<TcpUpstream>>,
    invalidation: Option<Arc<dyn GrantInvalidation>>,
    topology: Option<Arc<dyn TopologyRefresh>>,
    overrides: Arc<Mutex<HashMap<ServerId, Backend>>>,
    shutdown: Shutdown,
) {
    let mut ticks = tokio::time::interval(POLL_INTERVAL);
    loop {
        tokio::select! {
            () = shutdown.waited() => return,
            _ = ticks.tick() => {}
        }

        // Index 0: the probe was built with exactly one backend, this
        // primary, so there is no second index to confuse it with.
        let result = probe.probe(0).await;
        let just_demoted = handle(&server, &result, &demoted, invalidation.as_deref());

        // Only the poll that performed the transition asks for a refresh.
        // Every later poll while still demoted finds `just_demoted` false and
        // does nothing here, which is `handle`'s edge-trigger reaching this
        // step too: one refresh attempt per demotion, not one per poll.
        if just_demoted {
            refresh(&server, topology.as_deref(), &overrides).await;
        }
    }
}

/// Acts on one poll's result. Split from [`poll`] so the decision is testable
/// without a socket, a clock tick, or a spawned task.
///
/// Returns whether *this call* performed the transition into demoted, which is
/// distinct from `demoted`'s final value: a later call on an already-demoted
/// primary returns `false` even though the primary is still demoted, because
/// nothing changed on this call.
fn handle(
    server: &ServerId,
    result: &Result<Probe, String>,
    demoted: &AtomicBool,
    invalidation: Option<&dyn GrantInvalidation>,
) -> bool {
    let Ok(Probe { in_recovery, .. }) = *result else {
        // A failed probe is inconclusive, not a demotion. The most common
        // cause is the host being briefly unreachable, and invalidating a
        // primary's grants on every network blip would turn a poll interval
        // into a resolve storm on the sidecar for a server that never
        // actually changed. `crate::replicas` ages a reading out instead,
        // which has no equivalent here because nothing routes on this value;
        // a probe that starts succeeding again finds the same `demoted` flag
        // it left, still false.
        return false;
    };

    if !in_recovery {
        return false;
    }

    // Edge-triggered: the first probe to see recovery fires the invalidation,
    // and `swap` makes "was it already true" and "mark it true" one atomic
    // step, so two polls racing on a slow tick cannot both fire.
    if demoted.swap(true, Ordering::AcqRel) {
        return false;
    }

    let dropped = invalidation.map_or(0, |handle| handle.invalidate_primary(server));
    tracing::warn!(
        %server,
        dropped_grants = dropped,
        "primary reports pg_is_in_recovery(): demoted, cached grants naming it dropped"
    );
    true
}

/// Asks where `server`'s topology stands now, and stores a usable answer.
///
/// Best-effort. `topology` is `None` on a node whose resolver is not a caching
/// one, which today is only a test fixture, and a real failure just means the
/// override table stays as it was: a session that could not be helped is
/// exactly as unhelped as it would have been without this. See ADR 0028.
async fn refresh(
    server: &ServerId,
    topology: Option<&dyn TopologyRefresh>,
    overrides: &Mutex<HashMap<ServerId, Backend>>,
) {
    let Some(topology) = topology else { return };

    match topology.refresh_topology(server).await {
        Ok(answer) => {
            let moved = answer.primary.server != *server;
            overrides
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(server.clone(), answer.primary.clone());
            tracing::info!(
                %server,
                new_primary = %answer.primary.server,
                replicas = answer.replicas.len(),
                moved,
                "topology refreshed: sessions still connected pick up the correction \
                 at their next connection acquire"
            );
        }
        Err(reason) => {
            tracing::warn!(
                %server,
                %reason,
                "topology refresh failed: a session already connected keeps failing \
                 until it reconnects, same as before this existed"
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use pgprox_core::auth::{FakeInvalidation, TlsMode};
    use pgprox_core::ids::Lsn;

    fn backend(host: &str) -> Backend {
        Backend {
            server: ServerId::new(host, 5432),
            database: "acme".into(),
            user: "acme_app".into(),
            password: pgprox_core::secret::SecretString::new("hunter2"),
            tls: TlsMode::Disabled,
        }
    }

    /// A backend naming a real, already-listening address.
    fn backend_at(addr: std::net::SocketAddr) -> Backend {
        Backend {
            server: ServerId::new("127.0.0.1", addr.port()),
            ..backend("127.0.0.1")
        }
    }

    /// What one `.await` writes to the log.
    ///
    /// Same idea as `run.rs`'s helper of the same name, adapted for an async
    /// callee: `set_default`'s guard lives across the `.await` rather than
    /// wrapping a synchronous call, which is sound because these tests run
    /// on `#[tokio::test]`'s single-threaded flavour and never hop threads
    /// mid-future. `refresh`'s `moved` field is computed and logged but
    /// never stored, so this is the only way a test can observe it.
    async fn logged(f: impl std::future::Future<Output = ()>) -> String {
        use std::sync::Mutex as StdMutex;

        #[derive(Clone)]
        struct Sink(Arc<StdMutex<Vec<u8>>>);

        impl std::io::Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
            type Writer = Self;
            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        let buffer = Arc::new(StdMutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(Sink(Arc::clone(&buffer)))
            .with_ansi(false)
            .finish();

        let guard = tracing::subscriber::set_default(subscriber);
        f.await;
        drop(guard);

        let held = buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&held).into_owned()
    }

    // Both fixtures return `Ok`, unlike a real probe: `handle` takes
    // `Result<Probe, String>` and the failure case is written inline at its
    // one call site instead, so keeping these `Result`-shaped is what lets a
    // reader compare the three fixtures at a glance.
    #[allow(clippy::unnecessary_wraps)]
    fn healthy() -> Result<Probe, String> {
        Ok(Probe {
            replayed: Lsn::new(0),
            in_recovery: false,
        })
    }

    #[allow(clippy::unnecessary_wraps)]
    fn demoted_probe() -> Result<Probe, String> {
        Ok(Probe {
            replayed: Lsn::new(0),
            in_recovery: true,
        })
    }

    #[test]
    fn a_healthy_primary_invalidates_nothing() {
        let flag = AtomicBool::new(false);
        let invalidation = FakeInvalidation::new();
        handle(
            &ServerId::new("db-1", 5432),
            &healthy(),
            &flag,
            Some(&invalidation),
        );
        assert!(!flag.load(Ordering::Acquire));
        assert!(invalidation.calls().is_empty());
    }

    #[test]
    fn a_failed_probe_is_inconclusive_rather_than_a_demotion() {
        let flag = AtomicBool::new(false);
        let invalidation = FakeInvalidation::new();
        handle(
            &ServerId::new("db-1", 5432),
            &Err("connection refused".to_owned()),
            &flag,
            Some(&invalidation),
        );
        assert!(
            !flag.load(Ordering::Acquire),
            "a network failure was read as a demotion"
        );
        assert!(invalidation.calls().is_empty());
    }

    #[test]
    fn a_demoted_primary_flips_the_flag_and_invalidates_it() {
        let flag = AtomicBool::new(false);
        let invalidation = FakeInvalidation::new();
        let server = ServerId::new("db-1", 5432);

        handle(&server, &demoted_probe(), &flag, Some(&invalidation));

        assert!(flag.load(Ordering::Acquire));
        assert_eq!(invalidation.calls(), vec![server]);
    }

    #[test]
    fn a_second_demoted_reading_does_not_invalidate_again() {
        // The edge-trigger. A demoted primary keeps answering `true` on every
        // subsequent poll, and invalidating on each one would mean a demoted
        // primary that stays demoted for an hour asks the sidecar to
        // re-invalidate a cache that is already empty of it 14,400 times.
        let flag = AtomicBool::new(false);
        let invalidation = FakeInvalidation::new();
        let server = ServerId::new("db-1", 5432);

        handle(&server, &demoted_probe(), &flag, Some(&invalidation));
        handle(&server, &demoted_probe(), &flag, Some(&invalidation));
        handle(&server, &demoted_probe(), &flag, Some(&invalidation));

        assert_eq!(
            invalidation.calls().len(),
            1,
            "a primary that stayed demoted was invalidated more than once"
        );
    }

    #[test]
    fn only_the_transition_reports_true() {
        // `poll` asks for a refresh only on a `true` return, so this is the
        // property that keeps a demoted-for-an-hour primary from being asked
        // about 14,400 times: the same reason the invalidation count above is
        // one rather than three, seen at the boundary that decides it.
        let flag = AtomicBool::new(false);
        let server = ServerId::new("db-1", 5432);

        assert!(
            handle(&server, &demoted_probe(), &flag, None),
            "the first demoted reading did not report a transition"
        );
        assert!(
            !handle(&server, &demoted_probe(), &flag, None),
            "a repeat reading reported a transition that did not happen"
        );
        assert!(
            !handle(&server, &healthy(), &flag, None),
            "a healthy reading reported a transition"
        );
    }

    #[tokio::test]
    async fn a_successful_refresh_stores_the_new_primary() {
        let server = ServerId::new("db-1", 5432);
        let fresh = pgprox_core::auth::FakeTopologyRefresh::new().with_topology(
            server.clone(),
            pgprox_core::auth::Topology {
                primary: backend("db-2"),
                replicas: vec![backend("db-2-replica")],
            },
        );
        let overrides = Mutex::new(HashMap::new());

        let noisy = logged(refresh(&server, Some(&fresh), &overrides)).await;

        let stored = overrides
            .lock()
            .unwrap()
            .get(&server)
            .cloned()
            .expect("the successful refresh stored nothing");
        assert_eq!(stored.server, ServerId::new("db-2", 5432));
        assert!(
            noisy.contains("moved=true"),
            "a refresh that changed the primary did not say so: {noisy}"
        );
    }

    #[tokio::test]
    async fn a_refresh_that_confirms_the_same_primary_says_so() {
        // The other side of `moved`: the sidecar can answer a `RefreshTopology`
        // call with the primary a session already has, and that is not a
        // move. Both branches write to the same `overrides` entry either way,
        // so `moved` is the one place this distinction is visible at all.
        let server = ServerId::new("db-1", 5432);
        let fresh = pgprox_core::auth::FakeTopologyRefresh::new().with_topology(
            server.clone(),
            pgprox_core::auth::Topology {
                primary: backend("db-1"),
                replicas: vec![backend("db-1-replica")],
            },
        );
        let overrides = Mutex::new(HashMap::new());

        let noisy = logged(refresh(&server, Some(&fresh), &overrides)).await;

        assert!(
            noisy.contains("moved=false"),
            "a refresh confirming the same primary reported a move: {noisy}"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_stores_nothing() {
        // Best-effort: a session already connected gets no relief in this
        // case, and that is the same outcome as before this existed, not a
        // worse one. Nothing here should make that look like progress.
        let server = ServerId::new("db-1", 5432);
        let fresh = pgprox_core::auth::FakeTopologyRefresh::new();
        fresh.set_unavailable(true);
        let overrides = Mutex::new(HashMap::new());

        refresh(&server, Some(&fresh), &overrides).await;

        assert!(overrides.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_topology_handle_does_nothing_and_does_not_panic() {
        let overrides = Mutex::new(HashMap::new());
        refresh(&ServerId::new("db-1", 5432), None, &overrides).await;
        assert!(overrides.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn current_backend_is_none_until_something_overrides_it() {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            None,
            None,
        );
        let server = ServerId::new("db-1", 5432);

        assert!(watches.current_backend(&server).is_none());
    }

    #[test]
    fn no_invalidation_handle_still_flips_the_flag() {
        // A node whose resolver is not a caching one, which today is only a
        // test fixture, still gets the visible signal even though nothing
        // acts on it.
        let flag = AtomicBool::new(false);
        handle(&ServerId::new("db-1", 5432), &demoted_probe(), &flag, None);
        assert!(flag.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn ensure_watched_is_idempotent() {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            None,
            None,
        );
        let primary = backend("db-1");

        watches.ensure_watched(&primary);
        watches.ensure_watched(&primary);
        watches.ensure_watched(&primary);

        assert_eq!(
            watches.len(),
            1,
            "watching the same primary three times started three loops"
        );
        assert!(!watches.is_demoted(&primary.server));
    }

    #[tokio::test]
    async fn a_demoted_primary_is_detected_and_invalidated_within_two_seconds() {
        // The end-to-end claim, proved against a real socket and a real clock
        // rather than the unit-level `handle` above: connect a primary,
        // resolve a grant naming it into a real `CachingResolver`, and prove
        // the entry is gone well inside the two-second budget this module
        // exists to meet.
        //
        // `fakepg::fake_postgres` answers `pg_is_in_recovery()` with `t`
        // unconditionally, which is documented as modelling a replica. Used
        // as a primary here, that is exactly the demoted-primary case, and it
        // demotes from the very first poll rather than partway through, which
        // is what makes the two-second bound the whole of what this measures.
        let primary_addr = crate::fakepg::fake_postgres().await;
        let primary = backend_at(primary_addr);

        let grant = pgprox_core::auth::Grant {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            primary: primary.clone(),
            replicas: Vec::new(),
            pool: pgprox_core::auth::PoolHints::default(),
            ttl: Duration::from_secs(300),
            claims: pgprox_core::auth::ClaimSet::default(),
        };
        // A well-formed header, so the algorithm allowlist lets it through to
        // the fake resolver rather than refusing it before the cache is ever
        // touched.
        let token = format!(
            "{}.{}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"alg":"RS256","typ":"JWT"}"#),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(r#"{"sub":"acme"}"#),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("signature"),
        );
        let inner = Arc::new(
            pgprox_core::auth::FakeCredentialResolver::new().with_grant(&token, grant.clone()),
        );
        let caching = pgprox_auth::cache::CachingResolver::new(
            inner,
            Arc::new(pgprox_core::clock::SystemClock) as Arc<dyn pgprox_core::clock::Clock>,
            pgprox_auth::cache::CacheConfig::default(),
        );
        pgprox_core::auth::CredentialResolver::resolve(
            caching.as_ref(),
            pgprox_core::auth::AuthRequest {
                token: pgprox_core::secret::SecretString::new(&token),
                startup_database: "acme".into(),
                startup_user: "acme_app".into(),
                client_addr: "10.0.0.1".parse().unwrap(),
            },
        )
        .await
        .unwrap();
        assert_eq!(caching.len(), 1, "the grant did not populate the cache");

        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            Some(Arc::clone(&caching) as Arc<dyn GrantInvalidation>),
            None,
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        watches.ensure_watched(&primary);

        while tokio::time::Instant::now() < deadline {
            if caching.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert_eq!(
            caching.len(),
            0,
            "the demoted primary's grant was not invalidated within two seconds"
        );
        assert!(watches.is_demoted(&primary.server));
    }

    #[tokio::test]
    async fn an_established_sessions_next_acquire_finds_the_corrected_primary() {
        // The claim ADR 0028 makes for a session that already authenticated:
        // it holds a `Grant` naming the old primary for its whole life, and
        // this proves the one place that matters — `backend_for`, which is
        // what every connection acquire calls — resolves the corrected
        // backend from that same, unmodified `Grant`, with no new grant and
        // no reconnect involved.
        let primary_addr = crate::fakepg::fake_postgres().await;
        let primary = backend_at(primary_addr);
        let new_primary = backend("db-2-after-failover");

        let grant = pgprox_core::auth::Grant {
            tenant: pgprox_core::ids::TenantId::new("acme"),
            primary: primary.clone(),
            replicas: Vec::new(),
            pool: pgprox_core::auth::PoolHints::default(),
            ttl: Duration::from_secs(300),
            claims: pgprox_core::auth::ClaimSet::default(),
        };

        let topology = pgprox_core::auth::FakeTopologyRefresh::new().with_topology(
            primary.server.clone(),
            pgprox_core::auth::Topology {
                primary: new_primary.clone(),
                replicas: vec![],
            },
        );

        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            None,
            Some(Arc::new(topology) as Arc<dyn TopologyRefresh>),
        );

        // Before the demotion is even detected, `backend_for` resolves the
        // grant's own primary: nothing has overridden it yet.
        assert_eq!(
            crate::replicas::backend_for(
                &grant,
                pgprox_core::route::RouteTarget::Primary,
                &watches
            )
            .server,
            primary.server
        );

        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        watches.ensure_watched(&primary);
        while tokio::time::Instant::now() < deadline {
            if watches.current_backend(&primary.server).is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let resolved = crate::replicas::backend_for(
            &grant,
            pgprox_core::route::RouteTarget::Primary,
            &watches,
        );
        assert_eq!(
            resolved.server, new_primary.server,
            "the session's next acquire still resolved the demoted primary"
        );
    }

    #[tokio::test]
    async fn two_primaries_are_watched_independently() {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            None,
            None,
        );

        watches.ensure_watched(&backend("db-1"));
        watches.ensure_watched(&backend("db-2"));

        assert_eq!(watches.len(), 2);
        assert!(!watches.is_empty());
    }

    #[tokio::test]
    async fn a_freshly_built_registry_is_empty_and_says_so_in_its_debug_form() {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            None,
            None,
        );

        assert_eq!(watches.len(), 0);
        assert!(watches.is_empty());
        assert!(
            format!("{watches:?}").contains("watched: 0"),
            "Debug did not report the count it holds"
        );

        watches.ensure_watched(&backend("db-1"));

        assert!(!watches.is_empty());
        assert!(
            format!("{watches:?}").contains("watched: 1"),
            "Debug did not track a watch added after construction"
        );
    }
}
