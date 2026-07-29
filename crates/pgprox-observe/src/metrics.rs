//! Every metric the proxy exports, declared in one place.
//!
//! # Why a registry rather than call sites
//!
//! A metric declared where it is incremented is a metric nobody can enumerate.
//! Nobody can answer "what do we export", the dashboards drift from the code,
//! and the one rule that actually matters here cannot be checked at all.
//!
//! That rule is cardinality. A `tenant` label at five thousand tenants
//! multiplied by the other labels is a series count that takes a Prometheus
//! down, and it takes it down at exactly the moment somebody is trying to work
//! out why the proxy is unhappy. Declaring every metric here means
//! [`Metric::labels`] is enumerable, so
//! `no_metric_has_an_unbounded_label` is a test rather than a code review
//! someone might be having a bad day during.
//!
//! Per-tenant detail is not lost, it lives in the admin API and `SHOW` output,
//! which are pull-based and cost nothing when nobody is looking. See ADR 0007.
//!
//! # Every metric carries `node`
//!
//! Aggregates are meaningless without it and split brain is invisible without
//! it. `pgprox_cluster_view_hash` exists for that second reason specifically: a
//! mismatch between two pods surfaces directly rather than being inferred from
//! two membership lists that look similar.

use std::fmt;

/// What a metric measures, which decides how it is exported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Kind {
    /// A value that goes up and down.
    Gauge,
    /// A value that only goes up, reset on restart.
    Counter,
    /// A distribution, exported with buckets.
    Histogram,
}

impl Kind {
    /// The name Prometheus uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gauge => "gauge",
            Self::Counter => "counter",
            Self::Histogram => "histogram",
        }
    }
}

/// A label, and the reason its values are bounded.
///
/// The reason is not documentation. It is the thing a reviewer checks, and
/// having to write one is what makes adding `tenant` feel wrong rather than
/// convenient.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Label {
    /// The label name.
    pub name: &'static str,
    /// Roughly how many distinct values it can take in a real deployment.
    ///
    /// An estimate, and deliberately conservative. It exists to be compared
    /// against [`MAX_LABEL_VALUES`], not to be accurate.
    pub cardinality: u32,
    /// Why it cannot grow beyond that.
    pub bounded_because: &'static str,
}

/// The most values a single label may take.
///
/// Deliberately small. A label with a hundred values multiplied by two others
/// is already thousands of series per metric per node, and the metrics that
/// matter here are the ones an operator reads during an incident, which is
/// exactly when a slow Prometheus is worst.
pub const MAX_LABEL_VALUES: u32 = 64;

/// One exported metric.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metric {
    /// The full name, including the `pgprox_` prefix.
    pub name: &'static str,
    /// What it measures.
    pub kind: Kind,
    /// What it means, for the `HELP` line.
    pub help: &'static str,
    /// Its labels, `node` excluded: every metric has that one.
    pub labels: &'static [Label],
}

impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// The label every metric carries.
///
/// Aggregates are meaningless without it, and a split brain is invisible
/// without it. Bounded by the deployment: a handful of pods, not a handful of
/// thousands.
pub const NODE: Label = Label {
    name: "node",
    cardinality: 16,
    bounded_because: "the number of proxy pods, which is a deployment decision",
};

const STATE: Label = Label {
    name: "state",
    cardinality: 4,
    bounded_because: "the fixed set of connection states",
};

const SERVER: Label = Label {
    name: "server",
    cardinality: 32,
    bounded_because: "configured upstream servers, one entry each in the config document",
};

const REASON: Label = Label {
    name: "reason",
    cardinality: 16,
    bounded_because: "the enumerated pin and shed reasons, which are code, not data",
};

const ROUTE: Label = Label {
    name: "route",
    cardinality: 4,
    bounded_because: "primary, the query cache, or one of a handful of replicas",
};

const RESULT: Label = Label {
    name: "result",
    cardinality: 8,
    bounded_because: "the enumerated outcomes of a lookup, which are code, not data: \
                      hit, miss, expired, evicted, invalidated, rejected",
};

const REPLICA: Label = Label {
    name: "replica",
    cardinality: 8,
    bounded_because: "replicas of one upstream, listed in the grant",
};

/// Client connections, by state.
pub const CLIENT_CONNS: Metric = Metric {
    name: "pgprox_client_conns",
    kind: Kind::Gauge,
    help: "Client connections held by this node, by state",
    labels: &[STATE],
};

/// Upstream connections, by server and state.
pub const UPSTREAM_CONNS: Metric = Metric {
    name: "pgprox_upstream_conns",
    kind: Kind::Gauge,
    help: "Upstream connections held by this node, by server and state",
    labels: &[SERVER, STATE],
};

/// Quota leased beyond the guaranteed share.
pub const QUOTA_LEASED: Metric = Metric {
    name: "pgprox_quota_leased",
    kind: Kind::Gauge,
    help: "Connections this node holds on lease beyond its guaranteed share",
    labels: &[SERVER],
};

/// Time spent blocked acquiring an upstream connection.
///
/// The single most important latency signal in the proxy. Everything else can
/// look healthy while this climbs, and when it climbs, clients are waiting.
pub const WAIT_SECONDS: Metric = Metric {
    name: "pgprox_wait_seconds",
    kind: Kind::Histogram,
    help: "Time a client spent waiting for an upstream connection",
    labels: &[SERVER],
};

/// Query duration, by where it was routed.
pub const QUERY_DURATION_SECONDS: Metric = Metric {
    name: "pgprox_query_duration_seconds",
    kind: Kind::Histogram,
    help: "Time from a client's query to its ReadyForQuery, by route",
    labels: &[ROUTE],
};

/// Sessions pinned, by reason.
///
/// A rising pin rate is the early warning that multiplexing is degrading, and
/// the reason says which feature to go and look at.
pub const PIN_TOTAL: Metric = Metric {
    name: "pgprox_pin_total",
    kind: Kind::Counter,
    help: "Sessions pinned to one upstream connection, by reason",
    labels: &[REASON],
};

/// Clients shed, by reason.
pub const SHED_TOTAL: Metric = Metric {
    name: "pgprox_shed_total",
    kind: Kind::Counter,
    help: "Clients closed so they reconnect elsewhere, by reason",
    labels: &[REASON],
};

/// Grant cache outcomes.
///
/// The plan spells this `pgprox_auth_cache`. Renamed to end in `_total` because
/// it is a counter, and Prometheus tooling keys off that suffix to offer `rate`
/// and to reject it where a gauge is meant. A metric that reads as a gauge to
/// the tooling is one somebody will average.
pub const AUTH_CACHE: Metric = Metric {
    name: "pgprox_auth_cache_total",
    kind: Kind::Counter,
    help: "Grant cache lookups, by outcome",
    labels: &[RESULT],
};

/// Replica lag, in bytes of WAL.
pub const REPLICA_LAG_BYTES: Metric = Metric {
    name: "pgprox_replica_lag_bytes",
    kind: Kind::Gauge,
    help: "WAL bytes a replica is behind the primary",
    labels: &[REPLICA],
};

/// Live cluster members.
pub const CLUSTER_MEMBERS: Metric = Metric {
    name: "pgprox_cluster_members",
    kind: Kind::Gauge,
    help: "Nodes this node currently considers alive",
    labels: &[],
};

/// The membership view hash.
///
/// Exported so a mismatch across pods surfaces split brain directly rather than
/// being inferred from two membership lists that look similar.
pub const CLUSTER_VIEW_HASH: Metric = Metric {
    name: "pgprox_cluster_view_hash",
    kind: Kind::Gauge,
    help: "Hash of this node's membership view; a mismatch across pods is split brain",
    labels: &[],
};

/// Statements routed, by where they went.
///
/// The share a replica served, which nothing could answer before: a pool shows
/// connections rather than statements, and a replica pool at zero could mean
/// the router never chose one or that it chose one and the connection was
/// already warm. A read that a session's own write watermark sends to the
/// primary is correct and shows up here as a primary statement, which is what
/// makes the ratio worth watching rather than alarming.
///
/// `cache` is the third place a statement can go, and it is the one that never
/// left the process. It belongs in this metric rather than beside it because
/// the question is where the statements went: a hit left out of the total
/// makes every ratio built on it wrong in the direction that flatters the
/// cache, since its best cases would be missing from the denominator.
pub const ROUTE_TOTAL: Metric = Metric {
    name: "pgprox_route_total",
    kind: Kind::Counter,
    help: "Statements routed, by where they went: primary, replica, or the query cache",
    labels: &[ROUTE],
};

/// Buffers borrowed from the node's slab, by state.
///
/// The number that says whether buffer reclaim is working. `outstanding` at or
/// near the bound with `idle` at zero is a node whose connections are waiting
/// for memory, which shows up to a client as latency and to an operator as
/// nothing at all without this.
pub const BUFFER_SLAB: Metric = Metric {
    name: "pgprox_buffer_slab",
    kind: Kind::Gauge,
    help: "Buffers in the node's slab, by state: outstanding, idle, or the bound",
    labels: &[STATE],
};

/// Configuration reloads, by outcome.
///
/// A node serving a stale configuration looks identical to one serving a
/// current configuration, so this is how the difference reaches a dashboard.
pub const CONFIG_RELOAD_TOTAL: Metric = Metric {
    name: "pgprox_config_reload_total",
    kind: Kind::Counter,
    help: "Configuration reload attempts, by outcome",
    labels: &[RESULT],
};

/// Tenants the query cache is configured to serve.
///
/// The metric that says whether the cache is on, and the reason it exists is
/// that no counter can. A node nobody opted in to and a node whose tenants are
/// quiet both report zero hits and zero entries; this is zero on the first and
/// not on the second. See ADR 0021.
pub const CACHE_TENANTS: Metric = Metric {
    name: "pgprox_cache_tenants",
    kind: Kind::Gauge,
    help: "Tenants this node's query cache is configured to serve; zero is a cache that is off",
    labels: &[],
};

/// Query cache entries held.
pub const CACHE_ENTRIES: Metric = Metric {
    name: "pgprox_cache_entries",
    kind: Kind::Gauge,
    help: "Results this node's query cache currently holds",
    labels: &[],
};

/// Query cache memory, held and allowed.
///
/// Both under one name with a `state` label, the way the buffer slab reports
/// its bound: a usage without the budget beside it is a number nobody can act
/// on, and an alert on it would have to hard-code the configured value.
pub const CACHE_BYTES: Metric = Metric {
    name: "pgprox_cache_bytes",
    kind: Kind::Gauge,
    help: "Bytes in this node's query cache, by state: used or the bound",
    labels: &[STATE],
};

/// Query cache outcomes.
///
/// Every counter the store keeps, under one name. `hit`, `miss` and `expired`
/// are outcomes of a lookup and divide into a hit rate; `evicted`,
/// `invalidated` and `rejected` happen to entries rather than to questions and
/// must not be added to that denominator.
pub const CACHE_TOTAL: Metric = Metric {
    name: "pgprox_cache_total",
    kind: Kind::Counter,
    help: "Query cache events, by outcome",
    labels: &[RESULT],
};

/// The metadata for every metric, in registry order.
///
/// What an exporter emits before its samples. Deterministic, so a diff of two
/// nodes' `/metrics` output differs only where the numbers do.
#[must_use]
pub fn describe_all() -> String {
    ALL.iter().map(Metric::describe).collect()
}

/// Everything the proxy exports.
///
/// The list the cardinality test walks, and the list an operator can read to
/// find out what exists without grepping for increment calls.
pub const ALL: &[Metric] = &[
    CLIENT_CONNS,
    UPSTREAM_CONNS,
    QUOTA_LEASED,
    WAIT_SECONDS,
    QUERY_DURATION_SECONDS,
    PIN_TOTAL,
    SHED_TOTAL,
    AUTH_CACHE,
    REPLICA_LAG_BYTES,
    CLUSTER_MEMBERS,
    CLUSTER_VIEW_HASH,
    CONFIG_RELOAD_TOTAL,
    BUFFER_SLAB,
    ROUTE_TOTAL,
    CACHE_TENANTS,
    CACHE_ENTRIES,
    CACHE_BYTES,
    CACHE_TOTAL,
];

impl Metric {
    /// The `HELP` and `TYPE` lines Prometheus expects before the samples.
    ///
    /// The registry renders its own metadata so an exporter is built from it
    /// rather than beside it. Without this, whoever wires the exporter types
    /// every name a second time at the call site, and the registry becomes a
    /// description of what somebody intended rather than of what is exported.
    ///
    /// ```
    /// use pgprox_observe::metrics::WAIT_SECONDS;
    ///
    /// let described = WAIT_SECONDS.describe();
    /// assert!(described.starts_with("# HELP pgprox_wait_seconds "));
    /// assert!(described.contains("\n# TYPE pgprox_wait_seconds histogram\n"));
    /// ```
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "# HELP {name} {help}\n# TYPE {name} {kind}\n",
            name = self.name,
            help = self.help,
            kind = self.kind.as_str(),
        )
    }

    /// Every label, `node` included.
    ///
    /// `node` is not in [`Metric::labels`] because repeating it twelve times
    /// invites leaving it off the thirteenth. It is added here instead, so
    /// forgetting it is not possible.
    #[must_use]
    pub fn all_labels(&self) -> Vec<Label> {
        let mut labels = vec![NODE];
        labels.extend_from_slice(self.labels);
        labels
    }

    /// The worst-case series count for this metric on one node.
    ///
    /// The product of every label's cardinality. This is the number that
    /// matters: labels multiply, so two innocent-looking ones are not innocent
    /// together.
    #[must_use]
    pub fn max_series(&self) -> u64 {
        self.all_labels()
            .iter()
            .map(|label| u64::from(label.cardinality))
            .product()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The rule this module exists to make checkable.
    ///
    /// A `tenant` label at five thousand tenants multiplied by the others is a
    /// series count that takes a Prometheus down, at exactly the moment
    /// somebody is trying to work out why the proxy is unhappy. Per-tenant
    /// detail lives in the admin API instead, which is pull-based and costs
    /// nothing when nobody is looking.
    #[test]
    fn no_metric_has_an_unbounded_label() {
        for metric in ALL {
            for label in metric.all_labels() {
                assert!(
                    label.cardinality <= MAX_LABEL_VALUES,
                    "{metric}: label `{}` can take {} values, which is more than {MAX_LABEL_VALUES}. \
                     Per-entity detail belongs in the admin API, not in a Prometheus label.",
                    label.name,
                    label.cardinality,
                );
                assert!(
                    !label.bounded_because.is_empty(),
                    "{metric}: label `{}` does not say why it is bounded",
                    label.name,
                );
            }
        }
    }

    #[test]
    fn tenant_is_the_label_this_rule_is_about() {
        // Named explicitly, so the next person reaching for it finds this test
        // rather than discovering the problem in production. If a tenant label
        // is ever genuinely needed, it goes behind the allowlist in `tenants`.
        let tenant = Label {
            name: "tenant",
            cardinality: 5_000,
            bounded_because: "it is not",
        };
        assert!(
            tenant.cardinality > MAX_LABEL_VALUES,
            "a tenant label would have to be under the ceiling to be allowed, and it is not"
        );
    }

    #[test]
    fn no_metric_can_produce_an_absurd_number_of_series() {
        // Labels multiply, so each one being individually bounded is not
        // enough: two innocent-looking labels are not innocent together.
        const CEILING: u64 = 4_096;
        for metric in ALL {
            assert!(
                metric.max_series() <= CEILING,
                "{metric} can produce {} series on one node, which is more than {CEILING}",
                metric.max_series()
            );
        }
    }

    #[test]
    fn every_metric_carries_node() {
        // Aggregates are meaningless without it and split brain is invisible
        // without it.
        for metric in ALL {
            assert!(
                metric.all_labels().iter().any(|label| label.name == "node"),
                "{metric} has no node label"
            );
        }
    }

    #[test]
    fn node_is_added_rather_than_repeated_in_every_declaration() {
        // Repeating it twelve times invites leaving it off the thirteenth.
        for metric in ALL {
            assert!(
                !metric.labels.iter().any(|label| label.name == "node"),
                "{metric} declares node itself; all_labels adds it"
            );
        }
    }

    #[test]
    fn every_metric_is_named_and_prefixed() {
        for metric in ALL {
            assert!(
                metric.name.starts_with("pgprox_"),
                "{metric} is not namespaced"
            );
            assert!(!metric.help.is_empty(), "{metric} has no help text");
            assert!(
                metric
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{metric} is not a valid Prometheus name"
            );
        }
    }

    #[test]
    fn no_two_metrics_share_a_name() {
        // A collision would silently merge two unrelated series.
        let names: BTreeSet<&str> = ALL.iter().map(|metric| metric.name).collect();
        assert_eq!(names.len(), ALL.len(), "two metrics share a name");
    }

    #[test]
    fn a_metric_does_not_declare_the_same_label_twice() {
        for metric in ALL {
            let names: BTreeSet<&str> =
                metric.all_labels().iter().map(|label| label.name).collect();
            assert_eq!(
                names.len(),
                metric.all_labels().len(),
                "{metric} declares a label twice"
            );
        }
    }

    #[test]
    fn the_metrics_the_plan_names_all_exist() {
        // The plan lists these by name, and a dashboard built from it should
        // not find one missing.
        for expected in [
            "pgprox_client_conns",
            "pgprox_upstream_conns",
            "pgprox_quota_leased",
            "pgprox_wait_seconds",
            "pgprox_query_duration_seconds",
            "pgprox_pin_total",
            "pgprox_shed_total",
            // The plan spells this without the suffix; see the declaration for
            // why it has one.
            "pgprox_auth_cache_total",
            "pgprox_replica_lag_bytes",
            "pgprox_cluster_members",
            "pgprox_cluster_view_hash",
        ] {
            assert!(
                ALL.iter().any(|metric| metric.name == expected),
                "{expected} is named in the plan and is not exported"
            );
        }
    }

    #[test]
    fn the_latency_signal_that_matters_is_a_histogram() {
        // Everything else can look healthy while this climbs, and an average
        // would hide exactly the tail an operator is looking for.
        assert_eq!(WAIT_SECONDS.kind, Kind::Histogram);
        assert_eq!(QUERY_DURATION_SECONDS.kind, Kind::Histogram);
    }

    #[test]
    fn counters_and_gauges_are_the_right_way_round() {
        // A counter that goes down or a gauge that only goes up is a dashboard
        // that lies about rates.
        for metric in [PIN_TOTAL, SHED_TOTAL, AUTH_CACHE, CONFIG_RELOAD_TOTAL] {
            assert_eq!(metric.kind, Kind::Counter, "{metric}");
            assert!(
                metric.name.ends_with("_total"),
                "{metric} is not named as a counter"
            );
        }
        for metric in [CLIENT_CONNS, UPSTREAM_CONNS, QUOTA_LEASED, CLUSTER_MEMBERS] {
            assert_eq!(metric.kind, Kind::Gauge, "{metric}");
        }
    }

    #[test]
    fn kinds_render_as_prometheus_names_them() {
        assert_eq!(Kind::Gauge.as_str(), "gauge");
        assert_eq!(Kind::Counter.as_str(), "counter");
        assert_eq!(Kind::Histogram.as_str(), "histogram");
    }

    #[test]
    fn the_registry_renders_its_own_metadata() {
        // So an exporter is built from the registry rather than beside it. If
        // it cannot, whoever wires the exporter types every name a second time
        // and the registry describes an intention rather than reality.
        let described = describe_all();

        for metric in ALL {
            assert!(
                described.contains(&format!("# HELP {metric} ")),
                "{metric} has no HELP line"
            );
            assert!(
                described.contains(&format!("# TYPE {metric} {}", metric.kind.as_str())),
                "{metric} has no TYPE line, or the wrong one"
            );
        }
    }

    #[test]
    fn the_metadata_is_well_formed_exposition() {
        // A malformed HELP line makes Prometheus reject the whole scrape, so
        // one bad metric takes out every other one on the node.
        for line in describe_all().lines() {
            assert!(
                line.starts_with("# HELP ") || line.starts_with("# TYPE "),
                "unexpected line in the metadata: {line:?}"
            );
            assert!(
                !line[7..].trim().is_empty(),
                "an empty metadata line: {line:?}"
            );
            assert!(
                !line.contains('\n'),
                "a help text with a newline in it would end the line early: {line:?}"
            );
        }
    }

    #[test]
    fn the_metadata_is_deterministic() {
        // So a diff of two nodes' /metrics output differs only where the
        // numbers do.
        assert_eq!(describe_all(), describe_all());
    }

    #[test]
    fn a_metric_displays_as_its_name() {
        assert_eq!(WAIT_SECONDS.to_string(), "pgprox_wait_seconds");
    }
}
