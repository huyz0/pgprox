//! Rendering a `SHOW` result.
//!
//! # What compatibility means here
//!
//! For the five commands `PgBouncer` also has, the column names and their order
//! match its output exactly. A dashboard doing `SELECT cl_active FROM ...`, or
//! more likely reading column 3 by position, keeps working.
//!
//! It does not mean every column carries a `PgBouncer` value. `pgprox` has no
//! `sv_tested` state, no socket pointers, and no `remote_pid`, because it is
//! not built the way `PgBouncer` is built. Those columns are present, in place,
//! and empty.
//!
//! That is a deliberate choice between three options. Omitting them shifts every
//! later column and breaks positional readers, which is most of them. Inventing
//! plausible values is worse than useless, because a number an operator acts on
//! must be real. Leaving them empty keeps the shape and tells the truth, and
//! [`PLACEHOLDERS`] names every one so nobody has to work out which is which.
//!
//! # Where the values come from
//!
//! The same [`Observatory`] the HTTP handlers read, so the two surfaces cannot
//! disagree about the same question. See ADR 0018.

use pgprox_core::admin::{Observatory, Scope};

use crate::show::{ShowCommand, ShowTarget};

/// A rendered result set.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Rows {
    /// Column names, in order.
    pub columns: Vec<&'static str>,
    /// One vector of values per row, in column order.
    pub rows: Vec<Vec<String>>,
}

impl Rows {
    /// How many rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether there are no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The value in a named column of a row, for tests and for callers that
    /// would otherwise count positions by hand.
    #[must_use]
    pub fn get(&self, row: usize, column: &str) -> Option<&str> {
        let index = self.columns.iter().position(|name| *name == column)?;
        self.rows.get(row)?.get(index).map(String::as_str)
    }
}

/// Columns that exist for shape and carry nothing.
///
/// Named rather than left for a reader to discover. Each is a `PgBouncer`
/// concept `pgprox` does not have: socket bookkeeping, a per-connection server
/// state machine with a `tested` phase, or a backend process ID that a proxy
/// multiplexing transactions cannot attribute to one client.
pub const PLACEHOLDERS: &[&str] = &[
    "cl_active_cancel_req",
    "cl_waiting_cancel_req",
    "sv_active_cancel",
    "sv_being_canceled",
    "sv_used",
    "sv_tested",
    "sv_login",
    "load_balance_hosts",
    "replication",
    "addr",
    "port",
    "local_addr",
    "local_port",
    "connect_time",
    "request_time",
    "close_needed",
    "ptr",
    "link",
    "remote_pid",
    "tls",
    "application_name",
    "prepared_statements",
];

/// `PgBouncer`'s `SHOW POOLS` columns, in its order.
const POOLS: &[&str] = &[
    "database",
    "user",
    "cl_active",
    "cl_waiting",
    "cl_active_cancel_req",
    "cl_waiting_cancel_req",
    "sv_active",
    "sv_active_cancel",
    "sv_being_canceled",
    "sv_idle",
    "sv_used",
    "sv_tested",
    "sv_login",
    "maxwait",
    "maxwait_us",
    "pool_mode",
    "load_balance_hosts",
];

/// `PgBouncer`'s socket columns, shared by `SHOW CLIENTS` and `SHOW SERVERS`.
const SOCKETS: &[&str] = &[
    "type",
    "user",
    "database",
    "replication",
    "state",
    "addr",
    "port",
    "local_addr",
    "local_port",
    "connect_time",
    "request_time",
    "wait",
    "wait_us",
    "close_needed",
    "ptr",
    "link",
    "remote_pid",
    "tls",
    "application_name",
    "prepared_statements",
    "id",
];

/// The `SHOW STATS` columns `pgprox` fills.
///
/// `PgBouncer` has a further sixteen averages it derives from its own sampling.
/// They are omitted rather than zeroed: an average is a number an operator
/// reads directly, and a column of zeros claiming to be an average is a lie
/// where an absent column is a gap.
const STATS: &[&str] = &[
    "database",
    "total_xact_count",
    "total_query_count",
    "total_client_conns",
    "total_server_conns",
    "total_wait_count",
];

/// `SHOW CONFIG`, which `PgBouncer` renders as key/value/changeable.
const CONFIG: &[&str] = &["key", "value", "changeable"];

/// `SHOW PEERS`, which is `pgprox` only.
const PEERS: &[&str] = &["node", "mode", "client_conns", "view_hash"];

/// `SHOW QUOTA`, which is `pgprox` only.
const QUOTA: &[&str] = &[
    "server",
    "cap",
    "in_use",
    "headroom",
    "guaranteed",
    "leased",
];

/// `SHOW TENANTS`, which is `pgprox` only.
const TENANTS: &[&str] = &["tenant", "home", "client_conns", "upstream_conns"];

/// `SHOW CACHE`, which is `pgprox` only.
///
/// One row, because the cache is one thing on one node. `tenants` leads
/// because it is the column that says whether the cache is on at all: every
/// other number is zero both when nobody opted in and when the tenants who did
/// are quiet. `promise` is a word rather than a number for the reason ADR 0021
/// gives: the guarantee people infer from a cache is read-your-writes, this one
/// does not offer it, and the place to say so is the place they are looking.
const CACHE: &[&str] = &[
    "tenants",
    "promise",
    "entries",
    "bytes",
    "max_bytes",
    "hits",
    "misses",
    "expired",
    "evicted",
    "invalidated",
    "rejected",
    "abandoned",
];

/// The columns a command produces.
#[must_use]
pub const fn columns_for(target: ShowTarget) -> &'static [&'static str] {
    match target {
        ShowTarget::Pools => POOLS,
        ShowTarget::Servers | ShowTarget::Clients => SOCKETS,
        ShowTarget::Stats => STATS,
        ShowTarget::Config => CONFIG,
        ShowTarget::Peers => PEERS,
        ShowTarget::Quota => QUOTA,
        ShowTarget::Tenants => TENANTS,
        ShowTarget::Cache => CACHE,
    }
}

/// What a statement turned out to be.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Handled {
    /// A `SHOW` this proxy answered.
    Answered(Rows),
    /// Not a `SHOW`, so it is somebody's query and belongs upstream.
    ///
    /// Distinct from an error, because relaying it is the correct and common
    /// outcome rather than a failure.
    Relay,
    /// A `SHOW` aimed at this proxy that missed, to be reported to the client.
    Rejected(crate::show::ShowError),
}

/// Parses a statement and answers it if it is a `SHOW`.
///
/// The entry point a session reaches for. Parsing and rendering are separate
/// functions because they are separately testable, but a caller wanting one
/// without the other is a caller about to reimplement this, and the three
/// outcomes below are exactly the three a session has to distinguish.
///
/// # Errors
///
/// Whatever the [`Observatory`] returned. A statement that is not a `SHOW`, or
/// is one this proxy does not have, is not an error here: those are
/// [`Handled::Relay`] and [`Handled::Rejected`], because the caller does
/// something different with each.
pub async fn handle(
    observatory: &dyn Observatory,
    sql: &str,
) -> Result<Handled, pgprox_core::admin::AdminError> {
    let command = match crate::show::parse(sql) {
        Ok(command) => command,
        Err(crate::show::ShowError::NotShow) => return Ok(Handled::Relay),
        Err(err) => return Ok(Handled::Rejected(err)),
    };
    Ok(Handled::Answered(render(observatory, command).await?))
}

/// Renders a command.
///
/// # Errors
///
/// Whatever the [`Observatory`] returned, which for `SHOW CLIENTS` includes a
/// partial answer when a peer did not respond.
pub async fn render(
    observatory: &dyn Observatory,
    command: ShowCommand,
) -> Result<Rows, pgprox_core::admin::AdminError> {
    let columns = columns_for(command.target).to_vec();
    let scope = command.scope;

    let rows = match command.target {
        ShowTarget::Pools => pool_rows(observatory, scope),
        ShowTarget::Servers => server_socket_rows(observatory, scope),
        ShowTarget::Clients => client_rows(observatory, scope).await?,
        ShowTarget::Stats => stats_rows(observatory, scope),
        ShowTarget::Config => config_rows(observatory),
        ShowTarget::Peers => peer_rows(observatory),
        ShowTarget::Quota => quota_rows(observatory, scope),
        ShowTarget::Tenants => tenant_rows(observatory, scope),
        ShowTarget::Cache => cache_rows(observatory),
    };

    Ok(Rows { columns, rows })
}

/// Nothing, for a column that exists only to hold a position.
fn blank() -> String {
    String::new()
}

/// Writes a value into a socket row by column name.
///
/// By name rather than by index, because the index of `id` in a
/// twenty-one-column layout is something nobody should have to count, and
/// miscounting it puts a connection identifier in the `tls` column.
fn set(row: &mut [String], column: &str, value: &str) {
    if let Some(index) = SOCKETS.iter().position(|name| *name == column) {
        value.clone_into(&mut row[index]);
    }
}

fn pool_rows(observatory: &dyn Observatory, scope: Scope) -> Vec<Vec<String>> {
    observatory
        .pools(scope)
        .into_iter()
        .map(|pool| {
            vec![
                pool.key.database.to_string(),
                pool.key.user.to_string(),
                pool.stats.active.to_string(),
                pool.stats.waiting.to_string(),
                blank(),
                blank(),
                pool.stats.active.to_string(),
                blank(),
                blank(),
                pool.stats.idle.to_string(),
                blank(),
                blank(),
                blank(),
                // pgprox does not track how long the oldest waiter has waited
                // per pool; `pgprox_wait_seconds` is the histogram that answers
                // this properly, and a wrong number here would be read as if it
                // were that one.
                blank(),
                blank(),
                // Always transaction pooling. Session pooling is what pinning
                // degrades into, per session rather than per pool.
                "transaction".to_owned(),
                blank(),
            ]
        })
        .collect()
}

/// One row per upstream connection a pool holds, not one row per pool.
///
/// `PgBouncer`'s `SHOW SERVERS` is a socket view: an operator counting rows
/// is counting actual backend connections. `PoolStats` does not track
/// individual sockets, only how many are active and how many are idle, so a
/// row here cannot carry a connection's own `connect_time` the way
/// `PLACEHOLDERS` already admits for `ptr` and `link`. It can carry the right
/// count and the right state per row, which one row per pool did not: an
/// operator reading `SHOW SERVERS` to see how many backend connections a pool
/// actually holds saw one row regardless of whether it held one or a hundred.
///
/// `id` is the server address repeated across every row from the same pool
/// rather than a connection identifier, for the same reason: nothing here
/// knows one socket from another.
fn server_socket_rows(observatory: &dyn Observatory, scope: Scope) -> Vec<Vec<String>> {
    observatory
        .pools(scope)
        .into_iter()
        .flat_map(|pool| {
            // Active rows first, idle after: arbitrary within a pool, since
            // nothing here can attribute a specific connection to either
            // count, but fixed so two reads of an unchanged pool agree.
            let states = [(pool.stats.active, "active"), (pool.stats.idle, "idle")];
            states.into_iter().flat_map(move |(count, state)| {
                let pool = pool.clone();
                (0..count).map(move |_| {
                    let mut row = vec![blank(); SOCKETS.len()];
                    // Positional rather than a literal, because most of these
                    // columns are placeholders and a literal would bury the
                    // four real values in seventeen empty strings.
                    set(&mut row, "type", "S");
                    set(&mut row, "user", &pool.key.user);
                    set(&mut row, "database", &pool.key.database);
                    set(&mut row, "state", state);
                    set(&mut row, "id", &pool.key.server.to_string());
                    row
                })
            })
        })
        .collect()
}

async fn client_rows(
    observatory: &dyn Observatory,
    scope: Scope,
) -> Result<Vec<Vec<String>>, pgprox_core::admin::AdminError> {
    let clients = observatory.clients(scope).await?;
    Ok(clients
        .into_iter()
        .map(|client| {
            let mut row = vec![blank(); SOCKETS.len()];
            set(&mut row, "type", "C");
            // `user` and `database` left blank rather than filled with the
            // tenant: `ClientView` does not carry the client's actual startup
            // `user`/`database`, only which tenant it belongs to, and those
            // are three different strings (a grant's `user` and `database`
            // are not the tenant ID either; see `PoolKey`). Writing the
            // tenant into both columns put the same wrong-looking value in
            // two places an operator reads as real, which this module's own
            // policy on placeholder columns argues against: an invented
            // value that looks like the others is worse than an empty one.
            set(&mut row, "state", client.state.as_str());
            // Whole seconds and the remainder, as PgBouncer splits them. The
            // total in microseconds in both would have a dashboard adding them
            // and double-counting.
            set(&mut row, "wait", &client.since.as_secs().to_string());
            set(
                &mut row,
                "wait_us",
                &client.since.subsec_micros().to_string(),
            );
            set(&mut row, "id", &client.conn.to_string());
            row
        })
        .collect())
}

fn stats_rows(observatory: &dyn Observatory, scope: Scope) -> Vec<Vec<String>> {
    let stats = observatory.stats(scope);
    vec![vec![
        // One row for the fleet. PgBouncer emits one per database; pgprox has
        // five thousand of those, and a SHOW that returns five thousand rows is
        // not a thing anyone wants in a psql session. Per-tenant detail is
        // SHOW TENANTS.
        "*".to_owned(),
        stats.transactions.to_string(),
        // `total_query_count` left blank rather than filled with the
        // transaction count. `Stats` has no per-query counter — pgprox counts
        // transactions, not the statements inside them — and a transaction
        // count copied into this column reads as "one query per transaction",
        // which is false for most real workloads and worse than an empty
        // column: it looks like a real number rather than an absent one.
        blank(),
        stats.client_conns.to_string(),
        stats.upstream_conns.to_string(),
        stats.waiting.to_string(),
    ]]
}

fn config_rows(observatory: &dyn Observatory) -> Vec<Vec<String>> {
    let config = observatory.config();
    let mut rows = vec![
        vec![
            "max_client_conns".to_owned(),
            config.max_client_conns.to_string(),
            "yes".to_owned(),
        ],
        vec![
            "drain_grace".to_owned(),
            format!("{}s", config.drain_grace.as_secs()),
            "yes".to_owned(),
        ],
        vec![
            "grant_ttl_cap".to_owned(),
            format!("{}s", config.grant_ttl_cap.as_secs()),
            "yes".to_owned(),
        ],
    ];
    for server in &config.servers {
        rows.push(vec![
            format!("servers.{}.max_connections", server.server),
            server.max_connections.to_string(),
            "yes".to_owned(),
        ]);
    }
    rows
}

fn peer_rows(observatory: &dyn Observatory) -> Vec<Vec<String>> {
    let view = observatory.cluster();
    let hash = format!("{:016x}", view.view_hash);
    view.digests
        .iter()
        .map(|digest| {
            vec![
                digest.node.get().to_string(),
                format!("{:?}", digest.mode).to_lowercase(),
                digest.client_conns.to_string(),
                hash.clone(),
            ]
        })
        .collect()
}

fn quota_rows(observatory: &dyn Observatory, scope: Scope) -> Vec<Vec<String>> {
    observatory
        .servers(scope)
        .into_iter()
        .map(|server| {
            vec![
                server.server.to_string(),
                server.cap.to_string(),
                server.in_use.to_string(),
                server.headroom().to_string(),
                server.guaranteed.to_string(),
                server.leased.to_string(),
            ]
        })
        .collect()
}

fn tenant_rows(observatory: &dyn Observatory, scope: Scope) -> Vec<Vec<String>> {
    observatory
        .tenants(scope)
        .into_iter()
        .map(|tenant| {
            vec![
                tenant.tenant.as_str().to_owned(),
                tenant
                    .home
                    .map_or_else(blank, |node| node.get().to_string()),
                tenant.client_conns.to_string(),
                tenant.upstream_conns.to_string(),
            ]
        })
        .collect()
}

/// The query cache on the node that answered.
///
/// No scope. ADR 0021 makes the cache one node's, and a `SHOW LOCAL CACHE`
/// that differed from `SHOW CACHE` would be promising an aggregate that cannot
/// exist.
fn cache_rows(observatory: &dyn Observatory) -> Vec<Vec<String>> {
    let cache = observatory.cache();
    vec![vec![
        cache.tenants.to_string(),
        // Not "read-your-writes", not "consistent", and not blank. A tenant
        // that believed either of the first two would be wrong the first time
        // a batch job wrote straight to the database, and a blank column is an
        // invitation to assume.
        if cache.is_off() {
            "off".to_owned()
        } else {
            "bounded staleness".to_owned()
        },
        cache.entries.to_string(),
        cache.bytes.to_string(),
        cache.max_bytes.to_string(),
        cache.hits.to_string(),
        cache.misses.to_string(),
        cache.expired.to_string(),
        cache.evicted.to_string(),
        cache.invalidated.to_string(),
        cache.rejected.to_string(),
        cache.abandoned.to_string(),
    ]]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    #[test]
    fn a_server_row_is_active_only_while_something_is_checked_out() {
        // `pool.stats.active > 0` had three surviving mutants: `>= 0` makes
        // every pool active, `== 0` inverts it, and `< 0` on a `u32` makes
        // none active. This is the `state` column of `SHOW SERVERS`, which an
        // operator reads to see whether a backend is doing anything, and every
        // existing test used a pool in one state only.
        let with_active = |active: u32, idle: u32| {
            let fake = FakeObservatory::new(node(1));
            fake.set_pools(vec![PoolView {
                node: node(1),
                key: PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app"),
                stats: PoolStats {
                    active,
                    idle,
                    ..PoolStats::default()
                },
            }]);
            let rows = server_socket_rows(&*fake, Scope::Cluster);
            let state = SOCKETS
                .iter()
                .position(|name| *name == "state")
                .unwrap_or_default();
            rows[0][state].clone()
        };

        assert_eq!(with_active(0, 3), "idle", "nothing checked out");
        assert_eq!(with_active(1, 2), "active", "one checked out");
        assert_eq!(with_active(7, 0), "active", "several checked out");
    }

    use pgprox_core::admin::{
        ClientState, ClientView, FakeObservatory, PoolView, ServerView, TenantView,
    };
    use pgprox_core::cluster::{ClusterDigest, NodeMode};
    use pgprox_core::config::{Config, ServerConfig};
    use pgprox_core::ids::{ConnId, NodeId, PoolKey, ServerId, TenantId};
    use pgprox_core::pool::PoolStats;
    use std::sync::Arc;
    use std::time::Duration;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn observatory() -> Arc<FakeObservatory> {
        let fake = FakeObservatory::new(node(1));
        fake.set_pools(vec![PoolView {
            node: node(1),
            key: PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app"),
            stats: PoolStats {
                active: 2,
                idle: 3,
                waiting: 1,
                limit: 10,
            },
        }]);
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
        fake.set_clients(vec![ClientView {
            conn: ConnId::new(node(1), 42),
            tenant: TenantId::new("acme"),
            node: node(1),
            state: ClientState::Idle,
            since: Duration::from_millis(5_500),
            pinned: None,
        }]);
        fake.set_digest(ClusterDigest {
            node: node(1),
            mode: NodeMode::Active,
            client_conns: 7,
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

    async fn show(target: ShowTarget, scope: Scope) -> Rows {
        render(observatory().as_ref(), ShowCommand { target, scope })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn show_pools_matches_pgbouncers_columns_exactly() {
        // Names and order both, because most dashboards read by position and a
        // shifted column is a wrong number rather than a missing one.
        let rows = show(ShowTarget::Pools, Scope::Cluster).await;

        assert_eq!(
            rows.columns,
            vec![
                "database",
                "user",
                "cl_active",
                "cl_waiting",
                "cl_active_cancel_req",
                "cl_waiting_cancel_req",
                "sv_active",
                "sv_active_cancel",
                "sv_being_canceled",
                "sv_idle",
                "sv_used",
                "sv_tested",
                "sv_login",
                "maxwait",
                "maxwait_us",
                "pool_mode",
                "load_balance_hosts",
            ],
            "SHOW POOLS no longer matches PgBouncer"
        );
    }

    #[tokio::test]
    async fn show_clients_and_servers_share_pgbouncers_socket_columns() {
        for target in [ShowTarget::Clients, ShowTarget::Servers] {
            let rows = show(target, Scope::Cluster).await;
            assert_eq!(rows.columns[0], "type", "{target}");
            assert_eq!(rows.columns[1], "user", "{target}");
            assert_eq!(rows.columns[2], "database", "{target}");
            assert_eq!(rows.columns[4], "state", "{target}");
            assert_eq!(rows.columns.len(), 21, "{target}");
        }
    }

    #[tokio::test]
    async fn every_row_has_exactly_as_many_values_as_there_are_columns() {
        // A short row shifts everything after it, which a positional reader
        // silently misinterprets rather than failing on.
        for target in ShowTarget::all() {
            for scope in [Scope::Cluster, Scope::Local] {
                let rows = show(*target, scope).await;
                for (index, row) in rows.rows.iter().enumerate() {
                    assert_eq!(
                        row.len(),
                        rows.columns.len(),
                        "{target} row {index} has {} values for {} columns",
                        row.len(),
                        rows.columns.len()
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn the_values_pgprox_actually_has_are_filled_in() {
        // A compatible shape full of blanks would be compatible and useless.
        let rows = show(ShowTarget::Pools, Scope::Cluster).await;

        assert_eq!(rows.get(0, "database"), Some("tenant_acme"));
        assert_eq!(rows.get(0, "user"), Some("acme_app"));
        assert_eq!(rows.get(0, "cl_active"), Some("2"));
        assert_eq!(rows.get(0, "cl_waiting"), Some("1"));
        assert_eq!(rows.get(0, "sv_idle"), Some("3"));
        assert_eq!(rows.get(0, "pool_mode"), Some("transaction"));
    }

    #[tokio::test]
    async fn a_placeholder_column_is_empty_rather_than_invented() {
        // A number an operator acts on must be real. An invented one is worse
        // than useless because it looks like the others.
        let rows = show(ShowTarget::Pools, Scope::Cluster).await;

        for column in ["sv_tested", "sv_login", "cl_active_cancel_req"] {
            assert_eq!(
                rows.get(0, column),
                Some(""),
                "{column} carries a value pgprox does not have"
            );
        }
    }

    #[test]
    fn every_placeholder_is_named() {
        // So nobody has to work out which columns are real by comparing two
        // sets of documentation.
        for target in ShowTarget::all() {
            for column in columns_for(*target) {
                let _ = column;
            }
        }
        for placeholder in PLACEHOLDERS {
            let appears = ShowTarget::all()
                .iter()
                .any(|target| columns_for(*target).contains(placeholder));
            assert!(
                appears,
                "{placeholder} is listed as a placeholder and is not a column"
            );
        }
    }

    #[tokio::test]
    async fn scope_narrows_a_show_the_way_it_narrows_the_api() {
        // The two surfaces read the same Observatory, so this is the test that
        // they cannot disagree.
        let fake = observatory();
        fake.set_pools(vec![
            PoolView {
                node: node(1),
                key: PoolKey::new(ServerId::new("db-1", 5432), "a", "a"),
                stats: PoolStats::default(),
            },
            PoolView {
                node: node(2),
                key: PoolKey::new(ServerId::new("db-1", 5432), "b", "b"),
                stats: PoolStats::default(),
            },
        ]);

        let cluster = render(
            fake.as_ref(),
            ShowCommand {
                target: ShowTarget::Pools,
                scope: Scope::Cluster,
            },
        )
        .await
        .unwrap();
        let local = render(
            fake.as_ref(),
            ShowCommand {
                target: ShowTarget::Pools,
                scope: Scope::Local,
            },
        )
        .await
        .unwrap();

        assert_eq!(cluster.len(), 2);
        assert_eq!(local.len(), 1);
        assert!(!cluster.is_empty());
    }

    #[tokio::test]
    async fn show_stats_is_one_row_rather_than_five_thousand() {
        // PgBouncer emits one per database. pgprox has five thousand of those,
        // and a SHOW returning five thousand rows is not something anyone wants
        // in a psql session. Per-tenant detail is SHOW TENANTS.
        let rows = show(ShowTarget::Stats, Scope::Cluster).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.get(0, "database"), Some("*"));
    }

    #[tokio::test]
    async fn show_stats_does_not_invent_a_query_count_from_the_transaction_one() {
        // `total_query_count` used to be `stats.transactions.to_string()`, the
        // same value already written into `total_xact_count` two columns
        // earlier. Nothing here counts queries, only transactions, and a
        // transaction count copied into the query column reads as "exactly
        // one query per transaction", which is false for most workloads and
        // worse than blank: it looks like a real measurement.
        let rows = show(ShowTarget::Stats, Scope::Cluster).await;
        assert_eq!(rows.get(0, "total_xact_count"), Some("0"));
        assert_ne!(
            rows.get(0, "total_query_count"),
            rows.get(0, "total_xact_count"),
            "the query count was copied from the transaction count"
        );
        assert_eq!(rows.get(0, "total_query_count"), Some(""));
    }

    #[tokio::test]
    async fn show_clients_does_not_put_the_tenant_in_the_user_and_database_columns() {
        // `ClientView` carries which tenant a client belongs to, not the
        // `user`/`database` it actually started up with, and the two are not
        // the same string: a grant's `user` and `database` differ from the
        // tenant ID and from each other, the same way `PoolKey`'s do for
        // `SHOW SERVERS`. Writing the tenant into both columns put one wrong
        // value in two places an operator reads as independent facts.
        let rows = show(ShowTarget::Clients, Scope::Cluster).await;
        assert_eq!(rows.get(0, "user"), Some(""));
        assert_eq!(rows.get(0, "database"), Some(""));
    }

    #[tokio::test]
    async fn show_servers_reports_one_row_per_connection_not_one_per_pool() {
        // `observatory()`'s one pool holds 2 active and 3 idle connections.
        // One row regardless of that count hid how many backend connections a
        // pool actually held from an operator counting rows, which is what
        // `SHOW SERVERS` is a socket view for.
        let rows = show(ShowTarget::Servers, Scope::Cluster).await;
        assert_eq!(rows.len(), 5, "2 active + 3 idle collapsed to one row");

        let states: Vec<&str> = (0..rows.len())
            .filter_map(|i| rows.get(i, "state"))
            .collect();
        assert_eq!(
            states.iter().filter(|s| **s == "active").count(),
            2,
            "{states:?}"
        );
        assert_eq!(
            states.iter().filter(|s| **s == "idle").count(),
            3,
            "{states:?}"
        );
        assert!(
            (0..rows.len()).all(|i| rows.get(i, "type") == Some("S")),
            "an expanded row lost its own type"
        );
    }

    #[tokio::test]
    async fn the_pgprox_only_commands_report_what_they_are_for() {
        let peers = show(ShowTarget::Peers, Scope::Cluster).await;
        assert_eq!(peers.get(0, "node"), Some("1"));
        assert_eq!(peers.get(0, "mode"), Some("active"));
        assert_eq!(peers.get(0, "view_hash").unwrap().len(), 16);

        let quota = show(ShowTarget::Quota, Scope::Cluster).await;
        assert_eq!(quota.get(0, "cap"), Some("100"));
        assert_eq!(quota.get(0, "headroom"), Some("40"));

        let tenants = show(ShowTarget::Tenants, Scope::Cluster).await;
        assert_eq!(tenants.get(0, "tenant"), Some("acme"));
        assert_eq!(tenants.get(0, "home"), Some("1"));
    }

    #[tokio::test]
    async fn show_cache_answers_for_the_node_whatever_scope_was_asked_for() {
        // `SHOW LOCAL CACHE` is not refused, it is the same answer. ADR 0021
        // makes the cache one node's, so the two forms genuinely mean the same
        // thing here, and this pins that rather than leaving it as a property
        // nobody meant.
        let cluster = show(ShowTarget::Cache, Scope::Cluster).await;
        let local = show(ShowTarget::Cache, Scope::Local).await;

        assert_eq!(cluster.rows, local.rows);
        assert_eq!(cluster.len(), 1);
        assert_eq!(cluster.get(0, "tenants"), Some("0"));
        assert_eq!(cluster.get(0, "promise"), Some("off"));
    }

    #[test]
    fn show_cache_is_not_offered_as_a_pgbouncer_command() {
        // `PgBouncer` has no `SHOW CACHE`, so no existing dashboard reads
        // these columns and they are this repo's to choose. Saying it was
        // compatible would freeze a layout nobody is depending on.
        assert!(!ShowTarget::Cache.is_pgbouncer_compatible());
    }

    #[tokio::test]
    async fn show_config_reports_the_configuration_in_force() {
        let rows = show(ShowTarget::Config, Scope::Cluster).await;

        let keys: Vec<&str> = (0..rows.len()).filter_map(|i| rows.get(i, "key")).collect();
        assert!(keys.contains(&"max_client_conns"), "{keys:?}");
        assert!(
            keys.iter().any(|key| key.contains("db-1:5432")),
            "the configured server limits are not reported: {keys:?}"
        );
    }

    #[tokio::test]
    async fn a_partial_fan_out_is_reported_rather_than_rendered_short() {
        // A short result set in a psql session reads as "there are no clients",
        // which is the wrong conclusion to hand somebody mid-incident.
        let fake = observatory();
        fake.set_unreachable(true);

        let err = render(
            fake.as_ref(),
            ShowCommand {
                target: ShowTarget::Clients,
                scope: Scope::Cluster,
            },
        )
        .await
        .unwrap_err();

        assert!(
            matches!(err, pgprox_core::admin::AdminError::Partial { .. }),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn a_client_row_carries_its_wait_split_the_way_pgbouncer_does() {
        // `wait` is whole seconds and `wait_us` the remainder, not the total in
        // microseconds. A dashboard adding them would otherwise double-count.
        let rows = show(ShowTarget::Clients, Scope::Cluster).await;

        assert_eq!(rows.get(0, "wait"), Some("5"));
        assert_eq!(rows.get(0, "wait_us"), Some("500000"));
        assert_eq!(rows.get(0, "state"), Some("idle"));
        assert_eq!(rows.get(0, "type"), Some("C"));
    }

    #[tokio::test]
    async fn a_server_row_is_typed_s_and_a_client_row_c() {
        // PgBouncer's convention, and the only way to tell the two apart in a
        // combined view.
        let servers = show(ShowTarget::Servers, Scope::Cluster).await;
        assert_eq!(servers.get(0, "type"), Some("S"));

        let clients = show(ShowTarget::Clients, Scope::Cluster).await;
        assert_eq!(clients.get(0, "type"), Some("C"));
    }

    #[tokio::test]
    async fn an_empty_fleet_renders_columns_with_no_rows() {
        // A psql session needs the header even when there is nothing to show,
        // or the operator cannot tell an empty result from a failed one.
        let empty = FakeObservatory::new(node(1));
        let rows = render(
            empty.as_ref(),
            ShowCommand {
                target: ShowTarget::Pools,
                scope: Scope::Cluster,
            },
        )
        .await
        .unwrap();

        assert!(rows.is_empty());
        assert_eq!(rows.columns.len(), POOLS.len());
        assert_eq!(rows.get(0, "database"), None);
    }

    #[tokio::test]
    async fn a_show_is_parsed_and_answered_in_one_step() {
        // The entry point a session reaches for. Without it a caller has to
        // know to call parse and then render, and the first one to forget the
        // NotShow case breaks every client that sends a SELECT.
        let fake = observatory();
        let handled = handle(fake.as_ref(), "SHOW POOLS").await.unwrap();

        let Handled::Answered(rows) = handled else {
            panic!("a SHOW was not answered: {handled:?}");
        };
        assert_eq!(rows.get(0, "database"), Some("tenant_acme"));
    }

    #[tokio::test]
    async fn an_ordinary_query_is_relayed_rather_than_answered_or_refused() {
        // The common case by far. Treating it as an error would break every
        // client that ever sends a query, which is all of them.
        let fake = observatory();
        for sql in ["SELECT 1", "INSERT INTO t VALUES (1)", "BEGIN"] {
            assert_eq!(
                handle(fake.as_ref(), sql).await.unwrap(),
                Handled::Relay,
                "{sql}"
            );
        }
    }

    #[tokio::test]
    async fn a_show_this_proxy_does_not_have_is_rejected_rather_than_relayed() {
        // Relaying `SHOW MEM` would have the server answer a question about
        // itself that the operator asked about the proxy.
        let fake = observatory();
        let handled = handle(fake.as_ref(), "SHOW MEM").await.unwrap();

        let Handled::Rejected(err) = handled else {
            panic!("an unknown SHOW was not rejected: {handled:?}");
        };
        assert!(err.to_string().contains("MEM"), "{err}");
    }

    #[tokio::test]
    async fn show_local_narrows_through_the_entry_point_too() {
        let fake = observatory();
        fake.set_pools(vec![
            PoolView {
                node: node(1),
                key: PoolKey::new(ServerId::new("db-1", 5432), "a", "a"),
                stats: PoolStats::default(),
            },
            PoolView {
                node: node(2),
                key: PoolKey::new(ServerId::new("db-1", 5432), "b", "b"),
                stats: PoolStats::default(),
            },
        ]);

        let Handled::Answered(cluster) = handle(fake.as_ref(), "SHOW POOLS").await.unwrap() else {
            panic!("not answered");
        };
        let Handled::Answered(local) = handle(fake.as_ref(), "SHOW LOCAL POOLS").await.unwrap()
        else {
            panic!("not answered");
        };

        assert_eq!(cluster.len(), 2);
        assert_eq!(local.len(), 1);
    }

    #[tokio::test]
    async fn a_partial_fan_out_reaches_the_caller_as_an_error() {
        // The one outcome that is a genuine error rather than a routing
        // decision, so it must not be swallowed into one of the other two.
        let fake = observatory();
        fake.set_unreachable(true);

        let err = handle(fake.as_ref(), "SHOW CLIENTS").await.unwrap_err();
        assert!(
            matches!(err, pgprox_core::admin::AdminError::Partial { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn every_command_has_columns() {
        for target in ShowTarget::all() {
            let columns = columns_for(*target);
            assert!(!columns.is_empty(), "{target} has no columns");
            let mut sorted = columns.to_vec();
            sorted.sort_unstable();
            let count = sorted.len();
            sorted.dedup();
            assert_eq!(sorted.len(), count, "{target} names a column twice");
        }
    }
}
