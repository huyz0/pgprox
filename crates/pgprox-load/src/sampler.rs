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
    /// Whether the client sends this one through the extended protocol, with
    /// a named prepared statement, rather than the simple query protocol.
    ///
    /// Every mainstream driver uses the extended protocol, and it is the path
    /// whose statement mapping the proxy has to get right.
    pub prepared: bool,
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

/// A reproducible stream of transactions drawn from a workload.
#[derive(Debug)]
pub struct Sampler<'w> {
    workload: &'w Workload,
    rng: Rng,
    replica_units: u64,
    prepared_units: u64,
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
            prepared_units: units(workload.prepared_fraction),
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
        // Its own draw, so changing one fraction does not shift the other's
        // stream and two runs stay comparable field by field.
        let prepared = self.rng.below(SCALE) < self.prepared_units;

        Planned {
            name: statement.name.clone(),
            sql: statement.sql.clone(),
            kind: statement.kind,
            replica_eligible: eligible,
            prepared,
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
    /// `M14.43`. Eleven mutants survived in this file. Three are in the
    /// generator, which decides which schedule a measurement run explores, and
    /// the rest are boundaries on the draws that use it.
    #[test]
    fn a_roll_equal_to_the_fraction_is_outside_it() {
        const SEED: u64 = 12_345;

        // `roll < units` had two survivors, `<=` on the replica draw and on the
        // prepared draw. The two operators disagree for exactly one value of
        // `roll` out of a million, so no distributional test separates them:
        // the expected counts differ by one draw in `SCALE`.
        //
        // It is reachable, though, because the draw order is fixed and this
        // file's own comments treat it as a contract: each fraction gets its
        // own draw so that changing one does not shift the other's stream. So
        // the test replays that order to find the exact roll the first
        // statement will make, then sets the fraction to that value. With `<`
        // the statement is outside the fraction; with `<=` it is inside.
        //
        // If the draw order ever changes this test fails, which is correct:
        // the order is what makes two runs comparable field by field.
        let document = [
            "version: 3",
            "tenants:",
            "  - { name: only, count: 1, share: 1.0 }",
            "statements:",
            "  - { name: read, weight: 1, kind: read, sql: 'SELECT 1' }",
            "transactions:",
            "  - { statements: 1, weight: 1 }",
            "prepared_fraction: 0.0",
            "think: { min_ms: 1, max_ms: 1 }",
            "churn: { transactions_per_connection: 500 }",
            "replica_read_fraction: 0.0",
            "cluster_size: 1",
        ]
        .join("\n");
        let base = Workload::parse(&document).unwrap();

        // Replay the draws `next_transaction` makes before the replica roll:
        // the tenant group, the tenant index, the transaction size, and the
        // statement.
        let shape = Sampler::new(&base, SEED);
        let mut probe = Rng::new(SEED);
        let group = &base.tenants[probe.weighted(&shape.tenant_shares)];
        let _index = probe.below(u64::from(group.count));
        let _size = probe.weighted(&shape.size_weights);
        let _statement = probe.weighted(&shape.statement_weights);
        let roll = probe.below(SCALE);

        // A fraction whose unit count is exactly that roll.
        #[allow(clippy::cast_precision_loss, reason = "roll is below SCALE")]
        let fraction = roll as f64 / SCALE as f64;
        assert_eq!(
            units(fraction),
            roll,
            "the fraction does not land on the roll"
        );

        let mut replica = base.clone();
        replica.replica_read_fraction = fraction;
        let planned = Sampler::new(&replica, SEED).next_transaction();
        assert!(
            !planned.statements[0].replica_eligible,
            "a roll equal to the fraction was treated as inside it"
        );

        // The same boundary on the prepared draw, which is the draw after.
        let prepared_roll = probe.below(SCALE);
        #[allow(clippy::cast_precision_loss, reason = "roll is below SCALE")]
        let prepared_fraction = prepared_roll as f64 / SCALE as f64;
        assert_eq!(units(prepared_fraction), prepared_roll);

        let mut prepared = base.clone();
        prepared.prepared_fraction = prepared_fraction;
        let planned = Sampler::new(&prepared, SEED).next_transaction();
        assert!(
            !planned.statements[0].prepared,
            "a roll equal to the fraction was treated as inside it"
        );

        // And one unit higher is inside, so the assertions above are about the
        // boundary rather than about the fraction being too small to matter.
        #[allow(clippy::cast_precision_loss, reason = "roll is below SCALE")]
        let inside = (roll + 1) as f64 / SCALE as f64;
        if units(inside) == roll + 1 {
            let mut wider = base.clone();
            wider.replica_read_fraction = inside;
            let planned = Sampler::new(&wider, SEED).next_transaction();
            assert!(
                planned.statements[0].replica_eligible,
                "a roll below the fraction was treated as outside it"
            );
        }
    }

    #[test]
    fn think_time_spans_the_whole_configured_range_inclusive() {
        // `min_ms + below(max_ms - min_ms + 1)` had two survivors on the `+ 1`.
        // Without it the maximum is never drawn, so every run pauses slightly
        // less than configured and the load is quietly higher than the document
        // says. `-` instead of `+` would draw below the minimum.
        let mut workload = workload();
        workload.think.min_ms = 10;
        workload.think.max_ms = 12;

        let mut sampler = Sampler::new(&workload, 5);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..2_000 {
            seen.insert(sampler.next_transaction().think_ms);
        }

        assert_eq!(
            seen,
            [10, 11, 12].into_iter().collect(),
            "think time did not cover exactly its configured range"
        );
    }

    #[test]
    fn a_fraction_of_zero_draws_nothing_and_one_draws_everything() {
        // `roll < units` had four survivors across the replica and prepared
        // draws. `<=` lets a fraction of zero through once every SCALE draws,
        // and `>` inverts the fraction outright, which would make a workload
        // configured for half its statements on a replica send the other half.
        //
        // The two extremes are what pin the comparison: at zero nothing may be
        // eligible, and at one everything must be.
        let mut none = workload();
        none.replica_read_fraction = 0.0;
        none.prepared_fraction = 0.0;
        let mut sampler = Sampler::new(&none, 11);
        for _ in 0..500 {
            for planned in sampler.next_transaction().statements {
                assert!(
                    !planned.replica_eligible,
                    "a fraction of zero sent a read to a replica"
                );
                assert!(!planned.prepared, "a fraction of zero prepared a statement");
            }
        }

        let mut all = workload();
        all.replica_read_fraction = 1.0;
        all.prepared_fraction = 1.0;
        let mut sampler = Sampler::new(&all, 11);
        let mut reads = 0_u32;
        for _ in 0..500 {
            for planned in sampler.next_transaction().statements {
                assert!(
                    planned.prepared,
                    "a fraction of one left a statement unprepared"
                );
                if planned.kind == Kind::Read {
                    reads += 1;
                    assert!(
                        planned.replica_eligible,
                        "a fraction of one kept a read off the replica"
                    );
                }
            }
        }
        assert!(
            reads > 0,
            "no read was drawn, so the assertion above never ran"
        );
    }

    #[test]
    fn the_generator_produces_its_documented_sequence() {
        // A golden vector, for the reason `M14.15` used one for the cluster
        // simulator: every property short of the value itself holds for almost
        // any mixing function, including each of these mutants. A load run is
        // only comparable with another run if the same seed draws the same
        // stream, which is the whole basis for matched-pair measurement, and
        // `M11.1`'s eight pairs rest on it.
        let mut rng = Rng::new(1);
        let drawn: Vec<u64> = (0..5).map(|_| rng.next()).collect();
        assert_eq!(
            drawn,
            vec![
                0x47e4_ce4b_896c_dd1d,
                0xabcf_a6a8_e079_651d,
                0xb9d1_0d8f_eb73_1f57,
                0x4db4_18a0_bb1b_019d,
                0x0e61_99b0_4d5a_a600,
            ],
            "the generator is no longer the xorshift this file documents"
        );
    }

    #[test]
    fn a_weighted_draw_lands_in_the_entry_its_point_falls_in() {
        // `point -= weight` could become `+` or `/`, which walks the wrong way
        // through the table and skews every mix in the workload: the share of
        // reads to writes, the transaction sizes, the tenant distribution.
        //
        // Driven over the whole range rather than sampled, so the boundaries
        // between entries are all covered.
        let weights = [1_u64, 2, 3];
        let mut seen = [0_usize; 3];
        let mut rng = Rng::new(99);
        for _ in 0..6_000 {
            seen[rng.weighted(&weights)] += 1;
        }

        // Every entry is reachable, which `+` breaks by never leaving the
        // first, and the counts follow the weights rather than being uniform.
        assert!(
            seen.iter().all(|count| *count > 0),
            "an entry was unreachable: {seen:?}"
        );
        assert!(
            seen[0] < seen[1],
            "weight 1 drew at least as often as weight 2: {seen:?}"
        );
        assert!(
            seen[1] < seen[2],
            "weight 2 drew at least as often as weight 3: {seen:?}"
        );

        // Roughly one, two and three sixths, generously bounded so this is a
        // statement about the walk rather than about the generator.
        let total: usize = seen.iter().sum();
        let sixth = total / 6;
        assert!(seen[0].abs_diff(sixth) < sixth / 2, "{seen:?}");
        assert!(seen[2].abs_diff(sixth * 3) < sixth, "{seen:?}");

        // All-zero weights fall back to the first entry rather than panicking.
        assert_eq!(rng.weighted(&[0, 0, 0]), 0);
    }

    fn workload() -> Workload {
        Workload::parse(include_str!(
            "../../../docs/internal/product/perf/workload.yaml"
        ))
        .unwrap()
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
    fn the_pinning_workloads_emit_listen_at_the_rate_they_declare() {
        // The arithmetic `M11.7`'s curve is predicted from. A document whose
        // `LISTEN` weight did not reach the sampled stream would produce a run
        // with no pins in it, which reads as "pinning costs nothing" and is
        // the reference workload wearing another name.
        for (name, yaml, expected) in [
            (
                "low",
                include_str!("../../../docs/internal/product/perf/workload-pin-low.yaml"),
                1.0 / 1001.0,
            ),
            (
                "mid",
                include_str!("../../../docs/internal/product/perf/workload-pin-mid.yaml"),
                2.0 / 1002.0,
            ),
            (
                "high",
                include_str!("../../../docs/internal/product/perf/workload-pin-high.yaml"),
                20.0 / 1020.0,
            ),
        ] {
            let workload = Workload::parse(yaml).unwrap();
            let mut sampler = Sampler::new(&workload, 13);
            let statements: Vec<Planned> = (0..40_000)
                .flat_map(|_| sampler.next_transaction().statements)
                .collect();
            let total = statements.len() as f64;
            let seen = statements
                .iter()
                .filter(|s| s.name == "watch_channel")
                .count() as f64
                / total;

            // Relative rather than absolute: the three rates differ by a factor
            // of twenty, so one tolerance that suits the largest would pass a
            // smallest of zero.
            assert!(
                (seen - expected).abs() < expected * 0.35,
                "{name}: saw {seen:.5}, workload declares {expected:.5}"
            );
        }
    }

    #[test]
    fn a_pinning_workload_never_sends_its_listen_to_a_replica() {
        // The invariant that made `M11.4` file `Kind::Listen` as a variant
        // rather than reuse `Read`. It was vacuous there, because no committed
        // document held a `LISTEN` statement to check it against. These do.
        let workload = Workload::parse(include_str!(
            "../../../docs/internal/product/perf/workload-pin-high.yaml"
        ))
        .unwrap();
        let mut sampler = Sampler::new(&workload, 17);
        let statements: Vec<Planned> = (0..20_000)
            .flat_map(|_| sampler.next_transaction().statements)
            .collect();

        let watches = statements.iter().filter(|s| s.name == "watch_channel");
        let mut seen = 0;
        for statement in watches {
            seen += 1;
            assert!(
                !statement.replica_eligible,
                "a LISTEN was marked replica-eligible, where the notifications never arrive"
            );
        }
        assert!(seen > 0, "no LISTEN was drawn, so nothing was checked");
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
    fn the_prepared_share_is_the_declared_one() {
        // Half, per the committed workload. The extended protocol is what
        // every mainstream driver uses and what deadlocked twice in M6, so a
        // workload that never sent it measured a proxy nobody deploys.
        let transactions = draw(31, 20_000);
        let statements: Vec<&Planned> = transactions
            .iter()
            .flat_map(|t| t.statements.iter())
            .collect();
        let prepared =
            statements.iter().filter(|s| s.prepared).count() as f64 / statements.len() as f64;
        assert!(
            (prepared - 0.50).abs() < 0.02,
            "{prepared:.3} of statements were prepared, workload declares 0.50"
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
            // And a `LISTEN`, for a different reason: it modifies nothing, so
            // the answer would not be wrong, but the session is pinned to
            // whichever connection the notifications arrive on and a replica is
            // the one place they will not.
            assert!(
                !(statement.kind == Kind::Listen && statement.replica_eligible),
                "a listen was marked replica-eligible: {statement:?}"
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
