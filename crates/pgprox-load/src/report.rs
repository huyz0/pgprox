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

use serde::Serialize;

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
    /// What the first failure said, when there was one.
    ///
    /// A count on its own is not diagnosable: three errors in a run of sixteen
    /// thousand is either a proxy refusing connections or a client giving up
    /// on its own timeout, and those want opposite responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_error: Option<String>,
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
            latency: Latency::from(&histogram_of(&[100, 200, 300, 400, 500])),
        }
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
