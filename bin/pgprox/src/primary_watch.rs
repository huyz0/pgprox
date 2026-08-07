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
//! It does not move a session already connected to the demoted host. That
//! session's writes start failing with Postgres's own "cannot execute ... in
//! a read-only transaction", which is a transient error a retry policy can
//! act on; this module only shortens how long new connections keep walking
//! into the same wall.
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

use pgprox_core::auth::{Backend, GrantInvalidation};
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
    upstream: TcpUpstream,
    shutdown: Shutdown,
    slab: Arc<BufferSlab>,
    invalidation: Option<Arc<dyn GrantInvalidation>>,
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
    /// `invalidation` is `None` for a node built with a resolver that is not a
    /// caching one, which today is only a test fixture: `entry.rs` always
    /// wraps the real sidecar client in `CachingResolver`. A `None` here means
    /// probing still runs, and its finding is only ever logged rather than
    /// acted on.
    #[must_use]
    pub fn new(
        upstream: TcpUpstream,
        shutdown: Shutdown,
        slab: Arc<BufferSlab>,
        invalidation: Option<Arc<dyn GrantInvalidation>>,
    ) -> Self {
        Self {
            watched: Mutex::new(HashMap::new()),
            upstream,
            shutdown,
            slab,
            invalidation,
        }
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
        handle(&server, &result, &demoted, invalidation.as_deref());
    }
}

/// Acts on one poll's result. Split from [`poll`] so the decision is testable
/// without a socket, a clock tick, or a spawned task.
fn handle(
    server: &ServerId,
    result: &Result<Probe, String>,
    demoted: &AtomicBool,
    invalidation: Option<&dyn GrantInvalidation>,
) {
    let Ok(Probe { in_recovery, .. }) = *result else {
        // A failed probe is inconclusive, not a demotion. The most common
        // cause is the host being briefly unreachable, and invalidating a
        // primary's grants on every network blip would turn a poll interval
        // into a resolve storm on the sidecar for a server that never
        // actually changed. `crate::replicas` ages a reading out instead,
        // which has no equivalent here because nothing routes on this value;
        // a probe that starts succeeding again finds the same `demoted` flag
        // it left, still false.
        return;
    };

    if !in_recovery {
        return;
    }

    // Edge-triggered: the first probe to see recovery fires the invalidation,
    // and `swap` makes "was it already true" and "mark it true" one atomic
    // step, so two polls racing on a slow tick cannot both fire.
    if demoted.swap(true, Ordering::AcqRel) {
        return;
    }

    let dropped = invalidation.map_or(0, |handle| handle.invalidate_primary(server));
    tracing::warn!(
        %server,
        dropped_grants = dropped,
        "primary reports pg_is_in_recovery(): demoted, cached grants naming it dropped"
    );
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
    async fn two_primaries_are_watched_independently() {
        let tls = pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap();
        let watches = PrimaryWatches::new(
            TcpUpstream::new(tls),
            Shutdown::new(),
            BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8),
            None,
        );

        watches.ensure_watched(&backend("db-1"));
        watches.ensure_watched(&backend("db-2"));

        assert_eq!(watches.len(), 2);
        assert!(!watches.is_empty());
    }
}
