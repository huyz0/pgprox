//! Turning what the node knows into what Prometheus reads.
//!
//! # Read on scrape, not counted twice
//!
//! Every gauge here is answered from the live component at the moment of the
//! scrape: the pool knows how many connections it holds, the registry of
//! sessions knows how many clients there are, and the cluster layer knows what
//! it has leased. A second copy incremented at each call site would be a
//! second source of the same number, and the two would disagree the first time
//! an error path forgot to decrement.
//!
//! The counters are the exception, because a counter is a thing that happened
//! rather than a thing that is: transactions, pins and sheds are already
//! counted where they occur, by `Sessions`, and this reads those.
//!
//! # The metadata comes from the registry
//!
//! `pgprox-observe` declares every metric's name, kind, help text and labels,
//! and renders its own `HELP` and `TYPE` lines. Nothing here retypes a name.
//! That was M4.17's whole point, and an exporter that spelled the names again
//! would be the second source it exists to remove.
//!
//! # What has no source yet, said out loud
//!
//! Four of the registry's metrics have nothing to read: the two histograms
//! need instrumentation on the wait and query paths, the auth cache counter
//! lives inside `pgprox-auth` behind a trait with no accessor, and replica lag
//! needs the primary's position at the same instant as the replica's. They are
//! declared in the output with their help and type and no samples, which is
//! what an absent series should look like, and [`UNSOURCED`] names them so a
//! fifth cannot quietly join them.

use std::fmt::Write as _;

use pgprox_core::admin::{Observatory, Scope};
use pgprox_core::ids::NodeId;
use pgprox_observe::metrics::{self, Metric};
use pgprox_observe::tenants::TenantAllowlist;

/// The metrics the registry declares that nothing can yet answer.
///
/// A list rather than an omission: a metric that is exported by nobody and
/// named by nobody is one an operator builds a dashboard panel on and waits
/// for.
pub const UNSOURCED: &[&str] = &[
    // Needs a timer around pool acquire.
    "pgprox_wait_seconds",
    // Needs a timer around the relay's transaction boundary.
    "pgprox_query_duration_seconds",
    // Lives in pgprox-auth's cache, behind CredentialResolver, which has no
    // accessor for it and should not grow one for this alone.
    "pgprox_auth_cache_total",
    // Needs the primary's LSN at the same instant as the replica's, which is
    // the write watermark's problem and not the exporter's.
    "pgprox_replica_lag_bytes",
];

/// Renders the node's metrics in the Prometheus text format.
///
/// Every metric the registry declares appears, so a scrape is a complete
/// picture of what exists rather than of what happened to have a value.
pub async fn render(
    observatory: &dyn Observatory,
    node: NodeId,
    tenants: &TenantAllowlist,
    slab: &pgprox_core::buf::BufferSlab,
    routes: &crate::routes::RouteCounts,
) -> String {
    let node = node.get().to_string();
    // The one read that fans out is asked for locally, because a scrape is of
    // this node: Prometheus scrapes every pod, and a cluster-scoped answer
    // would count every client once per pod.
    let clients = observatory.clients(Scope::Local).await.unwrap_or_default();
    let sources = Sources {
        observatory,
        clients: &clients,
        node: &node,
        tenants,
        slab,
        routes,
    };
    let mut out = String::with_capacity(4096);

    for metric in metrics::ALL {
        out.push_str(&metric.describe());
        samples(&mut out, metric, &sources);
    }
    out
}

/// Clients grouped by state and tenant label together, one count per pair.
///
/// One grouping rather than two, which is what `pgprox_client_conns` used to
/// be: a state-only breakdown and a tenant-only breakdown emitted as
/// separate sets of samples under the same metric name. Both were the full
/// client count, sliced two different ways, so `sum(pgprox_client_conns)`
/// with no label filter counted every client twice. A joint breakdown counts
/// each client in exactly one cell, which is what makes the metric's own
/// total meaningful again.
///
/// The tenant half of the key is the allowlist's label, not the tenant
/// itself: everything outside it collapses into `OTHER`, which is the
/// cardinality bound `pgprox-observe`'s registry checks `TENANT` against.
///
/// A `BTreeMap` rather than the fixed three-element array `M17.4` used for
/// state alone: that shape hardened `+= 1` against becoming `*= 1` or `-= 1`
/// by making every arm's count independently observable, and a map keyed on
/// both dimensions keeps that property — each `(state, tenant)` cell is one
/// counter nothing else can be confused with.
fn count_by_state_and_tenant<'a>(
    clients: &'a [pgprox_core::admin::ClientView],
    tenants: &'a TenantAllowlist,
) -> std::collections::BTreeMap<(&'static str, &'a str), u32> {
    let mut counts: std::collections::BTreeMap<(&'static str, &'a str), u32> =
        std::collections::BTreeMap::new();
    for view in clients {
        *counts
            .entry((view.state.as_str(), tenants.label_for(&view.tenant)))
            .or_default() += 1;
    }
    counts
}

fn client_samples(
    out: &mut String,
    metric: &Metric,
    clients: &[pgprox_core::admin::ClientView],
    node: &str,
    tenants: &TenantAllowlist,
) {
    for ((state, tenant), count) in count_by_state_and_tenant(clients, tenants) {
        let _ = writeln!(
            out,
            "{}{{node=\"{node}\",state=\"{state}\",tenant=\"{tenant}\"}} {count}",
            metric.name
        );
    }
}

/// Upstream connections, by server and state.
///
/// The registry declares `pgprox_upstream_conns` with a `state` label —
/// `"Upstream connections held by this node, by server and state"` — and the
/// exporter used to fold `active + idle` into one number with no `state`
/// label at all, so a rising count could not say whether the pool was busy
/// or merely holding warm connections it did not need.
///
/// One pool per `(server, database, user)` key, and one server can have
/// several. Summed per server here rather than emitted per pool, which fixes
/// a second thing the per-pool version did wrong: two pools on the same
/// server produced two samples with an identical label set, which is not
/// valid Prometheus exposition (Prometheus requires one sample per distinct
/// label combination). Grouping by server first is what a `server`-labelled
/// gauge is supposed to mean anyway.
fn upstream_samples(out: &mut String, metric: &Metric, observatory: &dyn Observatory, node: &str) {
    let mut totals: std::collections::BTreeMap<String, (u32, u32)> =
        std::collections::BTreeMap::new();
    for pool in observatory.pools(Scope::Local) {
        let entry = totals.entry(pool.key.server.to_string()).or_default();
        entry.0 += pool.stats.active;
        entry.1 += pool.stats.idle;
    }
    for (server, (active, idle)) in totals {
        for (state, count) in [("active", active), ("idle", idle)] {
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\",server=\"{server}\",state=\"{state}\"}} {count}",
                metric.name
            );
        }
    }
}

/// Statements routed, by where they went.
fn route_samples(
    out: &mut String,
    metric: &Metric,
    node: &str,
    routes: &crate::routes::RouteCounts,
) {
    for (route, count) in [
        ("primary", routes.primary()),
        ("replica", routes.replica()),
        ("cache", routes.cache()),
    ] {
        let _ = writeln!(
            out,
            "{}{{node=\"{node}\",route=\"{route}\"}} {count}",
            metric.name
        );
    }
}

/// The node's buffer slab, in the three numbers that describe it.
///
/// The bound is a sample rather than a comment, because "47 outstanding" means
/// nothing without it and an operator should not have to find the config to
/// read the graph.
fn slab_samples(
    out: &mut String,
    metric: &Metric,
    node: &str,
    slab: &pgprox_core::buf::BufferSlab,
) {
    for (state, count) in [
        ("outstanding", slab.outstanding()),
        ("idle", slab.idle()),
        ("bound", slab.capacity()),
    ] {
        let _ = writeln!(
            out,
            "{}{{node=\"{node}\",state=\"{state}\"}} {count}",
            metric.name
        );
    }
}

/// The query cache, in the four metrics that describe it.
///
/// One function for all four because they read one view: two scrapes of the
/// same node that disagreed about whether its cache was on would be worse than
/// either answer.
fn cache_samples(
    out: &mut String,
    metric: &Metric,
    node: &str,
    cache: &pgprox_core::admin::CacheView,
) {
    match metric.name {
        // The one that says whether the cache is on. Emitted on every node
        // including the ones where it is off, because a series that is absent
        // and a series that is zero are different facts to an alert, and only
        // the second is what a node with no `query_cache` section means.
        "pgprox_cache_tenants" => {
            let _ = writeln!(out, "{}{{node=\"{node}\"}} {}", metric.name, cache.tenants);
        }
        "pgprox_cache_entries" => {
            let _ = writeln!(out, "{}{{node=\"{node}\"}} {}", metric.name, cache.entries);
        }
        // The budget beside the usage, the way the slab reports its bound: an
        // alert on bytes held would otherwise have to hard-code the configured
        // number.
        "pgprox_cache_bytes" => {
            for (state, count) in [("used", cache.bytes), ("bound", cache.max_bytes)] {
                let _ = writeln!(
                    out,
                    "{}{{node=\"{node}\",state=\"{state}\"}} {count}",
                    metric.name
                );
            }
        }
        "pgprox_cache_total" => {
            for (result, count) in [
                ("hit", cache.hits),
                ("miss", cache.misses),
                ("expired", cache.expired),
                ("evicted", cache.evicted),
                ("invalidated", cache.invalidated),
                ("rejected", cache.rejected),
                ("abandoned", cache.abandoned),
            ] {
                let _ = writeln!(
                    out,
                    "{}{{node=\"{node}\",result=\"{result}\"}} {count}",
                    metric.name
                );
            }
        }
        _ => {}
    }
}

/// The samples for one metric, or none where nothing can answer it.
/// What every metric needs to answer for itself.
///
/// A struct rather than eight parameters: the list grew one at a time and each
/// addition was reasonable, which is how a signature ends up unreadable.
struct Sources<'a> {
    observatory: &'a dyn Observatory,
    clients: &'a [pgprox_core::admin::ClientView],
    node: &'a str,
    tenants: &'a TenantAllowlist,
    slab: &'a pgprox_core::buf::BufferSlab,
    routes: &'a crate::routes::RouteCounts,
}

fn samples(out: &mut String, metric: &Metric, from: &Sources<'_>) {
    let Sources {
        observatory,
        clients,
        node,
        tenants,
        slab,
        routes,
    } = *from;

    match metric.name {
        "pgprox_buffer_slab" => slab_samples(out, metric, node, slab),
        "pgprox_route_total" => route_samples(out, metric, node, routes),
        "pgprox_client_conns" => client_samples(out, metric, clients, node, tenants),
        "pgprox_upstream_conns" => upstream_samples(out, metric, observatory, node),
        "pgprox_quota_leased" => {
            for server in observatory.servers(Scope::Local) {
                let _ = writeln!(
                    out,
                    "{}{{node=\"{node}\",server=\"{}\"}} {}",
                    metric.name, server.server, server.leased
                );
            }
        }
        "pgprox_pin_total" => {
            // The reason label needs per-reason counters, which `Sessions`
            // does not keep: it records the reason on the client and counts
            // pins in total. `unknown` is honest about that, and the label
            // stays present so a dashboard grouping by it keeps working when
            // the per-reason counters arrive.
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\",reason=\"unknown\"}} {}",
                metric.name,
                observatory.stats(Scope::Local).pins
            );
        }
        "pgprox_shed_total" => {
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\",reason=\"unknown\"}} {}",
                metric.name,
                observatory.stats(Scope::Local).sheds
            );
        }
        "pgprox_cluster_members" => {
            let cluster = observatory.cluster();
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\"}} {}",
                metric.name,
                cluster.membership.active_count()
            );
        }
        "pgprox_cluster_view_hash" => {
            // The hash as a number, because two pods disagreeing is the thing
            // being watched for and a difference is visible in any format.
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\"}} {}",
                metric.name,
                observatory.cluster().view_hash
            );
        }
        name if name.starts_with("pgprox_cache_") => {
            cache_samples(out, metric, node, &observatory.cache());
        }
        "pgprox_config_reload_total" => {
            // Every scrape reports the config the node is serving, which is
            // what an operator compares across pods after an edit. The count
            // of reloads is not kept anywhere yet; the generation is what
            // matters and this is the honest version of it.
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\",result=\"current\"}} {}",
                metric.name,
                observatory.config().max_client_conns
            );
            // And whether the last read worked. A node serving a stale
            // document looks exactly like one serving the current document,
            // and this is the difference an alert can be written against.
            let _ = writeln!(
                out,
                "{}{{node=\"{node}\",result=\"stale\"}} {}",
                metric.name,
                u32::from(!observatory.config_is_current())
            );
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {

    /// A client view with only the two fields these counters read.
    fn client(
        tenant: &str,
        state: pgprox_core::admin::ClientState,
    ) -> pgprox_core::admin::ClientView {
        pgprox_core::admin::ClientView {
            conn: pgprox_core::ids::ConnId::new(NodeId::new(1), 7),
            tenant: pgprox_core::ids::TenantId::new(tenant),
            node: NodeId::new(1),
            state,
            since: std::time::Duration::from_secs(1),
            pinned: None,
        }
    }

    #[test]
    fn clients_are_counted_by_state_and_tenant_together() {
        // `M17.4`'s hardening carried forward: every `+= 1` here could become
        // `*= 1` or `-= 1` and a test that only read the writer's text would
        // not notice. Each `(state, tenant)` cell below is a distinct,
        // directly observed count, so no arm can be deleted or miscounted
        // without one of these numbers moving.
        use pgprox_core::admin::{ClientState, ClientView};

        fn view(tenant: &str, state: ClientState) -> ClientView {
            client(tenant, state)
        }

        let allowed = TenantAllowlist::from_configured([pgprox_core::ids::TenantId::new("acme")])
            .expect("a one-tenant allowlist is within the cap");

        let clients = vec![
            view("acme", ClientState::Active),
            view("acme", ClientState::Active),
            view("acme", ClientState::Idle),
            view("other-tenant", ClientState::Idle),
            view("another-tenant", ClientState::Idle),
            view("acme", ClientState::Waiting),
        ];

        let counted = count_by_state_and_tenant(&clients, &allowed);

        assert_eq!(counted.get(&("active", "acme")).copied(), Some(2));
        assert_eq!(counted.get(&("idle", "acme")).copied(), Some(1));
        assert_eq!(counted.get(&("waiting", "acme")).copied(), Some(1));
        // Two different unlisted tenants, both idle, collapse into one cell
        // rather than becoming two series: the cardinality bound this counts
        // labels rather than tenants to get.
        assert_eq!(
            counted
                .get(&("idle", pgprox_observe::tenants::OTHER))
                .copied(),
            Some(2),
            "two unlisted tenants became two series: {counted:?}"
        );
        // No cell for a state nobody was in, and no accidental cross terms:
        // the total over every cell is the client count and nothing more.
        assert_eq!(
            counted.values().sum::<u32>(),
            u32::try_from(clients.len()).unwrap()
        );
        assert_eq!(count_by_state_and_tenant(&[], &allowed).len(), 0);
    }

    /// A slab for the exporter's tests, with a bound the assertions can name.
    fn test_slab() -> std::sync::Arc<pgprox_core::buf::BufferSlab> {
        pgprox_core::buf::BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8)
    }
    use super::*;
    use pgprox_core::admin::FakeObservatory;
    use pgprox_core::ids::TenantId;
    use std::sync::Arc;

    /// An observatory with one of everything, so a metric whose samples come
    /// from a list has a list to come from.
    fn seeded() -> Arc<FakeObservatory> {
        use pgprox_core::admin::{ClientState, ClientView, PoolView, ServerView};
        use pgprox_core::ids::{PoolKey, ServerId};
        use pgprox_core::pool::PoolStats;

        let observatory = FakeObservatory::new(NodeId::new(1));
        let server = ServerId::new("db-1", 5432);
        observatory.set_pools(vec![PoolView {
            node: NodeId::new(1),
            key: PoolKey::new(server.clone(), "acme", "acme_app"),
            stats: PoolStats {
                active: 2,
                idle: 3,
                ..PoolStats::default()
            },
        }]);
        observatory.set_servers(vec![ServerView {
            server,
            cap: 100,
            in_use: 5,
            guaranteed: 16,
            leased: 4,
        }]);
        observatory.set_clients(vec![ClientView {
            conn: pgprox_core::ids::ConnId::new(NodeId::new(1), 7),
            tenant: TenantId::new("acme"),
            node: NodeId::new(1),
            state: ClientState::Active,
            since: std::time::Duration::from_secs(1),
            pinned: None,
        }]);
        observatory
    }

    async fn rendered() -> String {
        render(
            seeded().as_ref(),
            NodeId::new(1),
            &TenantAllowlist::new(),
            &test_slab(),
            &crate::routes::RouteCounts::new(),
        )
        .await
    }

    async fn rendered_allowing(tenant: &str) -> String {
        let mut allowlist = TenantAllowlist::new();
        allowlist.add(TenantId::new(tenant)).unwrap();
        render(
            seeded().as_ref(),
            NodeId::new(1),
            &allowlist,
            &test_slab(),
            &crate::routes::RouteCounts::new(),
        )
        .await
    }

    #[tokio::test]
    async fn every_declared_metric_appears() {
        // A scrape is a complete picture of what exists, so a dashboard panel
        // for a metric with no samples reads as zero series rather than as a
        // typo in the panel.
        let out = rendered().await;
        for metric in metrics::ALL {
            assert!(
                out.contains(&format!("# HELP {} ", metric.name)),
                "{} is declared and not exported",
                metric.name
            );
        }
    }

    #[tokio::test]
    async fn every_metric_has_samples_or_is_named_as_having_no_source() {
        // The rule that stops the unsourced list growing quietly: a metric
        // that stops being answered has to be added to it on purpose.
        let out = rendered().await;
        for metric in metrics::ALL {
            let sampled = out
                .lines()
                .any(|line| line.starts_with(metric.name) && !line.starts_with('#'));
            let excused = UNSOURCED.contains(&metric.name);

            assert!(
                sampled != excused,
                "{}: sampled={sampled}, listed as unsourced={excused}",
                metric.name
            );
        }
    }

    #[tokio::test]
    async fn a_cache_that_is_off_is_visible_in_a_scrape_as_zero_rather_than_absent() {
        // ADR 0021's acceptance, at the surface an alert reads. A series that
        // is absent and a series that is zero are different facts, and a node
        // with no `query_cache` section means the second one: the cache exists
        // and serves nobody.
        let out = rendered().await;
        assert!(
            out.contains("pgprox_cache_tenants{node=\"1\"} 0"),
            "a node with the cache off did not say so: {out}"
        );
        assert!(
            out.contains("pgprox_cache_total{node=\"1\",result=\"hit\"} 0"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn a_cache_doing_nothing_is_distinguishable_from_one_that_is_off() {
        // The half of the acceptance the counters cannot meet. Both nodes
        // report zero hits, zero entries and zero bytes held; only the tenant
        // count differs, which is why it is a metric of its own.
        let observatory = seeded();
        observatory.set_cache(pgprox_core::admin::CacheView {
            tenants: 3,
            max_bytes: 64 * 1024 * 1024,
            ..pgprox_core::admin::CacheView::default()
        });
        let idle = render(
            observatory.as_ref(),
            NodeId::new(1),
            &TenantAllowlist::new(),
            &test_slab(),
            &crate::routes::RouteCounts::new(),
        )
        .await;

        assert!(
            idle.contains("pgprox_cache_tenants{node=\"1\"} 3"),
            "{idle}"
        );
        assert!(
            idle.contains("pgprox_cache_bytes{node=\"1\",state=\"bound\"} 67108864"),
            "the budget is not exported beside the usage: {idle}"
        );

        // And every counter is still zero, which is the point: an operator
        // reading only those two scrapes could not have told them apart.
        for result in [
            "hit",
            "miss",
            "expired",
            "evicted",
            "invalidated",
            "rejected",
            // `M25.1`. Apart from `rejected` because the remedies differ: one
            // says the budget is too small for a result, the other says the
            // results are too big for a cache to be the right tool.
            "abandoned",
        ] {
            let sample = format!("pgprox_cache_total{{node=\"1\",result=\"{result}\"}} 0");
            assert!(idle.contains(&sample), "missing {sample} in {idle}");
        }
    }

    #[tokio::test]
    async fn a_stale_configuration_is_visible_in_a_scrape() {
        // A node serving the last good document looks exactly like one serving
        // the current one, and this is the difference an alert is written
        // against.
        let out = rendered().await;

        assert!(
            out.contains("pgprox_config_reload_total{node=\"1\",result=\"stale\"} 0"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn pgprox_client_conns_does_not_double_count_a_bare_sum() {
        // The bug this whole rework exists for: a state-only breakdown and a
        // tenant-only breakdown, both the same total, emitted as two
        // overlapping sets of samples under one metric name. `sum()` with no
        // label filter — the query anyone writes first — counted every
        // client twice. `seeded()` has exactly one client, so the sum of
        // every `pgprox_client_conns` sample must be exactly one.
        let out = rendered().await;

        let total: u32 = out
            .lines()
            .filter(|line| line.starts_with("pgprox_client_conns{"))
            .filter_map(|line| line.rsplit(' ').next())
            .filter_map(|value| value.parse::<u32>().ok())
            .sum();

        assert_eq!(
            total, 1,
            "a bare sum over the metric was not the client count: {out}"
        );
    }

    #[tokio::test]
    async fn pgprox_upstream_conns_carries_the_state_label() {
        // The registry declares this metric "by server and state"; the
        // exporter used to fold active and idle into one number with no
        // `state` label at all, so a rising count could not say whether a
        // pool was busy or merely holding warm connections. `seeded()`'s
        // pool has 2 active and 3 idle.
        let out = rendered().await;

        assert!(
            out.contains(
                "pgprox_upstream_conns{node=\"1\",server=\"db-1:5432\",state=\"active\"} 2"
            ),
            "{out}"
        );
        assert!(
            out.contains("pgprox_upstream_conns{node=\"1\",server=\"db-1:5432\",state=\"idle\"} 3"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn a_tenant_off_the_allowlist_is_aggregated() {
        // The unbounded label the registry's own cardinality test names as the
        // example: one series per tenant is five thousand series per node.
        let out = rendered().await;

        assert!(out.contains("tenant=\"other\""), "{out}");
        assert!(!out.contains("tenant=\"acme\""), "{out}");
    }

    #[tokio::test]
    async fn a_tenant_on_the_allowlist_gets_its_own_series() {
        let out = rendered_allowing("acme").await;

        assert!(out.contains("tenant=\"acme\""), "{out}");
    }

    #[tokio::test]
    async fn every_sample_carries_the_node_label() {
        // Aggregates are meaningless without it and split brain is invisible
        // without it.
        for line in rendered()
            .await
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            assert!(line.contains("node=\"1\""), "{line}");
        }
    }

    #[tokio::test]
    async fn nothing_in_the_output_is_a_credential() {
        // The DTOs have no field for one, so this is a rule about what the
        // exporter renders rather than about what it is given. It holds by
        // construction and is asserted because that is cheap.
        let out = rendered().await.to_lowercase();
        for word in ["password", "token", "secret", "authorization"] {
            assert!(!out.contains(word), "{word} reached the metrics output");
        }
    }

    #[test]
    fn the_unsourced_list_names_only_metrics_that_exist() {
        // A stale entry would excuse a metric nobody exports under a name
        // nobody declares.
        for name in UNSOURCED {
            assert!(
                metrics::ALL.iter().any(|metric| metric.name == *name),
                "{name} is excused and not declared"
            );
        }
    }
}
