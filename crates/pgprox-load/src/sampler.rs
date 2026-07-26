//! Turning a workload into a deterministic stream of transactions.
//!
//! # Why deterministic
//!
//! Two runs a week apart are only comparable if they replayed the same thing.
//! A sampler seeded from the clock produces a run nobody can repeat, so a
//! regression is indistinguishable from a different draw. The seed goes in the
//! run report, and a run can be replayed exactly by passing it back.
//!
//! # Why its own generator
//!
//! This picks between four statements and a couple of transaction sizes. It is
//! not cryptography, nothing about the run is secret, and an adversary who
//! could predict it would gain the ability to know which `SELECT` comes next.
//! A sixty-line generator with a stated period is a smaller thing to justify to
//! this project's supply-chain gate than a dependency tree, and it makes the
//! stream reproducible across versions of anything else.
//!
//! `SystemEntropy` in `bin/pgprox` is the opposite case and stays as it is:
//! cancel keys are a security boundary and are drawn from the platform.

use crate::workload::{Kind, Workload};

/// One statement, as the client will send it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// The shape's name, for the report.
    pub name: String,
    /// The SQL.
    pub sql: String,
    /// Whether it writes.
    pub kind: Kind,
    /// Whether the client may ask for this one to be served by a replica.
    ///
    /// Only reads are ever eligible, and only the declared fraction of them.
    /// A read the client marks eligible may still land on the primary, because
    /// the session's own write watermark outranks the hint. That difference is
    /// exactly what a scale run measures.
    pub replica_eligible: bool,
}

/// One transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    /// Which tenant sends it, as `group-index`.
    pub tenant: String,
    /// The statements, in order.
    pub statements: Vec<Planned>,
    /// How long the client waits before sending the next one.
    ///
    /// Drawn with the transaction rather than after it, so the pause is part
    /// of the reproducible stream and two runs on one seed wait identically.
    pub think_ms: u64,
}

impl Transaction {
    /// Whether anything in it writes, so the whole transaction goes to the
    /// primary.
    #[must_use]
    pub fn writes(&self) -> bool {
        self.statements
            .iter()
            .any(|statement| statement.kind == Kind::Write)
    }
}

/// A reproducible stream of transactions drawn from a workload.
#[derive(Debug)]
pub struct Sampler<'w> {
    workload: &'w Workload,
    rng: Rng,
    replica_units: u64,
    tenant_shares: Vec<u64>,
    statement_weights: Vec<u64>,
    size_weights: Vec<u64>,
}

impl<'w> Sampler<'w> {
    /// Builds a sampler over a validated workload.
    ///
    /// Takes `&Workload` rather than the document, so the invariants
    /// [`Workload::parse`] checks hold here: at least one tenant group, at
    /// least one statement, and weights that are not all zero. Those are what
    /// make the choosing total rather than fallible.
    #[must_use]
    pub fn new(workload: &'w Workload, seed: u64) -> Self {
        Self {
            workload,
            rng: Rng::new(seed),
            replica_units: units(workload.replica_read_fraction),
            tenant_shares: workload.tenants.iter().map(|g| units(g.share)).collect(),
            statement_weights: workload
                .statements
                .iter()
                .map(|statement| u64::from(statement.weight))
                .collect(),
            size_weights: workload
                .transactions
                .iter()
                .map(|size| u64::from(size.weight))
                .collect(),
        }
    }

    /// How many transactions one connection runs before it is replaced.
    #[must_use]
    pub fn transactions_per_connection(&self) -> u32 {
        self.workload.churn.transactions_per_connection
    }

    /// Draws the next transaction.
    pub fn next_transaction(&mut self) -> Transaction {
        let tenant = self.next_tenant();
        let size = self.next_size();
        let statements = (0..size).map(|_| self.next_statement()).collect();
        let think = self.workload.think;
        let think_ms = think.min_ms + self.rng.below(think.max_ms - think.min_ms + 1);
        Transaction {
            tenant,
            statements,
            think_ms,
        }
    }

    fn next_tenant(&mut self) -> String {
        let group = &self.workload.tenants[self.rng.weighted(&self.tenant_shares)];
        // Within a group every tenant is the same, so the index is uniform.
        // The name is what a report groups by and what the proxy sees as a
        // separate grant, so it has to be a real per-tenant string rather than
        // the group's.
        let index = self.rng.below(u64::from(group.count));
        format!("{}-{index}", group.name)
    }

    fn next_size(&mut self) -> u32 {
        self.workload.transactions[self.rng.weighted(&self.size_weights)].statements
    }

    fn next_statement(&mut self) -> Planned {
        let statement = &self.workload.statements[self.rng.weighted(&self.statement_weights)];
        // Drawn for every statement, not only for reads, so that changing the
        // read fraction does not shift the whole stream. A stream that changes
        // shape when an unrelated field moves is one where two runs cannot be
        // compared field by field.
        let roll = self.rng.below(SCALE);
        let eligible = statement.kind == Kind::Read && roll < self.replica_units;

        Planned {
            name: statement.name.clone(),
            sql: statement.sql.clone(),
            kind: statement.kind,
            replica_eligible: eligible,
        }
    }
}

/// Fractions are scaled to this many units once, so that drawing works in
/// integers. Coarse enough to stay exact in `f64`, fine enough that a share of
/// a thousandth is still distinguishable from zero.
const SCALE: u64 = 1_000_000;

/// A validated fraction as a count of units.
///
/// Every caller passes a value the workload validator has already confined to
/// zero through one, so the clamp is what makes this total rather than a case
/// that occurs. The rounding direction does not matter: a millionth of a share
/// is far below any distinction a run could measure.
fn units(fraction: f64) -> u64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=1 first, so the product is within u64 and non-negative"
    )]
    {
        (fraction.clamp(0.0, 1.0) * SCALE as f64) as u64
    }
}

/// A small deterministic generator: xorshift64*, period 2^64 - 1.
///
/// Chosen for being short enough to read in full and stable across releases,
/// which is what a reproducible measurement needs. Not for anything else.
#[derive(Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // Zero is the one state xorshift cannot leave, so it is mapped to a
        // constant rather than left to produce an infinite run of zeroes.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value below `bound`, or zero when `bound` is zero.
    fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }

    /// An index into `weights`, chosen in proportion to them.
    ///
    /// Weights that are all zero fall back to the first entry. The workload
    /// validator refuses that case, so this is the branch that keeps the
    /// function total rather than a case that occurs.
    fn weighted(&mut self, weights: &[u64]) -> usize {
        let total: u64 = weights.iter().sum();
        if total == 0 {
            return 0;
        }
        let mut point = self.below(total);
        for (index, weight) in weights.iter().enumerate() {
            if point < *weight {
                return index;
            }
            point -= weight;
        }
        weights.len() - 1
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    fn workload() -> Workload {
        Workload::parse(include_str!("../../../product/perf/workload.yaml")).unwrap()
    }

    fn draw(seed: u64, count: usize) -> Vec<Transaction> {
        let workload = workload();
        let mut sampler = Sampler::new(&workload, seed);
        (0..count).map(|_| sampler.next_transaction()).collect()
    }

    #[test]
    fn the_same_seed_produces_the_same_stream() {
        // The property the whole crate rests on. Without it a regression and a
        // different draw look identical.
        assert_eq!(draw(42, 200), draw(42, 200));
    }

    #[test]
    fn a_different_seed_produces_a_different_stream() {
        // Otherwise the seed is decoration and every run measures one draw.
        assert_ne!(draw(42, 200), draw(43, 200));
    }

    #[test]
    fn the_statement_mix_converges_on_what_was_declared() {
        // 60/10/25/5 in the committed workload. The tolerance is wide enough
        // for 20k draws and far tighter than any error that would matter.
        let transactions = draw(7, 20_000);
        let statements: Vec<&Planned> = transactions
            .iter()
            .flat_map(|t| t.statements.iter())
            .collect();
        let total = statements.len() as f64;

        for (name, expected) in [
            ("point_select", 0.60),
            ("range_scan", 0.10),
            ("update_account", 0.25),
            ("insert_history", 0.05),
        ] {
            let seen = statements.iter().filter(|s| s.name == name).count() as f64 / total;
            assert!(
                (seen - expected).abs() < 0.02,
                "{name}: saw {seen:.3}, workload declares {expected}"
            );
        }
    }

    #[test]
    fn the_tenant_mix_converges_on_what_was_declared() {
        // Four hot tenants take 80% of the traffic and two hundred idle ones
        // take the rest. A uniform mix would hide both things the proxy is
        // judged on.
        let transactions = draw(11, 20_000);
        let hot = transactions
            .iter()
            .filter(|t| t.tenant.starts_with("hot-"))
            .count() as f64;
        let share = hot / transactions.len() as f64;
        assert!((share - 0.80).abs() < 0.02, "hot tenants took {share:.3}");

        let names: std::collections::BTreeSet<&str> =
            transactions.iter().map(|t| t.tenant.as_str()).collect();
        assert!(
            names.iter().filter(|n| n.starts_with("hot-")).count() == 4,
            "expected exactly the four hot tenants, got {names:?}"
        );
        assert!(
            names.len() > 150,
            "the long tail never appeared: {} distinct tenants",
            names.len()
        );
    }

    #[test]
    fn transaction_sizes_converge_on_what_was_declared() {
        // 80% of one statement, 15% of four, 5% of twenty.
        let transactions = draw(13, 20_000);
        let total = transactions.len() as f64;
        for (size, expected) in [(1, 0.80), (4, 0.15), (20, 0.05)] {
            let seen = transactions
                .iter()
                .filter(|t| t.statements.len() == size)
                .count() as f64
                / total;
            assert!(
                (seen - expected).abs() < 0.02,
                "size {size}: saw {seen:.3}, workload declares {expected}"
            );
        }
        assert!(
            transactions.iter().all(|t| !t.statements.is_empty()),
            "an empty transaction would send nothing and still be counted"
        );
    }

    #[test]
    fn only_reads_are_ever_replica_eligible() {
        // A write marked eligible would be a load client asking for a wrong
        // answer, which measures nothing.
        let transactions = draw(17, 5_000);
        for statement in transactions.iter().flat_map(|t| t.statements.iter()) {
            assert!(
                !(statement.kind == Kind::Write && statement.replica_eligible),
                "a write was marked replica-eligible: {statement:?}"
            );
        }
    }

    #[test]
    fn the_replica_eligible_fraction_is_the_declared_one() {
        // Half the reads, per the committed workload.
        let transactions = draw(19, 20_000);
        let reads: Vec<&Planned> = transactions
            .iter()
            .flat_map(|t| t.statements.iter())
            .filter(|s| s.kind == Kind::Read)
            .collect();
        let eligible =
            reads.iter().filter(|s| s.replica_eligible).count() as f64 / reads.len() as f64;
        assert!(
            (eligible - 0.50).abs() < 0.02,
            "{eligible:.3} of reads were eligible, workload declares 0.50"
        );
    }

    #[test]
    fn a_transaction_that_writes_says_so() {
        let workload = workload();
        let mut sampler = Sampler::new(&workload, 23);
        let mixed = (0..1_000)
            .map(|_| sampler.next_transaction())
            .find(Transaction::writes)
            .unwrap();
        assert!(mixed.statements.iter().any(|s| s.kind == Kind::Write));

        let read_only = Transaction {
            think_ms: 0,
            tenant: "hot-0".into(),
            statements: vec![Planned {
                name: "point".into(),
                sql: "SELECT 1".into(),
                kind: Kind::Read,
                replica_eligible: true,
            }],
        };
        assert!(!read_only.writes());
    }

    #[test]
    fn the_pause_between_transactions_is_inside_what_was_declared() {
        // Without it a run keeps every connection busy and measures the
        // database queueing rather than the proxy.
        let transactions = draw(29, 5_000);
        assert!(
            transactions
                .iter()
                .all(|t| (50..=500).contains(&t.think_ms)),
            "a pause fell outside the declared 50 to 500ms"
        );
        let distinct: std::collections::BTreeSet<u64> =
            transactions.iter().map(|t| t.think_ms).collect();
        assert!(
            distinct.len() > 100,
            "the pause barely varied: {} distinct values",
            distinct.len()
        );
    }

    #[test]
    fn churn_comes_from_the_workload() {
        let workload = workload();
        let sampler = Sampler::new(&workload, 1);
        assert_eq!(sampler.transactions_per_connection(), 500);
    }

    #[test]
    fn a_zero_seed_still_produces_a_stream() {
        // Zero is the state xorshift cannot leave. A run seeded with it would
        // otherwise send the same statement forever and report a suspiciously
        // good number.
        let stream = draw(0, 500);
        let distinct: std::collections::BTreeSet<&str> = stream
            .iter()
            .flat_map(|t| t.statements.iter())
            .map(|s| s.name.as_str())
            .collect();
        assert!(distinct.len() > 1, "the generator was stuck: {distinct:?}");
    }

    #[test]
    fn weighting_is_total_even_where_the_validator_makes_it_unreachable() {
        // Both branches exist so the sampler cannot panic on a workload built
        // in code rather than parsed. Neither is reachable through `parse`.
        let mut rng = Rng::new(5);
        assert_eq!(rng.weighted(&[0, 0, 0]), 0);
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.weighted(&[1, 0, 0]), 0);
    }
}
