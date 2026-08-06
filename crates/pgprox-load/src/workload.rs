//! The reference workload, as a document and as a validated value.
//!
//! # Why this is validated rather than deserialized and used
//!
//! A workload with weights that are all zero parses. So does one whose tenant
//! shares add up to 1.4, or one with two statements called `point_select`. Each
//! of those produces a load run that reports numbers, and the numbers are
//! wrong in a way nobody notices until two runs disagree for a reason nobody
//! can find. Validation happens once, here, so a broken workload fails before
//! anything is measured rather than after.
//!
//! # Every error names its field
//!
//! Same rule as `pgprox-config`, for the same reason: an error that does not
//! say which line is wrong means reading the document to guess.

use std::collections::BTreeSet;

use serde::Deserialize;

/// The version this crate knows how to read.
///
/// A workload file is a measurement baseline, so a version that changed
/// meaning without changing its number would silently invalidate every
/// recorded run. Refusing an unknown version is the cheap half of that
/// problem.
pub const SUPPORTED_VERSION: u32 = 3;

/// Shares are floating point, so exact equality is the wrong test. A tenth of
/// a percent is far tighter than any workload distinction that matters and far
/// looser than the error of summing a handful of decimal literals.
const SHARE_TOLERANCE: f64 = 0.001;

/// What went wrong with a workload document.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkloadError {
    /// The bytes were not YAML, or not this shape.
    #[error("workload is not readable: {0}")]
    Unreadable(String),
    /// A field is missing, empty, or outside its range.
    #[error("workload field `{field}`: {problem}")]
    Field {
        /// The field, as written in the document.
        field: &'static str,
        /// What is wrong with it.
        problem: String,
    },
}

impl WorkloadError {
    fn field(field: &'static str, problem: impl Into<String>) -> Self {
        Self::Field {
            field,
            problem: problem.into(),
        }
    }
}

/// Whether a statement writes.
///
/// The load client needs this to know what it may route to a replica; the
/// route decision in the proxy reaches its own conclusion from the SQL, and
/// comparing the two is one of the things a scale run can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Kind {
    /// Does not modify data.
    Read,
    /// Modifies data, so it goes to the primary and moves the watermark.
    Write,
    /// Pins the session to its upstream connection for the rest of its life.
    ///
    /// `LISTEN` is the case ADR 0001 names: a session that has issued one is
    /// bound to the connection the notifications will arrive on, so the proxy
    /// stops multiplexing it and the pool loses that connection until the
    /// client disconnects. The ADR calls the tenant population that decides how
    /// much this matters an open question and hands it to the plan; `M11.7`
    /// measures the half of it that does not need a population, which is the
    /// shape of the curve as the pinned share rises.
    ///
    /// Not a read and not a write, deliberately. It modifies nothing, so
    /// calling it a write would move a watermark that has not moved; and it
    /// must not be sent to a replica, so calling it a read would make it
    /// eligible for one. Every comparison in this crate is against `Read` or
    /// `Write` by name, so a third variant is excluded from both by
    /// construction rather than by a branch somebody has to remember.
    Listen,
}

/// One group of tenants that behave alike.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantGroup {
    /// What to call it in a report.
    pub name: String,
    /// How many tenants are in the group.
    pub count: u32,
    /// The fraction of all traffic this group produces.
    pub share: f64,
}

/// One query shape.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statement {
    /// What to call it in a report.
    pub name: String,
    /// Relative frequency. Weights need not sum to anything in particular.
    pub weight: u32,
    /// Whether it writes.
    pub kind: Kind,
    /// The SQL sent.
    pub sql: String,
}

/// One transaction size and how often it occurs.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionSize {
    /// Statements in the transaction.
    pub statements: u32,
    /// Relative frequency.
    pub weight: u32,
}

/// How long a client waits between transactions.
///
/// Without one, a run with N connections keeps N requests in flight and
/// measures the database queueing rather than the proxy hop. It is also what
/// makes the workload describe the design point, which is connections that are
/// idle most of the time.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Think {
    /// The shortest pause.
    pub min_ms: u64,
    /// The longest.
    pub max_ms: u64,
}

/// How often a connection is replaced.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Churn {
    /// Transactions one connection runs before it goes away.
    pub transactions_per_connection: u32,
}

/// The workload.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    /// The document version. See [`SUPPORTED_VERSION`].
    pub version: u32,
    /// The tenant mix.
    pub tenants: Vec<TenantGroup>,
    /// The query shapes.
    pub statements: Vec<Statement>,
    /// The transaction size distribution.
    pub transactions: Vec<TransactionSize>,
    /// How many statements use the extended protocol with a named prepared
    /// statement, rather than the simple query protocol.
    pub prepared_fraction: f64,
    /// How long a client waits between transactions.
    pub think: Think,
    /// Connection churn.
    pub churn: Churn,
    /// Of the reads, the fraction eligible for a replica.
    pub replica_read_fraction: f64,
    /// Cluster size the gossip budgets measure at.
    pub cluster_size: u32,
}

impl Workload {
    /// Reads and validates a workload document.
    ///
    /// # Errors
    ///
    /// Fails when the bytes are not this document, or when a field is missing,
    /// empty, or outside its range. The error names the field.
    pub fn parse(yaml: &str) -> Result<Self, WorkloadError> {
        let workload: Self =
            serde_yaml::from_str(yaml).map_err(|e| WorkloadError::Unreadable(e.to_string()))?;
        workload.validate()?;
        Ok(workload)
    }

    fn validate(&self) -> Result<(), WorkloadError> {
        if self.version != SUPPORTED_VERSION {
            return Err(WorkloadError::field(
                "version",
                format!(
                    "{} is not supported, expected {SUPPORTED_VERSION}",
                    self.version
                ),
            ));
        }

        self.validate_tenants()?;
        self.validate_statements()?;
        self.validate_transactions()?;

        if self.think.max_ms == 0 {
            return Err(WorkloadError::field(
                "think.max_ms",
                "no pause at all is the saturated run this field exists to stop",
            ));
        }
        if self.think.min_ms > self.think.max_ms {
            return Err(WorkloadError::field(
                "think.min_ms",
                format!(
                    "{} is longer than max_ms of {}",
                    self.think.min_ms, self.think.max_ms
                ),
            ));
        }
        if self.churn.transactions_per_connection == 0 {
            return Err(WorkloadError::field(
                "churn.transactions_per_connection",
                "zero would replace every connection before it ran anything",
            ));
        }
        if !(0.0..=1.0).contains(&self.prepared_fraction) {
            return Err(WorkloadError::field(
                "prepared_fraction",
                format!("{} is not a fraction", self.prepared_fraction),
            ));
        }
        if !(0.0..=1.0).contains(&self.replica_read_fraction) {
            return Err(WorkloadError::field(
                "replica_read_fraction",
                format!("{} is not a fraction", self.replica_read_fraction),
            ));
        }
        if self.cluster_size == 0 {
            return Err(WorkloadError::field(
                "cluster_size",
                "a cluster has at least one node",
            ));
        }
        Ok(())
    }

    fn validate_tenants(&self) -> Result<(), WorkloadError> {
        if self.tenants.is_empty() {
            return Err(WorkloadError::field("tenants", "no tenants, so no traffic"));
        }

        let mut seen = BTreeSet::new();
        let mut total = 0.0;
        for group in &self.tenants {
            if !seen.insert(group.name.as_str()) {
                return Err(WorkloadError::field(
                    "tenants.name",
                    format!("`{}` appears twice", group.name),
                ));
            }
            if group.count == 0 {
                return Err(WorkloadError::field(
                    "tenants.count",
                    format!("`{}` has no tenants in it", group.name),
                ));
            }
            if !(0.0..=1.0).contains(&group.share) {
                return Err(WorkloadError::field(
                    "tenants.share",
                    format!("`{}` has a share of {}", group.name, group.share),
                ));
            }
            total += group.share;
        }

        if (total - 1.0).abs() > SHARE_TOLERANCE {
            return Err(WorkloadError::field(
                "tenants.share",
                format!("shares sum to {total}, not 1"),
            ));
        }
        Ok(())
    }

    fn validate_statements(&self) -> Result<(), WorkloadError> {
        if self.statements.is_empty() {
            return Err(WorkloadError::field(
                "statements",
                "no statements, so nothing to send",
            ));
        }

        let mut seen = BTreeSet::new();
        for statement in &self.statements {
            if !seen.insert(statement.name.as_str()) {
                return Err(WorkloadError::field(
                    "statements.name",
                    format!("`{}` appears twice", statement.name),
                ));
            }
            if statement.sql.trim().is_empty() {
                return Err(WorkloadError::field(
                    "statements.sql",
                    format!("`{}` has no SQL", statement.name),
                ));
            }
        }

        if self.statements.iter().all(|s| s.weight == 0) {
            return Err(WorkloadError::field(
                "statements.weight",
                "every weight is zero, so no statement would ever be chosen",
            ));
        }
        if !self.statements.iter().any(|s| s.kind == Kind::Read) {
            return Err(WorkloadError::field(
                "statements.kind",
                "no read, so replica_read_fraction could never apply",
            ));
        }
        Ok(())
    }

    fn validate_transactions(&self) -> Result<(), WorkloadError> {
        if self.transactions.is_empty() {
            return Err(WorkloadError::field(
                "transactions",
                "no sizes, so no transaction shape",
            ));
        }
        for size in &self.transactions {
            if size.statements == 0 {
                return Err(WorkloadError::field(
                    "transactions.statements",
                    "an empty transaction sends nothing",
                ));
            }
        }
        if self.transactions.iter().all(|t| t.weight == 0) {
            return Err(WorkloadError::field(
                "transactions.weight",
                "every weight is zero, so no size would ever be chosen",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// A valid document, which each test then breaks in exactly one way.
    fn document() -> String {
        [
            "version: 3",
            "tenants:",
            "  - { name: hot, count: 2, share: 0.75 }",
            "  - { name: tail, count: 50, share: 0.25 }",
            "statements:",
            "  - { name: point, weight: 9, kind: read, sql: 'SELECT 1' }",
            "  - { name: write, weight: 1, kind: write, sql: 'UPDATE t SET a = 1' }",
            "transactions:",
            "  - { statements: 1, weight: 4 }",
            "prepared_fraction: 0.5",
            "think: { min_ms: 10, max_ms: 20 }",
            "churn: { transactions_per_connection: 500 }",
            "replica_read_fraction: 0.5",
            "cluster_size: 3",
        ]
        .join("\n")
    }

    #[test]
    fn the_validator_boundaries_are_where_the_document_says() {
        // Two `>` mutants survived, one on each validator, and both sit on a
        // boundary that decides whether a workload document is accepted. A
        // document that should be refused and is not becomes a measurement
        // baseline nobody knows is wrong.

        // `think.min_ms > think.max_ms` is an error; equal is not, because a
        // fixed pause is a legitimate thing to configure. `>=` would refuse it.
        let fixed = document().replace("min_ms: 10, max_ms: 20", "min_ms: 20, max_ms: 20");
        assert!(
            Workload::parse(&fixed).is_ok(),
            "a fixed think time was refused"
        );

        let inverted = document().replace("min_ms: 10, max_ms: 20", "min_ms: 21, max_ms: 20");
        assert!(
            Workload::parse(&inverted).is_err(),
            "a minimum above the maximum was accepted"
        );

        // Shares are compared against a tolerance of 0.001, and a sum exactly
        // at the tolerance is inside it. `>=` would refuse the boundary.
        let at_tolerance = document().replace("share: 0.25 }", "share: 0.251 }");
        assert!(
            Workload::parse(&at_tolerance).is_ok(),
            "a sum exactly at the tolerance was refused"
        );

        let past_tolerance = document().replace("share: 0.25 }", "share: 0.2521 }");
        assert!(
            Workload::parse(&past_tolerance).is_err(),
            "a sum past the tolerance was accepted"
        );
    }

    fn broken(find: &str, replace: &str) -> WorkloadError {
        let text = document().replace(find, replace);
        match Workload::parse(&text) {
            Err(error) => error,
            Ok(_) => panic!("`{find}` -> `{replace}` was accepted"),
        }
    }

    fn field_of(error: &WorkloadError) -> &str {
        match error {
            WorkloadError::Field { field, .. } => field,
            WorkloadError::Unreadable(text) => panic!("unreadable: {text}"),
        }
    }

    #[test]
    fn a_listen_statement_parses_and_is_neither_a_read_nor_a_write() {
        // The third kind exists so a workload can hold a session open, which is
        // what ADR 0001's open question is about. It must not be readable as
        // either of the other two: a write moves a watermark it did not move,
        // and a read is eligible for a replica the notifications never reach.
        let text = document().replace(
            "  - { name: write, weight: 1, kind: write, sql: 'UPDATE t SET a = 1' }",
            "  - { name: write, weight: 1, kind: write, sql: 'UPDATE t SET a = 1' }\n\
             \x20 - { name: watch, weight: 1, kind: listen, sql: 'LISTEN chan' }",
        );
        let workload = Workload::parse(&text).unwrap();

        let Some(watch) = workload.statements.iter().find(|s| s.name == "watch") else {
            panic!("the listen statement was dropped");
        };
        assert_eq!(watch.kind, Kind::Listen);
        assert_ne!(watch.kind, Kind::Read);
        assert_ne!(watch.kind, Kind::Write);
    }

    #[test]
    fn a_document_of_nothing_but_listens_is_refused() {
        // The read requirement is what `replica_read_fraction` rests on, and a
        // `LISTEN` does not satisfy it. Without this the third kind would open
        // a way to write a document whose replica fraction could never apply,
        // which is the case the existing rule was added to catch.
        let text = document()
            .replace("kind: read", "kind: listen")
            .replace("kind: write", "kind: listen");
        let err = Workload::parse(&text).unwrap_err();
        assert_eq!(field_of(&err), "statements.kind");
    }

    #[test]
    fn the_committed_workload_is_valid() {
        // The one that everything in M7 measures against. If it stops parsing,
        // every recorded run loses its meaning, so this is the test that
        // matters most in the file.
        let yaml = include_str!("../../../docs/internal/product/perf/workload.yaml");
        let workload = Workload::parse(yaml).unwrap();

        assert_eq!(workload.version, SUPPORTED_VERSION);
        assert_eq!(workload.tenants.iter().map(|g| g.count).sum::<u32>(), 204);
        assert_eq!(workload.statements.len(), 4);
        assert!(workload.statements.iter().any(|s| s.kind == Kind::Write));
        assert_eq!(workload.churn.transactions_per_connection, 500);
    }

    #[test]
    fn the_committed_pinning_workloads_are_valid_and_differ_only_in_one_weight() {
        // `M11.7`'s curve rests on these three being the reference document
        // with one statement added. If a weight drifted, the curve would be
        // measuring two changes at once and would say nothing about pinning.
        let documents = [
            (
                "low",
                include_str!("../../../docs/internal/product/perf/workload-pin-low.yaml"),
                1,
            ),
            (
                "mid",
                include_str!("../../../docs/internal/product/perf/workload-pin-mid.yaml"),
                2,
            ),
            (
                "high",
                include_str!("../../../docs/internal/product/perf/workload-pin-high.yaml"),
                20,
            ),
        ];

        for (name, yaml, expected_weight) in documents {
            let workload = Workload::parse(yaml)
                .unwrap_or_else(|error| panic!("workload-pin-{name} does not parse: {error}"));

            assert_eq!(workload.version, SUPPORTED_VERSION, "{name}");
            assert_eq!(workload.statements.len(), 5, "{name}");

            let listen: Vec<_> = workload
                .statements
                .iter()
                .filter(|s| s.kind == Kind::Listen)
                .collect();
            assert_eq!(listen.len(), 1, "{name} should hold exactly one LISTEN");
            assert_eq!(listen[0].weight, expected_weight, "{name}");
            assert!(listen[0].sql.starts_with("LISTEN "), "{name}");

            // The other four are the reference mix scaled by ten, which is the
            // same mix. Asserted as the ratio rather than as four numbers, so
            // the claim being checked is the one the documents make.
            let others: Vec<u32> = workload
                .statements
                .iter()
                .filter(|s| s.kind != Kind::Listen)
                .map(|s| s.weight)
                .collect();
            assert_eq!(others, vec![600, 100, 250, 50], "{name}");

            // Everything else has to be the reference document, or the curve
            // is not a curve in one variable.
            assert_eq!(workload.churn.transactions_per_connection, 500, "{name}");
            assert_eq!(
                workload.tenants.iter().map(|g| g.count).sum::<u32>(),
                204,
                "{name}"
            );
            assert_eq!(workload.cluster_size, 3, "{name}");
        }
    }

    #[test]
    fn the_pinning_workloads_rise_in_listen_weight() {
        // The curve needs its three points to be in order and distinct. Two
        // documents that happened to carry the same weight would produce two
        // identical rows and a curve nobody could read as a curve.
        let weights: Vec<u32> = [
            include_str!("../../../docs/internal/product/perf/workload-pin-low.yaml"),
            include_str!("../../../docs/internal/product/perf/workload-pin-mid.yaml"),
            include_str!("../../../docs/internal/product/perf/workload-pin-high.yaml"),
        ]
        .iter()
        .map(|yaml| {
            Workload::parse(yaml)
                .unwrap()
                .statements
                .iter()
                .find(|s| s.kind == Kind::Listen)
                .unwrap()
                .weight
        })
        .collect();

        assert!(
            weights.windows(2).all(|pair| pair[0] < pair[1]),
            "the pinning weights are not strictly increasing: {weights:?}"
        );
    }

    #[test]
    fn a_valid_document_round_trips_its_values() {
        let workload = Workload::parse(&document()).unwrap();
        assert_eq!(workload.tenants[0].name, "hot");
        assert_eq!(workload.tenants.iter().map(|g| g.count).sum::<u32>(), 52);
        assert_eq!(workload.statements[1].kind, Kind::Write);
        assert_eq!(workload.transactions[0].statements, 1);
        assert!((workload.replica_read_fraction - 0.5).abs() < f64::EPSILON);
        assert_eq!(workload.cluster_size, 3);
    }

    #[test]
    fn a_version_this_crate_does_not_know_is_refused() {
        // A file that changed meaning without changing its number would
        // invalidate every recorded run silently.
        assert_eq!(field_of(&broken("version: 3", "version: 4")), "version");
    }

    #[test]
    fn shares_that_do_not_sum_to_one_are_refused() {
        // The failure this catches is a run that reports numbers which are
        // quietly about a different tenant mix than the one written down.
        let error = broken("share: 0.25", "share: 0.35");
        assert_eq!(field_of(&error), "tenants.share");
        assert!(format!("{error}").contains("1.1"), "{error}");
    }

    #[test]
    fn a_share_outside_zero_to_one_is_refused_before_the_sum_is() {
        let error = broken("share: 0.75", "share: 1.75");
        assert_eq!(field_of(&error), "tenants.share");
        assert!(format!("{error}").contains("hot"), "{error}");
    }

    #[test]
    fn a_repeated_name_is_refused() {
        // Two groups called the same thing make a report ambiguous, and a
        // report nobody can read is the same as no measurement.
        assert_eq!(field_of(&broken("name: tail", "name: hot")), "tenants.name");
        assert_eq!(
            field_of(&broken("name: write", "name: point")),
            "statements.name"
        );
    }

    #[test]
    fn an_empty_section_is_refused_and_says_which() {
        assert_eq!(
            field_of(&broken(
                "  - { name: hot, count: 2, share: 0.75 }\n  - { name: tail, count: 50, share: 0.25 }",
                "  []"
            )),
            "tenants"
        );
        assert_eq!(
            field_of(&broken(
                "  - { name: point, weight: 9, kind: read, sql: 'SELECT 1' }\n  - { name: write, weight: 1, kind: write, sql: 'UPDATE t SET a = 1' }",
                "  []"
            )),
            "statements"
        );
        assert_eq!(
            field_of(&broken("  - { statements: 1, weight: 4 }", "  []")),
            "transactions"
        );
    }

    #[test]
    fn a_group_with_no_tenants_in_it_is_refused() {
        assert_eq!(field_of(&broken("count: 2", "count: 0")), "tenants.count");
    }

    #[test]
    fn weights_that_are_all_zero_are_refused() {
        // This is the one that parses cleanly and produces a run that never
        // sends anything, which reads as a proxy that is infinitely fast.
        assert_eq!(
            field_of(&broken(
                "weight: 9, kind: read, sql: 'SELECT 1' }\n  - { name: write, weight: 1",
                "weight: 0, kind: read, sql: 'SELECT 1' }\n  - { name: write, weight: 0"
            )),
            "statements.weight"
        );
        assert_eq!(
            field_of(&broken(
                "statements: 1, weight: 4",
                "statements: 1, weight: 0"
            )),
            "transactions.weight"
        );
    }

    #[test]
    fn a_statement_with_no_sql_is_refused() {
        assert_eq!(
            field_of(&broken("sql: 'SELECT 1'", "sql: '   '")),
            "statements.sql"
        );
    }

    #[test]
    fn a_workload_with_no_read_is_refused() {
        // `replica_read_fraction` would be unreachable, so a run would measure
        // replica routing by never using it.
        assert_eq!(
            field_of(&broken("kind: read", "kind: write")),
            "statements.kind"
        );
    }

    #[test]
    fn an_empty_transaction_is_refused() {
        assert_eq!(
            field_of(&broken("statements: 1", "statements: 0")),
            "transactions.statements"
        );
    }

    #[test]
    fn a_workload_with_no_think_time_is_refused() {
        // The saturated run: N connections meaning N requests in flight,
        // which measures the database rather than the proxy.
        assert_eq!(field_of(&broken("max_ms: 20", "max_ms: 0")), "think.max_ms");
        assert_eq!(
            field_of(&broken("min_ms: 10", "min_ms: 30")),
            "think.min_ms"
        );
    }

    #[test]
    fn churn_of_zero_is_refused() {
        assert_eq!(
            field_of(&broken(
                "transactions_per_connection: 500",
                "transactions_per_connection: 0"
            )),
            "churn.transactions_per_connection"
        );
    }

    #[test]
    fn a_prepared_fraction_that_is_not_a_fraction_is_refused() {
        assert_eq!(
            field_of(&broken("prepared_fraction: 0.5", "prepared_fraction: 2.0")),
            "prepared_fraction"
        );
    }

    #[test]
    fn a_replica_fraction_that_is_not_a_fraction_is_refused() {
        assert_eq!(
            field_of(&broken(
                "replica_read_fraction: 0.5",
                "replica_read_fraction: 1.5"
            )),
            "replica_read_fraction"
        );
    }

    #[test]
    fn a_cluster_of_no_nodes_is_refused() {
        assert_eq!(
            field_of(&broken("cluster_size: 3", "cluster_size: 0")),
            "cluster_size"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_rather_than_ignored() {
        // A misspelled key that is silently dropped is a measurement that
        // silently stops measuring what its author wrote down.
        let text = format!("{}\nreplica_read_fration: 0.9\n", document());
        let error = Workload::parse(&text).unwrap_err();
        assert!(
            matches!(error, WorkloadError::Unreadable(ref text) if text.contains("fration")),
            "{error}"
        );
    }

    #[test]
    fn bytes_that_are_not_a_document_are_refused() {
        let error = Workload::parse("this: [is: not").unwrap_err();
        assert!(matches!(error, WorkloadError::Unreadable(_)), "{error}");
        assert!(format!("{error}").contains("not readable"), "{error}");
    }
}
