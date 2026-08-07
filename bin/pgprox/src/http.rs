//! The node's HTTP surface: the two probes and the admin API.
//!
//! # Why the probes live here and not in `pgprox-observe`
//!
//! That crate owns the *answers*: `Health` takes what a probe is allowed to
//! consider and returns a [`Probe`]. What it cannot own is where the answer
//! comes from, because readiness is a function of the drain overlay and the
//! configuration document, and a sans-I/O crate holds neither. Joining them is
//! composition, which is this binary's job.
//!
//! # One drain, read two ways
//!
//! `/readyz` and `POST /v1/drain` are the same fact seen from two sides. They
//! read one [`SharedDrain`], so a drain requested through the API is one the
//! probe reports on its next scrape. Until `M6.26` they were separate
//! `DrainState`s and the probe kept passing, which is the failure mode a drain
//! sequence cannot survive: Kubernetes keeps sending clients to a node that
//! believes it is leaving.
//!
//! # Liveness cannot fail from load
//!
//! Nothing here passes a load signal to `Health::liveness`, and its signature
//! has no parameter for one. See `pgprox_observe::health`.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use pgprox_admin::Shared;
use pgprox_core::clock::Clock;
use pgprox_core::config::ConfigSource;
use pgprox_observe::health::Probe;

use crate::wiring::{SharedDrain, SharedHealth, lock};

/// Everything the two probes read.
pub struct Probes {
    health: SharedHealth,
    drain: SharedDrain,
    config: Arc<dyn ConfigSource>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for Probes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Probes").finish_non_exhaustive()
    }
}

impl Probes {
    /// The probes for one node.
    #[must_use]
    pub const fn new(
        health: SharedHealth,
        drain: SharedDrain,
        config: Arc<dyn ConfigSource>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            health,
            drain,
            config,
            clock,
        }
    }

    /// Whether this node is draining, from either the document or the API.
    #[must_use]
    pub fn is_draining(&self) -> bool {
        let config = self.config.watch().borrow().clone();
        lock(&self.drain).is_draining(&config, self.clock.now())
    }

    /// What `/readyz` answers.
    #[must_use]
    pub fn readiness(&self) -> Probe {
        lock(&self.health).readiness(self.is_draining())
    }

    /// What `/healthz` answers.
    #[must_use]
    pub fn liveness(&self) -> Probe {
        lock(&self.health).liveness(self.clock.now())
    }

    /// Records that the run loop is still running.
    pub fn beat(&self) {
        lock(&self.health).beat(self.clock.now());
    }
}

/// Renders a probe as a status and a one-word body.
///
/// The word is the reason, so an operator reading a probe failure in an event
/// log knows which of the reasons it was without a second request.
fn render(probe: Probe) -> impl IntoResponse {
    let status =
        axum::http::StatusCode::from_u16(probe.status()).unwrap_or(axum::http::StatusCode::OK);
    let body = probe
        .reason()
        .map_or("ok", pgprox_observe::health::Reason::as_str);
    (status, body)
}

/// `GET /readyz`
async fn readyz(State(probes): State<Arc<Probes>>) -> impl IntoResponse {
    render(probes.readiness())
}

/// `GET /healthz`
async fn healthz(State(probes): State<Arc<Probes>>) -> impl IntoResponse {
    render(probes.liveness())
}

/// `GET /metrics`
///
/// Text, not JSON: this is the one endpoint whose format is somebody else's.
async fn metrics(State(state): State<MetricsState>) -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        crate::metrics::render(
            state.observatory.as_ref(),
            state.node,
            state.tenants.as_ref(),
            state.slab.as_ref(),
            state.routes.as_ref(),
        )
        .await,
    )
}

/// What the exporter needs: somewhere to read from, and whose numbers they are.
#[derive(Clone)]
struct MetricsState {
    observatory: Shared,
    node: pgprox_core::ids::NodeId,
    /// Which tenants get their own series. See `pgprox_observe::tenants`.
    tenants: Arc<pgprox_observe::tenants::TenantAllowlist>,
    /// The node's buffer slab, whose occupancy is a metric of its own.
    slab: Arc<pgprox_core::buf::BufferSlab>,
    /// Where statements went.
    routes: Arc<crate::routes::RouteCounts>,
}

/// The probe routes.
pub fn probe_routes(probes: Arc<Probes>) -> Router {
    Router::new()
        .route("/healthz", axum::routing::get(healthz))
        .route("/readyz", axum::routing::get(readyz))
        .with_state(probes)
}

/// Everything this node serves over HTTP.
///
/// The admin routes come from `pgprox-admin` rather than being restated here,
/// so a route added there is served here without anyone remembering to.
pub fn router(
    observatory: Shared,
    probes: Arc<Probes>,
    node: pgprox_core::ids::NodeId,
    tenants: Arc<pgprox_observe::tenants::TenantAllowlist>,
    slab: Arc<pgprox_core::buf::BufferSlab>,
    routes: Arc<crate::routes::RouteCounts>,
) -> Router {
    let exporter = Router::new()
        .route("/metrics", axum::routing::get(metrics))
        .with_state(MetricsState {
            observatory: Arc::clone(&observatory),
            node,
            tenants,
            slab,
            routes,
        });

    probe_routes(probes)
        .merge(exporter)
        .merge(pgprox_admin::routes().with_state(observatory))
}

/// Serves until the shutdown future resolves.
///
/// # Errors
///
/// Fails when the listening socket does.
pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
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
    use pgprox_config::drain::{DrainConfig, DrainState};
    use pgprox_core::clock::FakeClock;
    use pgprox_core::cluster::NodeMode;
    use pgprox_core::config::{Config, FakeConfigSource, NodeOverride};
    use pgprox_observe::health::{Health, HealthConfig, Reason};
    use std::sync::Mutex;

    fn probes_over(config: Config) -> (Arc<Probes>, SharedDrain, Arc<dyn ConfigSource>) {
        let drain: SharedDrain = Arc::new(Mutex::new(DrainState::new(
            "pgprox-1",
            DrainConfig::default(),
        )));
        let health: SharedHealth = Arc::new(Mutex::new(Health::new(HealthConfig::default())));
        lock(&health).started();
        let source: Arc<dyn ConfigSource> = FakeConfigSource::new(config).unwrap();
        let probes = Arc::new(Probes::new(
            health,
            Arc::clone(&drain),
            Arc::clone(&source),
            Arc::new(FakeClock::new()),
        ));
        (probes, drain, source)
    }

    /// One request against a router, without binding a port.
    async fn request(router: &Router, method: &str, path: &str) -> (u16, String) {
        use tower::ServiceExt;

        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    async fn get(router: &Router, path: &str) -> (u16, String) {
        request(router, "GET", path).await
    }

    async fn post(router: &Router, path: &str) -> (u16, String) {
        request(router, "POST", path).await
    }

    #[tokio::test]
    async fn a_started_node_is_ready_and_alive() {
        let (probes, _drain, _source) = probes_over(Config::default());
        let router = probe_routes(probes);

        assert_eq!(get(&router, "/readyz").await, (200, "ok".to_owned()));
        assert_eq!(get(&router, "/healthz").await, (200, "ok".to_owned()));
    }

    #[tokio::test]
    async fn a_node_that_has_not_loaded_a_configuration_is_not_ready() {
        // Which is what stops a rolling deploy sending clients to a pod that
        // cannot serve them yet.
        let health: SharedHealth = Arc::new(Mutex::new(Health::new(HealthConfig::default())));
        let probes = Arc::new(Probes::new(
            health,
            Arc::new(Mutex::new(DrainState::new(
                "pgprox-1",
                DrainConfig::default(),
            ))),
            FakeConfigSource::new(Config::default()).unwrap(),
            Arc::new(FakeClock::new()),
        ));

        let (status, body) = get(&probe_routes(probes), "/readyz").await;
        assert_eq!(status, 503);
        assert_eq!(body, Reason::Starting.as_str());
    }

    #[tokio::test]
    async fn a_drain_in_the_document_fails_readiness() {
        let mut draining = Config::default();
        draining.nodes.insert(
            "pgprox-1".to_owned(),
            NodeOverride {
                mode: NodeMode::Draining,
            },
        );
        let (probes, _drain, _source) = probes_over(draining);

        let (status, body) = get(&probe_routes(probes), "/readyz").await;
        assert_eq!(status, 503);
        assert_eq!(body, Reason::Draining.as_str());
    }

    /// A live observatory over the drain the probes read.
    fn observatory_over(
        source: Arc<dyn ConfigSource>,
        drain: SharedDrain,
    ) -> crate::observatory::NodeObservatory {
        let clock: Arc<dyn Clock> = Arc::new(FakeClock::new());
        let connector = Arc::new(pgprox_session::connect::PgConnector::new(
            crate::dial::TcpUpstream::new(
                pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).unwrap(),
            ),
            test_slab(),
        ));
        crate::observatory::NodeObservatory::new(crate::observatory::NodeParts {
            node: pgprox_core::ids::NodeId::new(1),
            clock: Arc::clone(&clock),
            config: source,
            cluster: pgprox_cluster::service::GossipCoordinator::new(
                pgprox_core::ids::NodeId::new(1),
                pgprox_cluster::coordinator::CoordinatorConfig::default(),
                Arc::clone(&clock),
            ),
            pool: pgprox_pool::live::LivePool::new(
                connector,
                Arc::clone(&clock),
                Arc::new(crate::entropy::SystemJitter),
                pgprox_pool::pool::PoolConfig::default(),
            ),
            sessions: crate::sessions::Sessions::new(),
            drain,
            cache: pgprox_cache::Store::new(Arc::clone(&clock)),
            recordings: Arc::new(crate::recording::Recordings::new()),
        })
    }

    #[tokio::test]
    async fn the_admin_routes_are_served_alongside_the_probes() {
        // One router, so a deployment exposes one port and an operator does not
        // have to know which surface answers which path.
        let (probes, drain, source) = probes_over(Config::default());
        let observatory: Shared = Arc::new(observatory_over(source, drain));

        let router = router(
            observatory,
            probes,
            pgprox_core::ids::NodeId::new(1),
            Arc::new(pgprox_observe::tenants::TenantAllowlist::new()),
            test_slab(),
            Arc::new(crate::routes::RouteCounts::new()),
        );
        assert_eq!(get(&router, "/healthz").await.0, 200);
        assert_eq!(get(&router, "/v1/cluster").await.0, 200);
    }

    #[tokio::test]
    async fn the_surface_answers_over_a_real_socket_and_stops_when_told() {
        // `oneshot` proves the routing; this proves the serving. A probe that
        // only ever answered in a test harness is a probe kubelet cannot reach.
        let (probes, _drain, _source) = probes_over(Config::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();

        let served = tokio::spawn(serve(listener, probe_routes(probes), async {
            let _ = stopped.await;
        }));

        let response = raw_get(addr, "/readyz").await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("ok"), "{response}");

        stop.send(()).unwrap();
        served.await.unwrap().unwrap();
    }

    /// One HTTP/1.1 request, hand-written, so the test depends on no client.
    async fn raw_get(addr: std::net::SocketAddr, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: probe\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let mut answer = String::new();
        socket.read_to_string(&mut answer).await.unwrap();
        answer
    }

    #[tokio::test]
    async fn the_exporter_is_served_beside_the_probes() {
        // One port for everything an operator or a collector asks a node, so a
        // deployment exposes one thing rather than three.
        let (probes, drain, source) = probes_over(Config::default());
        let observatory: Shared = Arc::new(observatory_over(source, drain));
        let router = router(
            observatory,
            probes,
            pgprox_core::ids::NodeId::new(1),
            Arc::new(pgprox_observe::tenants::TenantAllowlist::new()),
            test_slab(),
            Arc::new(crate::routes::RouteCounts::new()),
        );

        let (status, body) = get(&router, "/metrics").await;
        assert_eq!(status, 200);
        assert!(body.contains("# HELP pgprox_client_conns "), "{body}");
        assert!(body.contains("node=\"1\""), "{body}");
    }

    #[tokio::test]
    async fn a_drain_posted_to_the_api_fails_the_probe() {
        // The seam M6.26 exists to close: two DrainStates meant the API said
        // draining, the probe said ready, and Kubernetes kept sending clients
        // to a node that believed it was leaving. Driven over HTTP rather than
        // through the trait, because the wiring is what was wrong.
        let (probes, drain, source) = probes_over(Config::default());
        let observatory: Shared = Arc::new(observatory_over(source, drain));
        let router = router(
            observatory,
            Arc::clone(&probes),
            pgprox_core::ids::NodeId::new(1),
            Arc::new(pgprox_observe::tenants::TenantAllowlist::new()),
            test_slab(),
            Arc::new(crate::routes::RouteCounts::new()),
        );

        assert_eq!(get(&router, "/readyz").await, (200, "ok".to_owned()));
        assert_eq!(post(&router, "/v1/drain").await.0, 200);

        assert_eq!(
            get(&router, "/readyz").await,
            (503, Reason::Draining.as_str().to_owned()),
            "a drain through the admin API left the probe passing"
        );
        assert_eq!(
            get(&router, "/healthz").await.0,
            200,
            "draining is not a reason to restart the process"
        );
    }

    #[tokio::test]
    async fn a_loop_that_stops_beating_fails_liveness_and_one_that_beats_does_not() {
        // `M17.4`: `beat` replaced with nothing survived, so a wedged node
        // would have passed `/healthz` forever. Liveness is the only probe a
        // restart is the answer to, and it fails on exactly one thing: the run
        // loop stopped. Before the first beat there is nothing to have
        // stopped, which is why this beats first.
        let clock = Arc::new(FakeClock::new());
        let health: SharedHealth = Arc::new(Mutex::new(Health::new(HealthConfig::default())));
        lock(&health).started();
        let probes = Arc::new(Probes::new(
            health,
            Arc::new(Mutex::new(DrainState::new(
                "pgprox-1",
                DrainConfig::default(),
            ))),
            FakeConfigSource::new(Config::default()).unwrap(),
            Arc::clone(&clock) as Arc<dyn Clock>,
        ));

        probes.beat();
        let router = probe_routes(Arc::clone(&probes));
        assert_eq!(get(&router, "/healthz").await, (200, "ok".to_owned()));

        // Past the timeout with no further beat.
        clock
            .advance(HealthConfig::default().heartbeat_timeout + std::time::Duration::from_secs(1));
        let (status, body) = get(&router, "/healthz").await;
        assert_eq!(status, 503, "a wedged loop still reported alive");
        assert_eq!(body, Reason::Stuck.as_str());

        // And a beat brings it back, which is what makes the assertion above
        // about the heartbeat rather than about the clock.
        probes.beat();
        assert_eq!(get(&router, "/healthz").await, (200, "ok".to_owned()));
    }

    #[test]
    fn probes_print_which_type_they_are() {
        // `M17.4`: this `Debug` could return an empty string. It is what an
        // operator reads in a panic from the HTTP task, and a blank there
        // names nothing at all.
        let (probes, _drain, _source) = probes_over(Config::default());
        let rendered = format!("{probes:?}");
        assert!(rendered.contains("Probes"), "{rendered}");
    }
}
