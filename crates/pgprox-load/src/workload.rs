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
pub const SUPPORTED_VERSION: u32 = 2;

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

    /// How many tenants the workload describes in total.
    #[must_use]
    pub fn tenant_count(&self) -> u32 {
        self.tenants.iter().map(|group| group.count).sum()
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
            "version: 2",
            "tenants:",
            "  - { name: hot, count: 2, share: 0.75 }",
            "  - { name: tail, count: 50, share: 0.25 }",
            "statements:",
            "  - { name: point, weight: 9, kind: read, sql: 'SELECT 1' }",
            "  - { name: write, weight: 1, kind: write, sql: 'UPDATE t SET a = 1' }",
            "transactions:",
            "  - { statements: 1, weight: 4 }",
            "think: { min_ms: 10, max_ms: 20 }",
            "churn: { transactions_per_connection: 500 }",
            "replica_read_fraction: 0.5",
            "cluster_size: 3",
        ]
        .join("\n")
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
    fn the_committed_workload_is_valid() {
        // The one that everything in M7 measures against. If it stops parsing,
        // every recorded run loses its meaning, so this is the test that
        // matters most in the file.
        let yaml = include_str!("../../../product/perf/workload.yaml");
        let workload = Workload::parse(yaml).unwrap();

        assert_eq!(workload.version, SUPPORTED_VERSION);
        assert_eq!(workload.tenant_count(), 204);
        assert_eq!(workload.statements.len(), 4);
        assert!(workload.statements.iter().any(|s| s.kind == Kind::Write));
        assert_eq!(workload.churn.transactions_per_connection, 500);
    }

    #[test]
    fn a_valid_document_round_trips_its_values() {
        let workload = Workload::parse(&document()).unwrap();
        assert_eq!(workload.tenants[0].name, "hot");
        assert_eq!(workload.tenant_count(), 52);
        assert_eq!(workload.statements[1].kind, Kind::Write);
        assert_eq!(workload.transactions[0].statements, 1);
        assert!((workload.replica_read_fraction - 0.5).abs() < f64::EPSILON);
        assert_eq!(workload.cluster_size, 3);
    }

    #[test]
    fn a_version_this_crate_does_not_know_is_refused() {
        // A file that changed meaning without changing its number would
        // invalidate every recorded run silently.
        assert_eq!(field_of(&broken("version: 2", "version: 3")), "version");
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
