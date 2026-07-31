//! The HTTP/JSON admin API.
//!
//! # Hitting any pod gives the whole truth
//!
//! Aggregates answer from the local gossip digest, so there is no wrong pod to
//! ask and no cost to asking. Only drill-downs fan out, and those are the ones
//! that can come back partial. `?scope=local` narrows any read to the node that
//! answered. See ADR 0007.
//!
//! # What a response may contain
//!
//! No credentials, which holds structurally: the [`Observatory`] DTOs have no
//! field for one, so a handler cannot leak what it was never given. See ADR
//! 0018.
//!
//! Upstream hostnames are a different question. They appear in pool keys, and
//! an operator debugging a pool needs them. This API is for operators and is
//! expected to be reachable only from an authenticated admin surface, which is
//! a deployment decision rather than something this crate can enforce. It is
//! said here rather than assumed.
//!
//! # Errors say which kind of incomplete they are
//!
//! A fan-out that lost a peer answers 206 with the rows it did gather, not 200
//! and not 500. An operator seeing 200 concludes the tenant has no clients; one
//! seeing 500 concludes the proxy is broken. Neither is true, and the
//! difference decides whether they act.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use pgprox_core::admin::{
    AdminError, ClientView, ClusterView, Observatory, PoolView, Scope, ServerView, Stats,
    TenantView,
};
use pgprox_core::ids::{PoolKey, ServerId, TenantId};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What every handler is given.
pub type Shared = Arc<dyn Observatory>;

/// The `?scope=` query parameter.
#[derive(Debug, Default, Deserialize)]
pub struct ScopeQuery {
    /// `cluster` or `local`.
    #[serde(default)]
    pub scope: Option<String>,
}

impl ScopeQuery {
    /// Reads the scope, or reports what was not understood.
    ///
    /// # Errors
    ///
    /// [`ApiError::BadRequest`] for an unrecognised value. Quietly answering
    /// for the cluster when somebody asked for something they spelled wrong is
    /// how an operator draws the wrong conclusion from a real number.
    pub fn resolve(&self) -> Result<Scope, ApiError> {
        match self.scope.as_deref() {
            None => Ok(Scope::Cluster),
            Some(value) => Scope::parse(value).ok_or_else(|| {
                ApiError::BadRequest(format!("scope must be `cluster` or `local`, got `{value}`"))
            }),
        }
    }
}

/// What a handler can fail with.
#[derive(Debug)]
#[non_exhaustive]
pub enum ApiError {
    /// The request was not understood.
    BadRequest(String),
    /// The thing asked about does not exist.
    NotFound(String),
    /// The answer is incomplete because a peer did not respond.
    Partial(String),
    /// The request was understood and refused.
    Refused(String),
}

impl From<AdminError> for ApiError {
    fn from(err: AdminError) -> Self {
        match &err {
            AdminError::NotFound { .. } => Self::NotFound(err.to_string()),
            AdminError::Partial { .. } => Self::Partial(err.to_string()),
            // `AdminError` is `#[non_exhaustive]`, so `Refused` shares this arm
            // with any variant added later. A new failure this crate has not
            // been taught about is reported as a refusal carrying its own
            // message, which beats reporting it as a success.
            _ => Self::Refused(err.to_string()),
        }
    }
}

/// The body of an error response.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ErrorBody {
    /// What went wrong, in terms an operator can act on.
    pub error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::NotFound(message) => (StatusCode::NOT_FOUND, message),
            // 206 rather than 200 or 500. An operator seeing 200 concludes the
            // tenant has no clients; one seeing 500 concludes the proxy is
            // broken. The answer is neither, and which it is decides whether
            // they act.
            Self::Partial(message) => (StatusCode::PARTIAL_CONTENT, message),
            Self::Refused(message) => (StatusCode::CONFLICT, message),
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

/// `GET /v1/cluster`
///
/// Always the answering node's own view, whatever the scope: comparing two of
/// them is how split brain is found, so narrowing it would remove the point.
#[utoipa::path(
    get, path = "/v1/cluster", tag = "read",
    responses((status = 200, description = "This node's view of the cluster")),
)]
pub async fn cluster(State(observatory): State<Shared>) -> Json<ClusterBody> {
    Json(ClusterBody::from(observatory.cluster()))
}

/// `GET /v1/pools`
#[utoipa::path(
    get, path = "/v1/pools", tag = "read",
    params(("scope" = Option<String>, Query, description = "cluster (default) or local")),
    responses(
        (status = 200, description = "Upstream pools"),
        (status = 400, description = "Unrecognised scope", body = ErrorBody),
    ),
)]
pub async fn pools(
    State(observatory): State<Shared>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Vec<PoolBody>>, ApiError> {
    let scope = query.resolve()?;
    Ok(Json(
        observatory
            .pools(scope)
            .into_iter()
            .map(PoolBody::from)
            .collect(),
    ))
}

/// `GET /v1/servers`
///
/// The capacity view: caps, usage, headroom. Its `SHOW` equivalent is
/// `SHOW QUOTA`, **not** `SHOW SERVERS`, which is `PgBouncer`'s per-connection
/// socket view and has to keep that shape. The shared word is a trap, and
/// `tests/surfaces_agree.rs` pins the correspondence that actually holds.
#[utoipa::path(
    get, path = "/v1/servers", tag = "read",
    params(("scope" = Option<String>, Query, description = "cluster (default) or local")),
    responses(
        (status = 200, description = "Upstream servers and their caps. The SHOW \
                                      equivalent is SHOW QUOTA, not SHOW SERVERS."),
        (status = 400, description = "Unrecognised scope", body = ErrorBody),
    ),
)]
pub async fn servers(
    State(observatory): State<Shared>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Vec<ServerBody>>, ApiError> {
    let scope = query.resolve()?;
    Ok(Json(
        observatory
            .servers(scope)
            .into_iter()
            .map(ServerBody::from)
            .collect(),
    ))
}

/// `GET /v1/tenants`
#[utoipa::path(
    get, path = "/v1/tenants", tag = "read",
    params(("scope" = Option<String>, Query, description = "cluster (default) or local")),
    responses(
        (status = 200, description = "Tenants"),
        (status = 400, description = "Unrecognised scope", body = ErrorBody),
    ),
)]
pub async fn tenants(
    State(observatory): State<Shared>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Vec<TenantBody>>, ApiError> {
    let scope = query.resolve()?;
    Ok(Json(
        observatory
            .tenants(scope)
            .into_iter()
            .map(TenantBody::from)
            .collect(),
    ))
}

/// `GET /v1/tenants/{id}`
#[utoipa::path(
    get, path = "/v1/tenants/{id}", tag = "read",
    params(("id" = String, Path, description = "Tenant identifier")),
    responses(
        (status = 200, description = "One tenant"),
        (status = 404, description = "No such tenant", body = ErrorBody),
    ),
)]
pub async fn tenant(
    State(observatory): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<TenantBody>, ApiError> {
    observatory
        .tenant(&TenantId::new(&id))
        .map(|view| Json(TenantBody::from(view)))
        .ok_or_else(|| ApiError::NotFound(format!("no such tenant: {id}")))
}

/// `GET /v1/clients`
///
/// The drill-down that fans out, and so the one that can come back partial.
#[utoipa::path(
    get, path = "/v1/clients", tag = "read",
    params(("scope" = Option<String>, Query, description = "cluster (default) or local")),
    responses(
        (status = 200, description = "Client connections"),
        (status = 206, description = "Some nodes did not answer", body = ErrorBody),
        (status = 400, description = "Unrecognised scope", body = ErrorBody),
    ),
)]
pub async fn clients(
    State(observatory): State<Shared>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<Vec<ClientBody>>, ApiError> {
    let scope = query.resolve()?;
    let clients = observatory.clients(scope).await?;
    Ok(Json(clients.into_iter().map(ClientBody::from).collect()))
}

/// `GET /v1/stats`
#[utoipa::path(
    get, path = "/v1/stats", tag = "read",
    params(("scope" = Option<String>, Query, description = "cluster (default) or local")),
    responses(
        (status = 200, description = "Fleet counters"),
        (status = 400, description = "Unrecognised scope", body = ErrorBody),
    ),
)]
pub async fn stats(
    State(observatory): State<Shared>,
    Query(query): Query<ScopeQuery>,
) -> Result<Json<StatsBody>, ApiError> {
    let scope = query.resolve()?;
    Ok(Json(StatsBody::from(observatory.stats(scope))))
}

/// `GET /v1/cache`
///
/// No `scope`. ADR 0021 makes the cache one node's rather than the fleet's, so
/// there is nothing to aggregate: two nodes hold different entries for the same
/// tenant, and a summed hit count would describe nothing that happened
/// anywhere. A caller wanting the fleet's picture asks every node, which is the
/// honest shape of the question.
#[utoipa::path(
    get, path = "/v1/cache", tag = "read",
    responses((status = 200, description = "The query cache on the node that answered")),
)]
pub async fn cache(State(observatory): State<Shared>) -> Json<CacheBody> {
    Json(CacheBody::from(observatory.cache()))
}

/// `GET /v1/config`
#[utoipa::path(
    get, path = "/v1/config", tag = "read",
    responses((status = 200, description = "The configuration in force")),
)]
pub async fn config(State(observatory): State<Shared>) -> Json<ConfigBody> {
    let config = observatory.config();
    Json(ConfigBody {
        max_client_conns: config.max_client_conns,
        drain_grace_ms: u64::try_from(config.drain_grace.as_millis()).unwrap_or(u64::MAX),
        grant_ttl_cap_ms: u64::try_from(config.grant_ttl_cap.as_millis()).unwrap_or(u64::MAX),
        servers: config
            .servers
            .iter()
            .map(|server| ConfigServerBody {
                server: server.server.to_string(),
                max_connections: server.max_connections,
                guaranteed_fraction: server.guaranteed_fraction,
            })
            .collect(),
        nodes: config
            .nodes
            .iter()
            .map(|(name, node)| ConfigNodeBody {
                node: name.clone(),
                mode: format!("{:?}", node.mode).to_lowercase(),
            })
            .collect(),
    })
}

/// What a drain request carries.
#[derive(Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DrainRequest {
    /// How long the drain should last, in milliseconds.
    ///
    /// Omitted takes the default. There is no way to ask for no expiry at all,
    /// which is why the contract takes a `Duration` rather than an `Option`: a
    /// drain that never lapses belongs in the config document, where it is
    /// reviewed and survives a restart. See ADR 0006.
    #[serde(default)]
    pub ttl_ms: Option<u64>,
}

/// What a write returned.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AcceptedBody {
    /// What happened, for a human reading a terminal.
    pub result: String,
    /// How long a drain will last, in milliseconds.
    ///
    /// The applied value, which may be shorter than the one asked for. A caller
    /// that requested a week and got four hours finds out here rather than when
    /// the node comes back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
}

/// What a pool reset returned.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ResetBody {
    /// How many idle connections were closed.
    ///
    /// Connections in use are finishing real transactions and are left alone,
    /// so this is the number an operator should expect to be smaller than the
    /// pool.
    pub closed: u32,
}

/// `POST /v1/drain`
///
/// Writes the same desired state the config document would, carrying a TTL so
/// it expires. A node drained at 2am that stays drained forever is
/// indistinguishable from one somebody meant to drain.
#[utoipa::path(
    post, path = "/v1/drain", tag = "write",
    request_body = DrainRequest,
    responses(
        (status = 200, description = "The node is draining", body = AcceptedBody),
        (status = 409, description = "Refused", body = ErrorBody),
    ),
)]
pub async fn drain(
    State(observatory): State<Shared>,
    body: Option<Json<DrainRequest>>,
) -> Result<Json<AcceptedBody>, ApiError> {
    // An empty body is a drain with the default TTL, because `curl -X POST`
    // with no body is what an operator types under pressure.
    let ttl = body
        .and_then(|Json(request)| request.ttl_ms)
        .map_or(DEFAULT_DRAIN_TTL, Duration::from_millis);

    let applied = observatory.drain(ttl).await?;
    Ok(Json(AcceptedBody {
        result: "draining".to_owned(),
        ttl_ms: Some(u64::try_from(applied.as_millis()).unwrap_or(u64::MAX)),
    }))
}

/// `POST /v1/undrain`
///
/// Removes an imperative drain. It cannot undo one the config document asked
/// for: that would reverse a reviewed change, and the next config poll would
/// flip it back anyway.
#[utoipa::path(
    post, path = "/v1/undrain", tag = "write",
    responses(
        (status = 200, description = "The node is active", body = AcceptedBody),
        (status = 409, description = "The document drains this node", body = ErrorBody),
    ),
)]
pub async fn undrain(State(observatory): State<Shared>) -> Result<Json<AcceptedBody>, ApiError> {
    observatory.undrain().await?;
    Ok(Json(AcceptedBody {
        result: "active".to_owned(),
        ttl_ms: None,
    }))
}

/// `POST /v1/pools/{server}/{database}/{user}/reset`
///
/// Closes a pool's idle connections. Connections in use are finishing real
/// transactions, and an operator asking for a reset is not asking for those to
/// fail.
///
/// The key is three path segments rather than one opaque string, because an
/// operator types this from what `GET /v1/pools` showed them and a composite
/// key would have to be escaped.
#[utoipa::path(
    post, path = "/v1/pools/{server}/{database}/{user}/reset", tag = "write",
    params(
        ("server" = String, Path, description = "host:port"),
        ("database" = String, Path, description = "Database name"),
        ("user" = String, Path, description = "Role"),
    ),
    responses(
        (status = 200, description = "Idle connections closed", body = ResetBody),
        (status = 404, description = "No such pool", body = ErrorBody),
    ),
)]
pub async fn reset_pool(
    State(observatory): State<Shared>,
    Path((server, database, user)): Path<(String, String, String)>,
) -> Result<Json<ResetBody>, ApiError> {
    let Some(server_id) = ServerId::parse(&server) else {
        return Err(ApiError::BadRequest(format!(
            "server must be `host:port`, got `{server}`"
        )));
    };
    let key = PoolKey::new(server_id, &database, &user);
    let closed = observatory.reset_pool(&key).await?;
    Ok(Json(ResetBody { closed }))
}

/// The TTL used when a caller gives none.
///
/// Mirrors `pgprox_config::DrainConfig::default`. This crate cannot depend on
/// that one, so the value is repeated with a test in `pgprox-config` holding
/// the two together.
const DEFAULT_DRAIN_TTL: Duration = Duration::from_secs(30 * 60);

/// Declares a set of routes and the list of paths they serve, from one source.
///
/// The list is not maintained beside the router, it *is* the router: a path
/// added here appears in both, and one added to the router alone cannot exist.
/// That matters because the drift the `OpenAPI` tests look for is a route served
/// with no annotation, and a hand-written list of paths would drift the same
/// way the document does, leaving the comparison passing while both were wrong.
macro_rules! declare_routes {
    ($paths:ident, $router:ident, $($method:ident $path:literal => $handler:ident),* $(,)?) => {
        /// The paths this half serves, in the spelling `OpenAPI` uses.
        pub const $paths: &[&str] = &[$($path),*];

        #[doc = "The routes this half serves."]
        pub fn $router() -> axum::Router<Shared> {
            axum::Router::new()
                $(.route($path, axum::routing::$method($handler)))*
        }
    };
}

// The read half. Split from the writes so a deployment can expose it on a
// surface with different access: reading pool depths and draining a node are
// not the same privilege.
declare_routes!(
    READ_PATHS,
    read_routes,
    get "/v1/cluster" => cluster,
    get "/v1/pools" => pools,
    get "/v1/servers" => servers,
    get "/v1/tenants" => tenants,
    get "/v1/tenants/{id}" => tenant,
    get "/v1/clients" => clients,
    get "/v1/stats" => stats,
    get "/v1/config" => config,
    get "/v1/cache" => cache,
);

// The write half.
declare_routes!(
    WRITE_PATHS,
    write_routes,
    post "/v1/drain" => drain,
    post "/v1/undrain" => undrain,
    post "/v1/pools/{server}/{database}/{user}/reset" => reset_pool,
);

/// Every route.
pub fn routes() -> axum::Router<Shared> {
    read_routes().merge(write_routes())
}

/// Every path the admin API serves.
///
/// Derived from the routers rather than written beside them, so the `OpenAPI`
/// tests compare the document against what is actually served.
#[must_use]
pub fn all_paths() -> Vec<&'static str> {
    READ_PATHS.iter().chain(WRITE_PATHS).copied().collect()
}

// The response bodies. Separate from the `Observatory` DTOs so the wire format
// is this crate's to own: a field can be renamed in `pgprox-core` without
// breaking every dashboard, which is the same reasoning as the config document.

/// One node's gossip digest.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DigestBody {
    /// Which node.
    pub node: u16,
    /// `active` or `draining`.
    pub mode: String,
    /// Client connections it holds.
    pub client_conns: u32,
}

/// The cluster as one node sees it.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ClusterBody {
    /// Which node answered.
    pub node: u16,
    /// Nodes taking work.
    pub active: usize,
    /// Every node it knows about.
    pub members: Vec<DigestBody>,
    /// The membership view hash. A mismatch across pods is split brain.
    pub view_hash: String,
}

impl From<ClusterView> for ClusterBody {
    fn from(view: ClusterView) -> Self {
        Self {
            node: view.node.get(),
            active: view.membership.active_count(),
            members: view
                .digests
                .iter()
                .map(|digest| DigestBody {
                    node: digest.node.get(),
                    mode: format!("{:?}", digest.mode).to_lowercase(),
                    client_conns: digest.client_conns,
                })
                .collect(),
            // A string, because a u64 does not survive JSON's number type in
            // every client. A hash that changes when it reaches JavaScript is
            // worse than no hash at all.
            view_hash: format!("{:016x}", view.view_hash),
        }
    }
}

/// One upstream pool.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PoolBody {
    /// Which node holds it.
    pub node: u16,
    /// `host:port`.
    pub server: String,
    /// The database.
    pub database: String,
    /// The role.
    pub user: String,
    /// Connections checked out.
    pub active: u32,
    /// Connections open and idle.
    pub idle: u32,
    /// Callers waiting.
    pub waiting: u32,
    /// The cap in force.
    pub limit: u32,
}

impl From<PoolView> for PoolBody {
    fn from(view: PoolView) -> Self {
        Self {
            node: view.node.get(),
            server: view.key.server.to_string(),
            database: view.key.database.to_string(),
            user: view.key.user.to_string(),
            active: view.stats.active,
            idle: view.stats.idle,
            waiting: view.stats.waiting,
            limit: view.stats.limit,
        }
    }
}

/// One upstream server.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerBody {
    /// `host:port`.
    pub server: String,
    /// The cluster-wide cap.
    pub cap: u32,
    /// Connections every node reports holding.
    pub in_use: u32,
    /// Room left against the cap.
    pub headroom: u32,
    /// What the answering node may open without asking.
    pub guaranteed: u32,
    /// What it holds on lease beyond that.
    pub leased: u32,
}

impl From<ServerView> for ServerBody {
    fn from(view: ServerView) -> Self {
        Self {
            server: view.server.to_string(),
            cap: view.cap,
            in_use: view.in_use,
            headroom: view.headroom(),
            guaranteed: view.guaranteed,
            leased: view.leased,
        }
    }
}

/// The query cache on one node.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CacheBody {
    /// Tenants configured to be served. Zero is a cache that is off.
    pub tenants: u64,
    /// What the cache guarantees, in words rather than by omission.
    ///
    /// `off`, or `bounded staleness`. ADR 0021 asks for this in as many words:
    /// the guarantee people infer from a cache is read-your-writes, this one
    /// does not offer it, and a field that simply said nothing would be read as
    /// agreement.
    pub promise: String,
    /// Entries currently held.
    pub entries: u64,
    /// Bytes currently held.
    pub bytes: u64,
    /// The budget those are held against.
    pub max_bytes: u64,
    /// Lookups that found a live entry.
    pub hits: u64,
    /// Lookups that found nothing.
    pub misses: u64,
    /// Lookups that found an entry past its TTL.
    pub expired: u64,
    /// Entries thrown out to stay inside the budget.
    pub evicted: u64,
    /// Entries dropped because a tenant wrote.
    pub invalidated: u64,
    /// Results too large to store at all.
    pub rejected: u64,
}

impl From<pgprox_core::admin::CacheView> for CacheBody {
    fn from(view: pgprox_core::admin::CacheView) -> Self {
        Self {
            tenants: view.tenants,
            promise: if view.is_off() {
                "off".to_owned()
            } else {
                "bounded staleness".to_owned()
            },
            entries: view.entries,
            bytes: view.bytes,
            max_bytes: view.max_bytes,
            hits: view.hits,
            misses: view.misses,
            expired: view.expired,
            evicted: view.evicted,
            invalidated: view.invalidated,
            rejected: view.rejected,
        }
    }
}

/// One tenant.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TenantBody {
    /// Which tenant.
    pub tenant: String,
    /// The node that homes it, if any.
    pub home: Option<u16>,
    /// Client connections it has.
    pub client_conns: u32,
    /// Upstream connections held for it.
    pub upstream_conns: u32,
}

impl From<TenantView> for TenantBody {
    fn from(view: TenantView) -> Self {
        Self {
            tenant: view.tenant.as_str().to_owned(),
            home: view.home.map(pgprox_core::ids::NodeId::get),
            client_conns: view.client_conns,
            upstream_conns: view.upstream_conns,
        }
    }
}

/// One client connection.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ClientBody {
    /// The connection identifier.
    pub conn: String,
    /// Which tenant.
    pub tenant: String,
    /// The node serving it.
    pub node: u16,
    /// `idle`, `active` or `waiting`.
    pub state: String,
    /// How long it has been in that state.
    pub since_ms: u64,
    /// Why it is pinned, if it is.
    pub pinned: Option<String>,
}

impl From<ClientView> for ClientBody {
    fn from(view: ClientView) -> Self {
        Self {
            conn: view.conn.to_string(),
            tenant: view.tenant.as_str().to_owned(),
            node: view.node.get(),
            state: view.state.as_str().to_owned(),
            since_ms: u64::try_from(view.since.as_millis()).unwrap_or(u64::MAX),
            pinned: view.pinned,
        }
    }
}

/// Fleet counters.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct StatsBody {
    /// Client connections.
    pub client_conns: u32,
    /// Upstream connections.
    pub upstream_conns: u32,
    /// Transactions served since start.
    pub transactions: u64,
    /// Sessions pinned since start.
    pub pins: u64,
    /// Clients shed since start.
    pub sheds: u64,
    /// Callers waiting for an upstream connection.
    pub waiting: u32,
}

impl From<Stats> for StatsBody {
    fn from(stats: Stats) -> Self {
        Self {
            client_conns: stats.client_conns,
            upstream_conns: stats.upstream_conns,
            transactions: stats.transactions,
            pins: stats.pins,
            sheds: stats.sheds,
            waiting: stats.waiting,
        }
    }
}

/// One server's configured limits.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConfigServerBody {
    /// `host:port`.
    pub server: String,
    /// The cluster-wide cap.
    pub max_connections: u32,
    /// Fraction handed out as guaranteed share.
    pub guaranteed_fraction: f64,
}

/// One node's configured mode.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConfigNodeBody {
    /// The node name.
    pub node: String,
    /// `active` or `draining`.
    pub mode: String,
}

/// The configuration in force.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConfigBody {
    /// Client connections this node accepts.
    pub max_client_conns: u32,
    /// How long a draining node waits before force-closing.
    pub drain_grace_ms: u64,
    /// Upper bound on grant caching.
    pub grant_ttl_cap_ms: u64,
    /// Per-server limits.
    pub servers: Vec<ConfigServerBody>,
    /// Per-node overrides.
    pub nodes: Vec<ConfigNodeBody>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    #[test]
    fn the_default_drain_ttl_is_thirty_minutes() {
        // `30 * 60` could become `30 + 60`, which is ninety seconds, or
        // `30 / 60`, which is none at all. Two tests were supposed to stop
        // that and neither could.
        //
        // This crate's own check is `assert_eq!(ttl, Some(DEFAULT_DRAIN_TTL))`,
        // which compares the result against the constant that produced it and
        // passes for any value. `pgprox-config`'s check asserts its own
        // literal and says in a comment that this crate mirrors it, but the
        // two cannot see each other: this crate cannot depend on that one,
        // which is why the value is repeated in the first place.
        //
        // So the pairing only works if each side pins the literal
        // independently, which is what this does.
        assert_eq!(
            DEFAULT_DRAIN_TTL,
            Duration::from_secs(1_800),
            "thirty minutes; pgprox-config::DrainConfig::default mirrors this"
        );
    }

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use pgprox_core::admin::{ClientState, FakeObservatory};
    use pgprox_core::cluster::{ClusterDigest, NodeMode};
    use pgprox_core::config::{Config, ServerConfig};
    use pgprox_core::ids::{ConnId, NodeId, PoolKey, ServerId};
    use pgprox_core::pool::PoolStats;
    use std::time::Duration;
    use tower::ServiceExt;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn observatory() -> Arc<FakeObservatory> {
        let fake = FakeObservatory::new(node(1));
        fake.set_pools(vec![
            PoolView {
                node: node(1),
                key: PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app"),
                stats: PoolStats {
                    active: 2,
                    idle: 1,
                    waiting: 0,
                    limit: 10,
                },
            },
            PoolView {
                node: node(2),
                key: PoolKey::new(ServerId::new("db-1", 5432), "tenant_globex", "globex_app"),
                stats: PoolStats {
                    active: 1,
                    idle: 0,
                    waiting: 3,
                    limit: 10,
                },
            },
        ]);
        fake.set_servers(vec![ServerView {
            server: ServerId::new("db-1", 5432),
            cap: 100,
            in_use: 60,
            guaranteed: 10,
            leased: 5,
        }]);
        fake.set_tenants(vec![TenantView {
            tenant: TenantId::new("acme"),
            home: Some(node(1)),
            client_conns: 7,
            upstream_conns: 3,
        }]);
        fake.set_clients(vec![
            ClientView {
                conn: ConnId::new(node(1), 42),
                tenant: TenantId::new("acme"),
                node: node(1),
                state: ClientState::Idle,
                since: Duration::from_secs(5),
                pinned: None,
            },
            ClientView {
                conn: ConnId::new(node(2), 7),
                tenant: TenantId::new("globex"),
                node: node(2),
                state: ClientState::Active,
                since: Duration::from_secs(1),
                pinned: Some("listen".to_owned()),
            },
        ]);
        fake.set_digest(ClusterDigest {
            node: node(1),
            mode: NodeMode::Active,
            client_conns: 7,
            upstream_conns: Vec::new(),
            tenant_usage: Vec::new(),
        });
        fake.set_digest(ClusterDigest {
            node: node(2),
            mode: NodeMode::Draining,
            client_conns: 2,
            upstream_conns: Vec::new(),
            tenant_usage: Vec::new(),
        });
        fake.set_config(Config {
            servers: vec![ServerConfig {
                server: ServerId::new("db-1", 5432),
                max_connections: 100,
                guaranteed_fraction: 0.5,
            }],
            ..Config::default()
        });
        fake
    }

    /// Sends a request and returns the status and the parsed JSON body.
    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        let shared: Shared = observatory();
        let response = read_routes()
            .with_state(shared)
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("responses are JSON")
        };
        (status, json)
    }

    #[tokio::test]
    async fn an_aggregate_answers_for_the_whole_cluster_by_default() {
        // Hitting any pod gives the whole truth, which is why there is no wrong
        // pod to ask.
        let (status, body) = get("/v1/pools").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn scope_local_narrows_to_the_answering_node() {
        let (status, body) = get("/v1/pools?scope=local").await;
        assert_eq!(status, StatusCode::OK);

        let pools = body.as_array().unwrap();
        assert_eq!(pools.len(), 1);
        assert_eq!(pools[0]["node"], 1);
    }

    #[tokio::test]
    async fn an_unrecognised_scope_is_a_bad_request_rather_than_a_default() {
        // Quietly answering for the cluster when somebody asked for something
        // they spelled wrong is how an operator draws the wrong conclusion from
        // a real number.
        let (status, body) = get("/v1/pools?scope=loca").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap().contains("loca"),
            "the error should name what was not understood: {body}"
        );
    }

    #[tokio::test]
    async fn every_read_endpoint_honours_scope() {
        // A rule that holds for one handler and not the others is worse than no
        // rule, because it is the one nobody checks.
        for path in [
            "/v1/pools",
            "/v1/servers",
            "/v1/tenants",
            "/v1/clients",
            "/v1/stats",
        ] {
            let (status, _) = get(&format!("{path}?scope=local")).await;
            assert_eq!(status, StatusCode::OK, "{path} refused scope=local");

            let (status, _) = get(&format!("{path}?scope=nonsense")).await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "{path} accepted a nonsense scope"
            );
        }
    }

    #[tokio::test]
    async fn the_cluster_view_carries_a_hash_two_pods_can_compare() {
        let (status, body) = get("/v1/cluster").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["node"], 1);
        assert_eq!(body["members"].as_array().unwrap().len(), 2);
        assert_eq!(body["active"], 1, "a draining node counted as active");
        // A string, because a u64 does not survive JSON's number type in every
        // client, and a hash that changes when it reaches JavaScript is worse
        // than no hash at all.
        assert!(body["view_hash"].is_string(), "{body}");
    }

    #[tokio::test]
    async fn a_named_tenant_can_be_asked_about_and_a_missing_one_says_so() {
        let (status, body) = get("/v1/tenants/acme").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tenant"], "acme");
        assert_eq!(body["client_conns"], 7);

        let (status, body) = get("/v1/tenants/nobody").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("nobody"));
    }

    #[tokio::test]
    async fn a_partial_fan_out_answers_206_rather_than_200_or_500() {
        // An operator seeing 200 concludes the tenant has no clients; one
        // seeing 500 concludes the proxy is broken. The answer is neither, and
        // which it is decides whether they act.
        let fake = observatory();
        fake.set_unreachable(true);
        let shared: Shared = fake;

        let response = read_routes()
            .with_state(shared)
            .oneshot(
                Request::builder()
                    .uri("/v1/clients")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    }

    #[tokio::test]
    async fn a_local_read_needs_no_peers_and_still_answers() {
        let fake = observatory();
        fake.set_unreachable(true);
        let shared: Shared = fake;

        let response = read_routes()
            .with_state(shared)
            .oneshot(
                Request::builder()
                    .uri("/v1/clients?scope=local")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_client_row_carries_what_an_operator_needs_to_act() {
        let (status, body) = get("/v1/clients").await;
        assert_eq!(status, StatusCode::OK);

        let clients = body.as_array().unwrap();
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0]["state"], "idle");
        assert_eq!(clients[0]["tenant"], "acme");
        assert!(clients[0]["pinned"].is_null());
        assert_eq!(clients[1]["pinned"], "listen");
    }

    #[tokio::test]
    async fn a_server_row_reports_headroom_rather_than_making_it_arithmetic() {
        let (status, body) = get("/v1/servers").await;
        assert_eq!(status, StatusCode::OK);

        let servers = body.as_array().unwrap();
        assert_eq!(servers[0]["cap"], 100);
        assert_eq!(servers[0]["in_use"], 60);
        assert_eq!(servers[0]["headroom"], 40);
    }

    #[tokio::test]
    async fn the_configuration_in_force_is_readable() {
        let (status, body) = get("/v1/config").await;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["max_client_conns"], 10_000);
        assert_eq!(body["servers"][0]["server"], "db-1:5432");
        assert_eq!(body["servers"][0]["max_connections"], 100);
    }

    #[tokio::test]
    async fn stats_reflect_the_scope_asked_for() {
        let (_, cluster) = get("/v1/stats").await;
        let (_, local) = get("/v1/stats?scope=local").await;

        assert_eq!(cluster["client_conns"], 2);
        assert_eq!(local["client_conns"], 1);
    }

    #[tokio::test]
    async fn no_response_contains_anything_that_looks_like_a_credential() {
        // Structural, because the Observatory DTOs have no field for one, but
        // asserted anyway: the guarantee is only as good as the next person
        // adding a field to a response body.
        for path in [
            "/v1/cluster",
            "/v1/pools",
            "/v1/servers",
            "/v1/tenants",
            "/v1/tenants/acme",
            "/v1/clients",
            "/v1/stats",
            "/v1/config",
        ] {
            let (status, body) = get(path).await;
            assert_eq!(status, StatusCode::OK, "{path}");

            let rendered = body.to_string().to_lowercase();
            for forbidden in ["password", "secret", "token", "redacted", "jwt"] {
                assert!(
                    !rendered.contains(forbidden),
                    "{path} rendered something that looks like a credential: {rendered}"
                );
            }
        }
    }

    /// Sends a POST and returns the status and the parsed body.
    async fn post(
        fake: &Arc<FakeObservatory>,
        uri: &str,
        body: &str,
    ) -> (StatusCode, serde_json::Value) {
        let shared: Shared = Arc::clone(fake) as Shared;
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_owned()))
            .unwrap();

        let response = routes().with_state(shared).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("responses are JSON")
        };
        (status, json)
    }

    #[tokio::test]
    async fn draining_writes_the_same_desired_state_with_a_ttl() {
        // The imperative path and the declarative one end in the same place;
        // the TTL is what stops a 2am drain outliving the incident.
        let fake = observatory();
        let (status, body) = post(&fake, "/v1/drain", r#"{"ttl_ms": 600000}"#).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"], "draining");
        assert_eq!(
            fake.mode(),
            (NodeMode::Draining, Some(Duration::from_secs(600)))
        );
    }

    #[tokio::test]
    async fn a_drain_with_no_body_takes_the_default_ttl() {
        // `curl -X POST` with no body is what an operator types under pressure,
        // and it must not be a 400 at that moment.
        let fake = observatory();
        let shared: Shared = Arc::clone(&fake) as Shared;
        let response = routes()
            .with_state(shared)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/drain")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let (mode, ttl) = fake.mode();
        assert_eq!(mode, NodeMode::Draining);
        assert_eq!(ttl, Some(DEFAULT_DRAIN_TTL), "no TTL was applied");
    }

    #[tokio::test]
    async fn a_drain_always_carries_an_expiry() {
        // There is no way to ask for one that never lapses. A drain that should
        // outlive the incident belongs in the config document, where it is
        // reviewed and survives a restart.
        let fake = observatory();
        post(&fake, "/v1/drain", "{}").await;
        assert!(
            fake.mode().1.is_some(),
            "an API drain was written without an expiry"
        );
    }

    #[tokio::test]
    async fn undraining_clears_the_overlay() {
        let fake = observatory();
        post(&fake, "/v1/drain", "{}").await;
        assert_eq!(fake.mode().0, NodeMode::Draining);

        let (status, body) = post(&fake, "/v1/undrain", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"], "active");
        assert_eq!(fake.mode(), (NodeMode::Active, None));
    }

    #[tokio::test]
    async fn resetting_a_pool_closes_idle_connections_and_reports_how_many() {
        // Connections in use are finishing real transactions, and an operator
        // asking for a reset is not asking for those to fail.
        let fake = observatory();
        let (status, body) =
            post(&fake, "/v1/pools/db-1:5432/tenant_acme/acme_app/reset", "").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["closed"], 1);

        let remaining = fake.pools(Scope::Local);
        assert_eq!(remaining[0].stats.idle, 0);
        assert_eq!(
            remaining[0].stats.active, 2,
            "an in-use connection was closed"
        );
    }

    #[tokio::test]
    async fn resetting_a_pool_that_does_not_exist_is_a_404() {
        let fake = observatory();
        let (status, body) = post(&fake, "/v1/pools/db-9:5432/nope/nobody/reset", "").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("db-9"), "{body}");
    }

    #[tokio::test]
    async fn a_pool_key_that_is_not_a_server_address_is_a_bad_request() {
        // The path is typed by an operator from what GET /v1/pools showed them,
        // so a mistake here is a typo rather than an attack, and it should say
        // which part was wrong.
        let fake = observatory();
        let (status, body) = post(&fake, "/v1/pools/db-1/tenant_acme/acme_app/reset", "").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["error"].as_str().unwrap().contains("host:port"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn a_pool_key_round_trips_from_what_the_read_endpoint_showed() {
        // The one that would break silently: GET renders the key, POST parses
        // it back, and the two must agree about what a server address is.
        let (_, listed) = get("/v1/pools?scope=local").await;
        let pool = &listed.as_array().unwrap()[0];
        let uri = format!(
            "/v1/pools/{}/{}/{}/reset",
            pool["server"].as_str().unwrap(),
            pool["database"].as_str().unwrap(),
            pool["user"].as_str().unwrap(),
        );

        let fake = observatory();
        let (status, _) = post(&fake, &uri, "").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the key a read endpoint rendered was not accepted back"
        );
    }

    #[tokio::test]
    async fn the_write_routes_are_separable_from_the_reads() {
        // So a deployment can put them behind different access: reading pool
        // depths and draining a node are not the same privilege.
        let shared: Shared = observatory();
        let response = read_routes()
            .with_state(shared)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/drain")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "a write reached a router that was only given reads"
        );
    }

    #[tokio::test]
    async fn an_unknown_route_is_a_404_rather_than_a_panic() {
        let (status, _) = get("/v1/nothing-here").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn an_admin_error_variant_nobody_taught_this_crate_about_is_not_a_success() {
        // `AdminError` is non_exhaustive, so a variant added later lands in the
        // fallback. Reporting it as a refusal with its own message beats
        // reporting it as 200.
        let refused = ApiError::from(AdminError::Refused {
            reason: "already draining".into(),
        });
        assert!(matches!(refused, ApiError::Refused(_)));

        let not_found = ApiError::from(AdminError::NotFound {
            kind: "pool",
            name: "db-1".into(),
        });
        assert!(matches!(not_found, ApiError::NotFound(_)));

        let partial = ApiError::from(AdminError::Partial {
            reason: "pgprox-3 timed out".into(),
        });
        assert!(matches!(partial, ApiError::Partial(_)));
    }

    #[test]
    fn a_missing_scope_parameter_is_the_cluster() {
        let query = ScopeQuery { scope: None };
        assert_eq!(query.resolve().unwrap(), Scope::Cluster);

        let empty = ScopeQuery {
            scope: Some(String::new()),
        };
        assert_eq!(empty.resolve().unwrap(), Scope::Cluster);
    }
}
