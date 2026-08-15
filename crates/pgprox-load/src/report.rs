//! Latency, counted rather than stored, and the report a run produces.
//!
//! # Why a histogram and not a list of samples
//!
//! A run at 100k connections produces tens of millions of samples. Keeping them
//! to sort at the end would cost hundreds of megabytes in the process that is
//! measuring memory, which is the one place that error is least acceptable.
//! Counting into buckets costs a fixed few tens of kilobytes and an increment
//! per sample.
//!
//! # Why the buckets are exact where they matter
//!
//! The added latency this proxy is judged on is under a millisecond, so the
//! first tier is one microsecond wide and percentiles below 10ms are exact
//! rather than interpolated. Past that the question changes from "how fast" to
//! "how bad was the tail", and coarser buckets answer that just as well.
//!
//! # Why the report is JSON
//!
//! `scripts/scale.sh` reads it. A number a script has to scrape out of prose is
//! a number that changes meaning the next time somebody edits the prose.

use std::collections::BTreeMap;

use serde::Serialize;

/// What a failure that never reached a server is counted under.
///
/// A socket that would not open, a TLS handshake that failed, a frame that
/// would not decode, a client's own timeout. None of them has a SQLSTATE, and
/// borrowing one for them (`08006` is the obvious candidate) would put the
/// server's vocabulary on something the server never said. A SQLSTATE is five
/// alphanumeric characters, so this cannot collide with a real one.
pub const NO_SQLSTATE: &str = "local";

/// How many distinct messages are kept under one code.
///
/// Kept at all because a code is not always the whole answer, and unbounded
/// message variety is real: a socket error carries an address, a server's own
/// error carries whatever it wants to say, and a run against Postgres directly
/// sees the full vocabulary. A map keyed by message would grow with the run,
/// in the process that is measuring memory. Past this many, further messages
/// still raise the count and stop being listed.
///
/// Against this proxy the messages are less use than they look, which is worth
/// knowing before reading a report: `ClientError::client_message` is
/// deliberately vague, so every `53300` this proxy sends reads "too many
/// connections, please retry" whether it came from a node at its own client
/// ceiling or from a fleet at its upstream cap. That is the security posture
/// working. It means the client side of a run gives the code distribution and
/// the node's own view has to say which refusal produced it.
pub const MESSAGE_VARIANTS: usize = 8;

/// One microsecond per bucket, up to this value.
const FINE_LIMIT: u64 = 10_000;
/// A hundred microseconds per bucket, from `FINE_LIMIT` up to this value.
const MEDIUM_LIMIT: u64 = 1_000_000;
const MEDIUM_WIDTH: u64 = 100;
/// Ten milliseconds per bucket, from `MEDIUM_LIMIT` up to this value. Anything
/// slower is counted as this: a proxy taking a minute to answer has a problem
/// no percentile is going to describe.
const COARSE_LIMIT: u64 = 60_000_000;
const COARSE_WIDTH: u64 = 10_000;

// As `usize`, because they index. Written out rather than cast so the counts
// are a fact of the file rather than a conversion that has to be justified.
const FINE_BUCKETS: usize = 10_000;
const MEDIUM_BUCKETS: usize = 9_900;
const COARSE_BUCKETS: usize = 5_900;
const BUCKETS: usize = FINE_BUCKETS + MEDIUM_BUCKETS + COARSE_BUCKETS + 1;

/// Latency samples, in microseconds.
#[derive(Debug)]
pub struct Histogram {
    counts: Vec<u32>,
    count: u64,
    max: u64,
    total: u128,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    /// An empty histogram.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counts: vec![0; BUCKETS],
            count: 0,
            max: 0,
            total: 0,
        }
    }

    /// Records one sample, in microseconds.
    pub fn record(&mut self, micros: u64) {
        let bucket = Self::bucket(micros);
        self.counts[bucket] = self.counts[bucket].saturating_add(1);
        self.count += 1;
        self.max = self.max.max(micros);
        self.total += u128::from(micros);
    }

    /// How many samples were recorded.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// The largest sample, exactly, whatever bucket it landed in.
    #[must_use]
    pub fn max(&self) -> u64 {
        self.max
    }

    /// The mean, in microseconds, or zero when nothing was recorded.
    #[must_use]
    pub fn mean(&self) -> u64 {
        if self.count == 0 {
            return 0;
        }
        u64::try_from(self.total / u128::from(self.count)).unwrap_or(u64::MAX)
    }

    /// The value at `quantile`, in microseconds.
    ///
    /// Returns the upper edge of the bucket holding the sample at that rank,
    /// so the answer is exact below 10ms and never optimistic above it. An
    /// empty histogram reports zero, which is the only honest answer to "the
    /// median of nothing" that a caller can print.
    #[must_use]
    pub fn percentile(&self, quantile: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        // The rank of the sample being asked for, one-based: the median of ten
        // samples is the fifth, not the fourth.
        let rank = rank_of(quantile, self.count);

        let mut seen = 0_u64;
        for (index, count) in self.counts.iter().enumerate() {
            seen += u64::from(*count);
            if seen >= rank {
                // The overflow bucket has no upper edge, so the largest sample
                // is the only honest thing to report for it. Reporting its
                // nominal edge would say a minute about a run that took ten.
                if index == BUCKETS - 1 {
                    return self.max;
                }
                return Self::upper_edge(index).min(self.max);
            }
        }
        self.max
    }

    fn bucket(micros: u64) -> usize {
        // Every conversion below is of a value the branch has already bounded,
        // so the fallback is the overflow bucket rather than a case that
        // occurs. It is written this way because a cast that silently wrapped
        // would put a slow sample in a fast bucket.
        let index = |value: u64| usize::try_from(value).unwrap_or(BUCKETS - 1);

        if micros < FINE_LIMIT {
            return index(micros);
        }
        if micros < MEDIUM_LIMIT {
            return FINE_BUCKETS + index((micros - FINE_LIMIT) / MEDIUM_WIDTH);
        }
        if micros < COARSE_LIMIT {
            return FINE_BUCKETS + MEDIUM_BUCKETS + index((micros - MEDIUM_LIMIT) / COARSE_WIDTH);
        }
        BUCKETS - 1
    }

    /// The largest value that lands in this bucket.
    fn upper_edge(bucket: usize) -> u64 {
        let width = |steps: usize| u64::try_from(steps).unwrap_or(u64::MAX);

        if bucket < FINE_BUCKETS {
            return width(bucket);
        }
        if bucket < FINE_BUCKETS + MEDIUM_BUCKETS {
            let step = width(bucket - FINE_BUCKETS);
            return FINE_LIMIT + step * MEDIUM_WIDTH + (MEDIUM_WIDTH - 1);
        }
        if bucket < BUCKETS - 1 {
            let step = width(bucket - FINE_BUCKETS - MEDIUM_BUCKETS);
            return MEDIUM_LIMIT + step * COARSE_WIDTH + (COARSE_WIDTH - 1);
        }
        COARSE_LIMIT
    }
}

/// The one-based rank a quantile asks for out of `count` samples.
///
/// Kept separate because the rounding is the part that is easy to get wrong and
/// worth testing on its own: p50 of ten samples is the fifth, p99 of a hundred
/// is the ninety-ninth, and p100 of anything is the last.
fn rank_of(quantile: f64, count: u64) -> u64 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        reason = "quantile is clamped to 0..=1, so the product is within count"
    )]
    let rank = (quantile.clamp(0.0, 1.0) * count as f64).ceil() as u64;
    rank.max(1).min(count)
}

/// Latency, as a report states it. Microseconds throughout.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Latency {
    /// Samples.
    pub count: u64,
    /// The mean.
    pub mean_us: u64,
    /// The median.
    pub p50_us: u64,
    /// The ninetieth percentile.
    pub p90_us: u64,
    /// The ninety-ninth, which is what the roadmap's target is stated in.
    pub p99_us: u64,
    /// The largest sample.
    pub max_us: u64,
}

impl From<&Histogram> for Latency {
    fn from(histogram: &Histogram) -> Self {
        Self {
            count: histogram.count(),
            mean_us: histogram.mean(),
            p50_us: histogram.percentile(0.50),
            p90_us: histogram.percentile(0.90),
            p99_us: histogram.percentile(0.99),
            max_us: histogram.max(),
        }
    }
}

/// What the clients that failed were told, keyed by SQLSTATE.
///
/// A count of failures is not diagnosable on its own, and neither is one
/// example of the first. `M11.6` asks what a fleet with no upstream capacity
/// left tells the clients a dead node displaces, and the answer is a mixture:
/// some are refused at the door by the node's own gate, some wait for an
/// upstream connection and are told the server is full, some wait and time
/// out. Three operator responses, one error count.
///
/// What this can and cannot separate is worth stating, because the answer is
/// a design decision rather than an oversight. The two refusals differ in
/// their code from the timeout and not from each other: both are `53300`, and
/// the message a client sees is the same vague sentence for both, on purpose.
/// See [`MESSAGE_VARIANTS`].
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Outcomes(BTreeMap<String, Outcome>);

/// How many failures carried one SQLSTATE, and what they said.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Outcome {
    /// How many transactions ended this way.
    pub count: u64,
    /// The distinct messages seen under this code, counted.
    ///
    /// Capped at [`MESSAGE_VARIANTS`]; see there for why. The counts here sum
    /// to `count` only while the cap has not been reached.
    pub messages: BTreeMap<String, u64>,
}

impl Outcomes {
    /// Counts one failure.
    ///
    /// An empty code is a server that sent no SQLSTATE, which is not a code,
    /// so it is counted with the failures that never reached one.
    pub fn record(&mut self, code: &str, message: &str) {
        let code = if code.is_empty() { NO_SQLSTATE } else { code };
        // `entry` allocates the key on every call rather than only on the ones
        // that insert. Left as it is: this runs once per failed transaction in
        // a measurement tool, never in the proxy, and the version that avoids
        // the allocation needs an unreachable branch to satisfy the borrow
        // checker. An unreachable branch in a crate held to 95% is a worse
        // trade than an allocation on an error path.
        let outcome = self.0.entry(code.to_owned()).or_default();
        outcome.count += 1;
        if let Some(seen) = outcome.messages.get_mut(message) {
            *seen += 1;
        } else if outcome.messages.len() < MESSAGE_VARIANTS {
            outcome.messages.insert(message.to_owned(), 1);
        }
    }

    /// Folds another set of outcomes into this one.
    ///
    /// One per connection, merged at the end, because a shared map behind a
    /// lock would be contention the run does not need to measure.
    pub fn merge(&mut self, other: &Self) {
        for (code, outcome) in &other.0 {
            // Counts move over whole rather than one call to `record` per
            // failure: a run's error count reaches six figures, and merging by
            // replaying it would make the summary cost what the run cost.
            let mine = self.0.entry(code.clone()).or_default();
            mine.count += outcome.count;
            for (message, count) in &outcome.messages {
                if let Some(seen) = mine.messages.get_mut(message) {
                    *seen += count;
                } else if mine.messages.len() < MESSAGE_VARIANTS {
                    mine.messages.insert(message.clone(), *count);
                }
            }
        }
    }

    /// How many failures are counted here.
    ///
    /// Equal to a report's `errors` plus its `relocations`: a relocation is
    /// not a failure but it is something a client was told, and dropping it
    /// here would leave `57P01` invisible in the one document that says what
    /// clients saw.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.0.values().map(|outcome| outcome.count).sum()
    }

    /// What was seen under one code, if anything was.
    #[must_use]
    pub fn get(&self, code: &str) -> Option<&Outcome> {
        self.0.get(code)
    }

    /// Whether nothing failed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every code seen, with what it carried.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Outcome)> {
        self.0.iter()
    }
}

/// What one run of the load client produced.
///
/// The workload version and the seed are in here because a number without them
/// cannot be reproduced, and a number nobody can reproduce is not a
/// measurement.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Where the load was pointed, as given on the command line.
    pub target: String,
    /// The workload document's version.
    pub workload_version: u32,
    /// The sampler seed.
    pub seed: u64,
    /// Client connections opened.
    pub connections: u32,
    /// How long the run lasted.
    pub duration_ms: u64,
    /// Transactions that completed.
    pub transactions: u64,
    /// Transactions that failed and lost work.
    ///
    /// Counted rather than swallowed: a run that reports a wonderful p99
    /// because most of its work errored out immediately is the failure mode a
    /// load client has to make impossible to miss.
    ///
    /// A relocation is not one of these. See [`Report::relocations`].
    pub errors: u64,
    /// Transactions abandoned because the node asked this client to leave.
    ///
    /// A drain, a shed and a rolling restart all end with `57P01` on a
    /// connection that is between transactions, and every mainstream driver
    /// answers that by reconnecting. Counting it as a failure would mean a
    /// working drain could never report zero, which makes "zero failed
    /// transactions" a target nothing can hit and a number nobody reads.
    ///
    /// The distinction is where the `57P01` lands. Between transactions it is
    /// the node relocating a client, which costs a reconnect and no work. Once
    /// a statement in the transaction has succeeded it is the force-close
    /// after `drain_grace`, which lost something, and that is an error.
    pub relocations: u64,
    /// What the most recent failure said, when there was one.
    ///
    /// A count on its own is not diagnosable: three errors in a run of sixteen
    /// thousand is either a proxy refusing connections or a client giving up
    /// on its own timeout, and those want opposite responses. The most recent
    /// rather than the first: a connection retries for as long as the run
    /// does, and a target can change why it refuses partway through, so the
    /// first reason seen can describe a moment already gone by the time this
    /// is read.
    ///
    /// The field is still named for the first failure `M7.5` reported, which
    /// is why it does not match what it now holds; renaming it would change
    /// the JSON key `scripts/scale.sh` and any stored report already read by
    /// hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_error: Option<String>,
    /// What the clients that did not get what they asked for were told.
    ///
    /// Sums to `errors` plus `relocations`. See [`Outcomes`].
    pub outcomes: Outcomes,
    /// Per-transaction latency.
    pub latency: Latency,
}

impl Report {
    /// Transactions per second, over the whole run.
    #[must_use]
    pub fn throughput(&self) -> f64 {
        if self.duration_ms == 0 {
            return 0.0;
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "a rate printed to three decimals; the loss is far below the run's noise"
        )]
        {
            self.transactions as f64 * 1000.0 / self.duration_ms as f64
        }
    }

    /// The report as JSON, for a script to read.
    ///
    /// # Errors
    ///
    /// Fails only if the report cannot be serialized, which for this shape
    /// means a bug in this crate rather than anything a caller did.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    /// `M14.43`. Thirty mutants survived in this file, twenty-four of them in
    /// `Histogram::bucket` and `Histogram::upper_edge`, which between them are
    /// every latency number this project has ever published. A percentile that
    /// is quietly wrong does not fail anything: it changes what the repo
    /// believes about itself. `M11.1` overturned `M10.9`'s throughput claim on
    /// numbers this code produced.
    #[test]
    fn merging_outcomes_keeps_a_bounded_set_of_messages() {
        // `mine.messages.len() < MESSAGE_VARIANTS` could become `>`, which
        // stops recording messages after the first and starts again never. The
        // cap exists so one client's unique-per-connection error text cannot
        // grow a report without bound, and a report that silently keeps one
        // message per code is a report an operator reads as "there was only
        // one kind of failure".
        let mut into = Outcomes::default();
        let mut from = Outcomes::default();
        for i in 0..MESSAGE_VARIANTS + 3 {
            from.record("53300", &format!("message {i}"));
        }
        into.merge(&from);

        let merged: Vec<_> = into.iter().collect();
        assert_eq!(merged.len(), 1, "no code was merged");
        let outcome = merged[0].1;
        assert_eq!(
            outcome.count,
            u64::try_from(MESSAGE_VARIANTS + 3).unwrap_or(u64::MAX),
            "every occurrence should be counted even when its text is not kept"
        );
        assert_eq!(
            outcome.messages.len(),
            MESSAGE_VARIANTS,
            "the message set is not held at its cap"
        );
    }

    #[test]
    fn iterating_outcomes_yields_what_was_recorded() {
        // `iter` could return an empty iterator, which would make every report
        // read as though no client was ever told anything. `M11.8` added this
        // to answer what a full fleet tells displaced clients, and `M11.6`'s
        // whole result is a count of SQLSTATEs read through here.
        let mut outcomes = Outcomes::default();
        outcomes.record("53300", "too many connections");
        outcomes.record("53300", "too many connections");
        outcomes.record("57014", "canceled");

        let seen: std::collections::BTreeMap<&str, u64> = outcomes
            .iter()
            .map(|(code, outcome)| (code.as_str(), outcome.count))
            .collect();
        assert_eq!(seen.len(), 2, "iter did not yield both codes");
        assert_eq!(seen.get("53300"), Some(&2));
        assert_eq!(seen.get("57014"), Some(&1));
    }

    #[test]
    fn the_bucket_count_is_the_sum_of_the_three_bands_and_the_overflow() {
        // `FINE + MEDIUM + COARSE + 1` had four surviving mutants on its
        // operators. The constant sizes the backing vector, so a wrong value is
        // either wasted memory or an index that lands in the overflow bucket.
        assert_eq!(FINE_BUCKETS, 10_000);
        assert_eq!(MEDIUM_BUCKETS, 9_900);
        assert_eq!(COARSE_BUCKETS, 5_900);
        assert_eq!(BUCKETS, 25_801, "three bands and one overflow bucket");
    }

    #[test]
    fn a_bucket_and_its_upper_edge_are_inverses() {
        // The strongest thing that can be said about these two functions is
        // that they agree, across every bucket rather than at a few sampled
        // points. Twenty-four mutants sit on their arithmetic and most of them
        // move a boundary by one, which no spot check finds reliably.
        for index in 0..BUCKETS - 1 {
            let edge = Histogram::upper_edge(index);
            assert_eq!(
                Histogram::bucket(edge),
                index,
                "bucket {index} has upper edge {edge}, which lands in a different bucket"
            );

            // And one microsecond past the edge is the next bucket, so the
            // bands are contiguous with no gap and no overlap.
            assert_eq!(
                Histogram::bucket(edge + 1),
                index + 1,
                "the sample after bucket {index}'s edge did not land in {}",
                index + 1
            );
        }
    }

    #[test]
    fn each_band_starts_where_the_last_one_ended() {
        // The band boundaries themselves, named rather than derived, so that a
        // mutant which moves a limit rather than a step is caught too.
        assert_eq!(Histogram::bucket(0), 0);
        assert_eq!(Histogram::bucket(1), 1);
        assert_eq!(Histogram::bucket(FINE_LIMIT - 1), FINE_BUCKETS - 1);
        assert_eq!(Histogram::bucket(FINE_LIMIT), FINE_BUCKETS);

        assert_eq!(
            Histogram::bucket(MEDIUM_LIMIT - 1),
            FINE_BUCKETS + MEDIUM_BUCKETS - 1
        );
        assert_eq!(
            Histogram::bucket(MEDIUM_LIMIT),
            FINE_BUCKETS + MEDIUM_BUCKETS
        );

        assert_eq!(Histogram::bucket(COARSE_LIMIT - 1), BUCKETS - 2);

        // Anything at or past the coarse limit is the overflow bucket: a proxy
        // taking a minute has a problem no percentile describes.
        assert_eq!(Histogram::bucket(COARSE_LIMIT), BUCKETS - 1);
        assert_eq!(Histogram::bucket(u64::MAX), BUCKETS - 1);

        // The overflow bucket's edge is the coarse limit itself and not the
        // arithmetic the band below it would produce. The round-trip test above
        // walks `0..BUCKETS - 1` and so never reaches this one, which is how
        // two mutants of the `bucket < BUCKETS - 1` guard survived it: both let
        // the overflow bucket fall into the coarse branch, which answers
        // 60,009,999 instead of 60,000,000.
        assert_eq!(Histogram::upper_edge(BUCKETS - 1), COARSE_LIMIT);
    }

    #[test]
    fn the_upper_edges_rise_without_repeating() {
        // A mutant that turns a subtraction into an addition can leave the
        // edges non-monotonic while every individual value still looks
        // plausible, and a percentile read off a non-monotonic table is
        // nonsense that reports as a number.
        let mut previous = Histogram::upper_edge(0);
        for index in 1..BUCKETS - 1 {
            let edge = Histogram::upper_edge(index);
            assert!(
                edge > previous,
                "bucket {index} has edge {edge}, not above the previous {previous}"
            );
            previous = edge;
        }
    }

    fn histogram_of(samples: &[u64]) -> Histogram {
        let mut histogram = Histogram::new();
        for sample in samples {
            histogram.record(*sample);
        }
        histogram
    }

    #[test]
    fn percentiles_of_a_known_set_are_the_hand_computed_ones() {
        // Ten samples, 100 through 1000. The median is the fifth, p90 the
        // ninth, p99 rounds up to the tenth. All below the fine limit, so all
        // exact rather than bucketed.
        let histogram = histogram_of(&[100, 200, 300, 400, 500, 600, 700, 800, 900, 1000]);

        assert_eq!(histogram.count(), 10);
        assert_eq!(histogram.percentile(0.50), 500);
        assert_eq!(histogram.percentile(0.90), 900);
        assert_eq!(histogram.percentile(0.99), 1000);
        assert_eq!(histogram.max(), 1000);
        assert_eq!(histogram.mean(), 550);
    }

    #[test]
    fn p99_is_the_ninety_ninth_of_a_hundred() {
        // The percentile the roadmap's target is stated in, on the set where
        // an off-by-one is visible: 1 through 100.
        let histogram = histogram_of(&(1..=100).collect::<Vec<u64>>());
        assert_eq!(histogram.percentile(0.99), 99);
        assert_eq!(histogram.percentile(0.50), 50);
        assert_eq!(histogram.percentile(1.0), 100);
    }

    #[test]
    fn one_slow_sample_does_not_move_the_median_but_does_move_the_max() {
        // The property that makes a percentile worth reporting at all.
        let mut samples = vec![100; 99];
        samples.push(5_000_000);
        let histogram = histogram_of(&samples);

        assert_eq!(histogram.percentile(0.50), 100);
        assert_eq!(histogram.max(), 5_000_000);
        assert!(histogram.percentile(0.99) >= 100);
    }

    #[test]
    fn a_sample_under_ten_milliseconds_is_exact() {
        // Where the added-latency target lives, so bucketing error here would
        // be error in the number the milestone is judged on.
        for sample in [0, 1, 999, 1_000, 9_999] {
            let histogram = histogram_of(&[sample]);
            assert_eq!(
                histogram.percentile(0.50),
                sample,
                "{sample} was not recorded exactly"
            );
        }
    }

    #[test]
    fn a_coarse_sample_is_never_reported_as_faster_than_it_was() {
        // Rounding down would let a bad tail look acceptable.
        for sample in [10_000_u64, 55_555, 999_999, 1_000_000, 12_345_678] {
            let histogram = histogram_of(&[sample]);
            let reported = histogram.percentile(0.50);
            assert!(
                reported >= sample.min(histogram.max()),
                "{sample} was reported as {reported}"
            );
            assert_eq!(histogram.max(), sample);
        }
    }

    #[test]
    fn a_sample_past_the_last_bucket_still_counts() {
        // A minute is past every bucket. Dropping it would make an
        // unresponsive run look like a fast one.
        let histogram = histogram_of(&[90_000_000]);
        assert_eq!(histogram.count(), 1);
        assert_eq!(histogram.max(), 90_000_000);
        assert_eq!(histogram.percentile(0.50), 90_000_000);
    }

    #[test]
    fn an_empty_histogram_reports_zero_rather_than_panicking() {
        let histogram = Histogram::new();
        assert_eq!(histogram.count(), 0);
        assert_eq!(histogram.percentile(0.99), 0);
        assert_eq!(histogram.mean(), 0);
        assert_eq!(histogram.max(), 0);
        assert_eq!(Histogram::default().count(), 0);
    }

    #[test]
    fn a_quantile_outside_zero_to_one_is_clamped() {
        let histogram = histogram_of(&[10, 20, 30]);
        assert_eq!(histogram.percentile(-1.0), 10);
        assert_eq!(histogram.percentile(2.0), 30);
        assert_eq!(rank_of(0.0, 10), 1);
        assert_eq!(rank_of(1.0, 10), 10);
    }

    #[test]
    fn every_bucket_reports_an_edge_at_or_above_its_samples() {
        // The invariant the whole structure rests on, checked across all three
        // tiers rather than at the two boundaries a test would think of.
        for sample in (0..2_000_000).step_by(9_973) {
            let bucket = Histogram::bucket(sample);
            assert!(
                Histogram::upper_edge(bucket) >= sample,
                "{sample} landed in bucket {bucket}, whose edge is below it"
            );
        }
    }

    fn report() -> Report {
        Report {
            target: "pgprox-1:6432".into(),
            workload_version: 1,
            seed: 42,
            connections: 1000,
            duration_ms: 10_000,
            transactions: 50_000,
            errors: 0,
            relocations: 0,
            first_error: None,
            outcomes: Outcomes::default(),
            latency: Latency::from(&histogram_of(&[100, 200, 300, 400, 500])),
        }
    }

    #[test]
    fn outcomes_count_by_code_and_keep_what_was_said() {
        let mut outcomes = Outcomes::default();
        outcomes.record(
            "53300",
            "upstream primary:5432 is at its connection cap of 60",
        );
        outcomes.record(
            "53300",
            "upstream primary:5432 is at its connection cap of 60",
        );
        outcomes.record("57014", "timed out after 5s waiting for a connection");

        assert_eq!(outcomes.total(), 3);
        assert_eq!(outcomes.get("53300").unwrap().count, 2);
        assert_eq!(outcomes.get("57014").unwrap().count, 1);
        assert!(outcomes.get("28000").is_none());
        assert!(!outcomes.is_empty());
    }

    #[test]
    fn one_code_from_two_places_stays_two_entries() {
        // The reason the messages are kept at all. A run against Postgres
        // directly is the case that needs it: `53300` there is the server's
        // own, and its text is not one fixed sentence. This proxy's two
        // `53300`s are deliberately identical to a client, which the test
        // below is about.
        let mut outcomes = Outcomes::default();
        outcomes.record("53300", "sorry, too many clients already");
        outcomes.record("53300", "remaining connection slots are reserved");
        outcomes.record("53300", "remaining connection slots are reserved");

        let refused = outcomes.get("53300").unwrap();
        assert_eq!(refused.count, 3);
        assert_eq!(refused.messages.len(), 2);
        assert_eq!(refused.messages["sorry, too many clients already"], 1);
        assert_eq!(
            refused.messages["remaining connection slots are reserved"],
            2
        );
    }

    #[test]
    fn the_proxys_two_refusals_are_one_entry_because_a_client_cannot_tell_them_apart() {
        // Not a limitation of this type. `ClientError::client_message` is
        // vague on purpose: an untrusted client must not learn an upstream
        // hostname or a connection cap. So a node at its client ceiling and a
        // fleet at its upstream cap send the same code and the same sentence,
        // and no amount of client-side recording separates them. The node's
        // own view is what has to, which is why `M11.6` needs both.
        let mut outcomes = Outcomes::default();
        outcomes.record("53300", "too many connections, please retry");
        outcomes.record("53300", "too many connections, please retry");

        let refused = outcomes.get("53300").unwrap();
        assert_eq!(refused.count, 2);
        assert_eq!(refused.messages.len(), 1);
    }

    #[test]
    fn a_code_whose_messages_are_all_different_stops_growing_but_keeps_counting() {
        // `57014` carries how long the caller waited, so every one of them is
        // a distinct string. Left unbounded this map would hold one entry per
        // failure, in the process that is measuring memory.
        let mut outcomes = Outcomes::default();
        for waited in 0..MESSAGE_VARIANTS * 4 {
            outcomes.record("57014", &format!("timed out after {waited}ms"));
        }

        let timed_out = outcomes.get("57014").unwrap();
        assert_eq!(timed_out.count, MESSAGE_VARIANTS as u64 * 4);
        assert_eq!(timed_out.messages.len(), MESSAGE_VARIANTS);
        assert_eq!(outcomes.total(), MESSAGE_VARIANTS as u64 * 4);
    }

    #[test]
    fn a_failure_with_no_sqlstate_is_counted_as_one() {
        // A socket that would not open has no code, and neither does a server
        // that sent an `ErrorResponse` with no `C` field. Both are real and
        // neither is a SQLSTATE.
        let mut outcomes = Outcomes::default();
        outcomes.record("", "server said nothing");
        outcomes.record(NO_SQLSTATE, "connect 127.0.0.1:1: refused");

        assert_eq!(outcomes.get(NO_SQLSTATE).unwrap().count, 2);
        assert_eq!(outcomes.total(), 2);
        assert!(outcomes.get("").is_none());
    }

    #[test]
    fn merging_sums_counts_without_replaying_them() {
        let mut left = Outcomes::default();
        left.record("53300", "full");
        left.record("53300", "full");
        let mut right = Outcomes::default();
        right.record("53300", "full");
        right.record(
            "57P01",
            "terminating connection due to administrator command",
        );

        left.merge(&right);

        assert_eq!(left.total(), 4);
        assert_eq!(left.get("53300").unwrap().count, 3);
        assert_eq!(left.get("53300").unwrap().messages["full"], 3);
        assert_eq!(left.get("57P01").unwrap().count, 1);
    }

    #[test]
    fn merging_keeps_the_count_of_messages_it_had_no_room_for() {
        // The cap drops messages, never failures. A merged report whose total
        // was short by the dropped ones would be a report that undercounts
        // exactly when a run went worst.
        let mut left = Outcomes::default();
        let mut right = Outcomes::default();
        for waited in 0..MESSAGE_VARIANTS * 2 {
            left.record("57014", &format!("left {waited}"));
            right.record("57014", &format!("right {waited}"));
        }

        left.merge(&right);

        assert_eq!(left.total(), MESSAGE_VARIANTS as u64 * 4);
        assert_eq!(left.get("57014").unwrap().messages.len(), MESSAGE_VARIANTS);
    }

    #[test]
    fn outcomes_serialise_with_the_code_as_the_key() {
        // A script reads this. Keyed by code rather than wrapped in a list of
        // objects, so `jq '.outcomes["53300"].count'` is the whole query.
        let mut report = report();
        report.outcomes.record(
            "53300",
            "upstream primary:5432 is at its connection cap of 60",
        );
        let json = report.to_json().unwrap();

        assert!(json.contains("\"outcomes\""), "{json}");
        assert!(json.contains("\"53300\""), "{json}");
        assert!(json.contains("\"count\": 1"), "{json}");
        assert!(json.contains("connection cap of 60"), "{json}");
    }

    #[test]
    fn a_clean_run_carries_an_empty_outcome_map_rather_than_nothing() {
        // Present and empty rather than absent: a script that reads
        // `.outcomes` on a clean run should get an answer, and "no failures"
        // is an answer.
        let json = report().to_json().unwrap();
        assert!(json.contains("\"outcomes\": {}"), "{json}");
        assert!(report().outcomes.is_empty());
        assert_eq!(report().outcomes.iter().count(), 0);
    }

    #[test]
    fn the_report_serialises_to_json_a_script_can_read() {
        // scripts/scale.sh reads these keys. A number a script scrapes out of
        // prose changes meaning the next time somebody edits the prose.
        let json = report().to_json().unwrap();
        for key in [
            "\"target\"",
            "\"workload_version\"",
            "\"seed\"",
            "\"connections\"",
            "\"errors\"",
            "\"relocations\"",
            "\"p99_us\"",
            "\"p50_us\"",
        ] {
            assert!(json.contains(key), "{key} missing from {json}");
        }
    }

    #[test]
    fn latency_comes_from_the_histogram_rather_than_being_set_by_hand() {
        let latency = Latency::from(&histogram_of(&[100, 200, 300, 400, 500]));
        assert_eq!(latency.count, 5);
        assert_eq!(latency.p50_us, 300);
        assert_eq!(latency.max_us, 500);
        assert_eq!(latency.mean_us, 300);
    }

    #[test]
    fn throughput_is_transactions_over_the_run() {
        let mut report = report();
        assert!((report.throughput() - 5000.0).abs() < 0.001);

        // A run that recorded no time reports no rate rather than dividing by
        // zero, which is what a run that failed to start looks like.
        report.duration_ms = 0;
        assert!((report.throughput() - 0.0).abs() < f64::EPSILON);
    }
}
