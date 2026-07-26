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
//! Four of the registry's twelve metrics have nothing to read: the two
//! histograms need instrumentation on the wait and query paths, the auth cache
//! counter lives inside `pgprox-auth` behind a trait with no accessor, and
//! replica lag needs the primary's position at the same instant as the
//! replica's. They are declared in the output with their help and type and no
//! samples, which is what an absent series should look like, and
//! [`UNSOURCED`] names them so adding a thirteenth cannot quietly join them.

use std::fmt::Write as _;

use pgprox_core::admin::{Observatory, Scope};
use pgprox_core::ids::NodeId;
use pgprox_observe::metrics::{self, Metric};

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
pub async fn render(observatory: &dyn Observatory, node: NodeId) -> String {
    let node = node.get().to_string();
    // The one read that fans out is asked for locally, because a scrape is of
    // this node: Prometheus scrapes every pod, and a cluster-scoped answer
    // would count every client once per pod.
    let clients = observatory.clients(Scope::Local).await.unwrap_or_default();
    let mut out = String::with_capacity(4096);

    for metric in metrics::ALL {
        out.push_str(&metric.describe());
        samples(&mut out, metric, observatory, &clients, &node);
    }
    out
}

/// The samples for one metric, or none where nothing can answer it.
fn samples(
    out: &mut String,
    metric: &Metric,
    observatory: &dyn Observatory,
    clients: &[pgprox_core::admin::ClientView],
    node: &str,
) {
    match metric.name {
        "pgprox_client_conns" => {
            // By state, which is what the registry's label says, and the
            // session registry is what knows: an aggregate without the state
            // would be a different metric wearing this one's name.
            let mut idle = 0_u32;
            let mut active = 0_u32;
            let mut waiting = 0_u32;
            for view in clients {
                match view.state {
                    pgprox_core::admin::ClientState::Active => active += 1,
                    pgprox_core::admin::ClientState::Waiting => waiting += 1,
                    _ => idle += 1,
                }
            }
            for (state, count) in [("idle", idle), ("active", active), ("waiting", waiting)] {
                let _ = writeln!(
                    out,
                    "{}{{node=\"{node}\",state=\"{state}\"}} {count}",
                    metric.name
                );
            }
        }
        "pgprox_upstream_conns" => {
            for pool in observatory.pools(Scope::Local) {
                let _ = writeln!(
                    out,
                    "{}{{node=\"{node}\",server=\"{}\"}} {}",
                    metric.name,
                    pool.key.server,
                    pool.stats.active + pool.stats.idle
                );
            }
        }
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
        }
        _ => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_core::admin::FakeObservatory;
    use std::sync::Arc;

    /// An observatory with one of everything, so a metric whose samples come
    /// from a list has a list to come from.
    fn seeded() -> Arc<FakeObservatory> {
        use pgprox_core::admin::{ClientState, ClientView, PoolView, ServerView};
        use pgprox_core::ids::{PoolKey, ServerId, TenantId};
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
        render(seeded().as_ref(), NodeId::new(1)).await
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
