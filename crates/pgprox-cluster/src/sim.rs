//! A deterministic simulation of a cluster.
//!
//! # Why this exists rather than integration tests
//!
//! The cap invariant must hold under partition, leader loss and simultaneous
//! restart. Those are the conditions that never reproduce in staging, so
//! finding the bugs requires causing them on purpose, thousands of times, in
//! milliseconds.
//!
//! # Determinism is the whole point
//!
//! Every source of nondeterminism is a parameter: time advances only when told,
//! and the network's delay, drop and reorder decisions come from a seeded
//! generator. A failing seed replays exactly, which is the difference between a
//! property test and an anecdote.
//!
//! Nothing here uses the system clock or the system RNG.

use std::collections::VecDeque;
use std::time::Duration;

use pgprox_core::ids::NodeId;

/// A reproducible pseudorandom source.
///
/// xorshift64*, chosen because it is a dozen lines and its sequence is fixed
/// forever. A better generator would buy nothing: this decides message delays,
/// not keys.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seeds the generator. Zero is remapped, since xorshift is absorbing at 0.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// The next value.
    pub const fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// A value in `0..bound`, or zero when `bound` is zero.
    pub const fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }

    /// True with probability `percent`.
    pub const fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// Virtual time, advanced only by the simulation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct SimTime(pub u64);

impl SimTime {
    /// Milliseconds since the simulation began.
    #[must_use]
    pub const fn millis(self) -> u64 {
        self.0
    }

    /// This instant plus a duration.
    ///
    /// Saturates rather than truncating. A duration too large for the
    /// millisecond counter comes from a property test exploring extremes, and
    /// wrapping there would send simulated time backwards, which would look
    /// like an invariant violation that never happened.
    #[must_use]
    pub fn plus(self, d: Duration) -> Self {
        let millis = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        Self(self.0.saturating_add(millis))
    }
}

/// One message in flight.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Envelope<M> {
    /// When it becomes deliverable.
    pub due: SimTime,
    /// Who sent it.
    pub from: NodeId,
    /// Who receives it.
    pub to: NodeId,
    /// The payload.
    pub message: M,
}

/// How the network misbehaves.
#[derive(Clone, Copy, Debug)]
pub struct NetworkFaults {
    /// Percent chance a message is dropped outright.
    pub drop_percent: u64,
    /// Maximum extra delay, in milliseconds.
    pub max_delay_ms: u64,
    /// Percent chance a message is delivered later than one sent after it.
    pub reorder_percent: u64,
}

impl Default for NetworkFaults {
    fn default() -> Self {
        // A perfect network by default, so a test opts into chaos explicitly
        // and a failure is attributable.
        Self {
            drop_percent: 0,
            max_delay_ms: 1,
            reorder_percent: 0,
        }
    }
}

/// A network that can delay, drop, reorder and partition.
#[derive(Debug)]
pub struct Network<M> {
    now: SimTime,
    rng: Rng,
    faults: NetworkFaults,
    queue: Vec<Envelope<M>>,
    /// Node pairs that cannot reach each other, as an unordered pair.
    partitions: Vec<(NodeId, NodeId)>,
    delivered: usize,
    dropped: usize,
}

impl<M: Clone> Network<M> {
    /// A network with the given seed and fault profile.
    #[must_use]
    pub fn new(seed: u64, faults: NetworkFaults) -> Self {
        Self {
            now: SimTime::default(),
            rng: Rng::new(seed),
            faults,
            queue: Vec::new(),
            partitions: Vec::new(),
            delivered: 0,
            dropped: 0,
        }
    }

    /// The current virtual time.
    #[must_use]
    pub const fn now(&self) -> SimTime {
        self.now
    }

    /// How many messages were delivered and dropped, for assertions about
    /// whether a test actually exercised what it claims.
    #[must_use]
    pub const fn stats(&self) -> (usize, usize) {
        (self.delivered, self.dropped)
    }

    /// Cuts every link between the two groups.
    ///
    /// A partition is symmetric here. Asymmetric reachability is a real failure
    /// mode but a different one, and modelling it silently under this name
    /// would make a test claim more than it checks.
    pub fn partition(&mut self, left: &[NodeId], right: &[NodeId]) {
        for a in left {
            for b in right {
                self.partitions.push((*a, *b));
            }
        }
    }

    /// Restores every link.
    pub fn heal(&mut self) {
        self.partitions.clear();
    }

    /// Whether two nodes can currently reach each other.
    #[must_use]
    pub fn reachable(&self, a: NodeId, b: NodeId) -> bool {
        !self
            .partitions
            .iter()
            .any(|(x, y)| (*x == a && *y == b) || (*x == b && *y == a))
    }

    /// Offers a message to the network.
    ///
    /// It may be dropped, delayed, or reordered relative to others.
    pub fn send(&mut self, from: NodeId, to: NodeId, message: M) {
        if !self.reachable(from, to) {
            self.dropped += 1;
            return;
        }
        if self.rng.chance(self.faults.drop_percent) {
            self.dropped += 1;
            return;
        }

        let mut delay = self.rng.below(self.faults.max_delay_ms.max(1));
        if self.rng.chance(self.faults.reorder_percent) {
            // Reordering is modelled as an extra delay rather than as queue
            // shuffling, so it composes with partitions and cannot resurrect a
            // message the partition already dropped.
            delay += self.rng.below(self.faults.max_delay_ms.max(1));
        }

        self.queue.push(Envelope {
            due: SimTime(self.now.0 + delay),
            from,
            to,
            message,
        });
    }

    /// Advances time and returns everything that became deliverable, in due
    /// order with ties broken by insertion order.
    pub fn advance(&mut self, by: Duration) -> VecDeque<Envelope<M>> {
        self.now = self.now.plus(by);

        // Stable sort: equal due times keep insertion order, so a run is a
        // function of the seed rather than of the sort implementation.
        self.queue.sort_by_key(|e| e.due);

        let split = self.queue.partition_point(|e| e.due <= self.now);
        let ready: VecDeque<_> = self.queue.drain(..split).collect();
        self.delivered += ready.len();
        ready
    }

    /// Messages still in flight.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.queue.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    /// `M14.15`. Six mutants survived in this file and none of them is a
    /// missing assertion about the cluster. They are all the same thing: the
    /// simulator itself could be broken and every test would still pass.
    ///
    /// That is worse than it sounds rather than better. This crate's headline
    /// claim, that the quota invariant holds across a randomized schedule set
    /// including partitions, rests entirely on this file actually randomizing
    /// and actually partitioning. Degrade the generator to a constant and every
    /// property test still passes while exploring one schedule. Nothing fails,
    /// and the evidence quietly stops being evidence.
    ///
    /// So the simulator is pinned as the contract it is, rather than written
    /// into the baseline as "a weaker search is not a defect".
    #[test]
    fn the_generator_produces_its_documented_sequence() {
        // A golden vector, for the reason `pgprox-auth` uses published vectors
        // for SCRAM: the only property that pins an algorithm is its output.
        //
        // The first attempt at this asserted that draws were distinct and that
        // `below` covered its range. Both passed under every mutation, because
        // only one of the four operations is mutated at a time and a
        // three-quarters-intact xorshift still looks random. Distinctness is a
        // property of almost any generator; being *this* generator is not.
        //
        // It matters because a simulator is only evidence if it is
        // reproducible. Two runs of the suite on different machines must
        // explore the same schedules, or a failure cannot be re-run and a pass
        // means less than it appears to.
        let mut rng = Rng::new(1);
        let drawn: Vec<u64> = (0..6).map(|_| rng.next_u64()).collect();
        assert_eq!(
            drawn,
            vec![
                1_082_269_761,
                1_152_992_998_833_853_505,
                11_177_516_664_432_764_457,
                17_678_023_832_001_937_445,
                9_659_130_143_999_365_733,
                17_775_799_001_133_815_809,
            ],
            "the generator is no longer the xorshift this file documents"
        );

        // A different seed is a different stream, so the seed is doing work.
        let mut other = Rng::new(7);
        let seven: Vec<u64> = (0..4).map(|_| other.next_u64()).collect();
        assert_eq!(
            seven,
            vec![
                7_575_888_327,
                8_070_950_887_952_051_652,
                13_931_920_357_059_763_743,
                8_698_583_309_276_795_107,
            ]
        );
    }

    #[test]
    fn the_generator_covers_the_range_it_is_asked_for() {
        // `below` is what picks delays and drop decisions, so a generator that
        // technically varies but lands in one place still collapses the search.
        let mut rng = Rng::new(7);
        let mut buckets = [0_u32; 10];
        for _ in 0..10_000 {
            buckets[usize::try_from(rng.below(10)).unwrap()] += 1;
        }
        assert!(
            buckets.iter().all(|c| *c > 500),
            "some value in 0..10 was drawn fewer than 500 times in 10,000: {buckets:?}"
        );
    }

    #[test]
    fn reachability_blocks_exactly_the_pairs_that_were_partitioned() {
        // `==` could become `!=` in `reachable`, which inverts the match and
        // makes the answer meaningless in both directions at once. The existing
        // tests assert that a partition drops messages, which the mutant also
        // satisfies, because it makes almost everything unreachable.
        let mut net: Network<u8> = Network::new(1, NetworkFaults::default());
        assert!(
            net.reachable(node(1), node(2)),
            "an unpartitioned pair was unreachable"
        );

        net.partition(&[node(1)], &[node(2)]);
        assert!(!net.reachable(node(1), node(2)));
        assert!(
            !net.reachable(node(2), node(1)),
            "a partition must be symmetric"
        );

        // The pairs that were not named stay reachable. This is the half the
        // mutant breaks and the existing tests never asked.
        assert!(net.reachable(node(1), node(3)));
        assert!(net.reachable(node(3), node(2)));
        assert!(net.reachable(node(3), node(4)));

        net.heal();
        assert!(net.reachable(node(1), node(2)));
    }

    #[test]
    fn a_dropped_message_is_counted_on_both_paths() {
        // `dropped += 1` could become `*=`, and the counter starts at zero, so
        // it would report nothing dropped forever. `stats` exists so a test can
        // assert it exercised what it claims, which makes a counter that never
        // moves worse than no counter: it makes those assertions vacuous.
        let mut net: Network<u8> = Network::new(1, NetworkFaults::default());
        assert_eq!(net.stats().1, 0);

        net.partition(&[node(1)], &[node(2)]);
        net.send(node(1), node(2), 0);
        assert_eq!(
            net.stats().1,
            1,
            "a message across a partition was not counted as dropped"
        );

        net.send(node(2), node(1), 0);
        net.send(node(1), node(2), 0);
        assert_eq!(net.stats().1, 3, "the drop counter stopped moving");

        // `send` increments the counter in two places: once for a partition and
        // once for the configured drop rate. The first version of this test only
        // reached the partition path, so the counter on the rate path could stay
        // at zero forever and nothing asked.
        let mut lossy: Network<u8> = Network::new(
            1,
            NetworkFaults {
                drop_percent: 100,
                ..NetworkFaults::default()
            },
        );
        lossy.send(node(1), node(2), 0);
        lossy.send(node(1), node(2), 0);
        assert_eq!(
            lossy.stats().1,
            2,
            "messages lost to the configured drop rate were not counted"
        );
        assert_eq!(lossy.in_flight(), 0);
    }

    #[test]
    fn a_reordered_message_adds_its_extra_delay_rather_than_multiplying_it() {
        // `delay += self.rng.below(..)` could become `*=`. Reordering is
        // modelled as a second delay drawn from the same range and added, and a
        // product is a different distribution that happens to look plausible:
        // it is often larger, and it collapses to zero whenever either draw is
        // zero, which silently un-reorders that message.
        //
        // Rather than assert a hard-coded schedule, this re-derives it from a
        // generator seeded the same way, replicating the draws `send` makes in
        // the order it makes them. That states the rule the code is supposed to
        // follow instead of blessing whatever it currently produces.
        let seed = 4;
        let faults = NetworkFaults {
            drop_percent: 0,
            max_delay_ms: 50,
            reorder_percent: 100,
        };
        let mut net: Network<u8> = Network::new(seed, faults);
        let mut oracle = Rng::new(seed);

        let mut expected = Vec::new();
        for i in 0..8_u8 {
            net.send(node(1), node(2), i);

            // Exactly what `send` draws on a reachable link, in order: the
            // drop roll, the base delay, the reorder roll, then the extra
            // delay. The drop roll is consumed and discarded because
            // `drop_percent` is zero here, and skipping it would put the
            // oracle out of step with the network from the first message.
            let _ = oracle.chance(faults.drop_percent);
            let base = oracle.below(faults.max_delay_ms.max(1));
            let reordered = oracle.chance(faults.reorder_percent);
            let extra = if reordered {
                oracle.below(faults.max_delay_ms.max(1))
            } else {
                0
            };
            expected.push(base + extra);
        }

        let mut due: Vec<u64> = net
            .advance(Duration::from_millis(1_000))
            .into_iter()
            .map(|e| e.due.0)
            .collect();
        due.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            due, expected,
            "delivery times do not match base + extra for every message"
        );

        // And the sum genuinely exceeds a single draw somewhere, so the test is
        // not passing on a run where every extra happened to be zero.
        assert!(
            expected.iter().any(|d| *d >= faults.max_delay_ms),
            "no message was delayed beyond one draw, so this run proves nothing"
        );
    }

    #[test]
    fn the_same_seed_produces_the_same_run() {
        // Without this, a failing seed is not reproducible and every property
        // test below is an anecdote.
        fn run(seed: u64) -> Vec<(u64, u16)> {
            let mut net = Network::new(
                seed,
                NetworkFaults {
                    drop_percent: 20,
                    max_delay_ms: 50,
                    reorder_percent: 30,
                },
            );
            let mut trace = Vec::new();
            for round in 0..50 {
                for n in 1..=5_u16 {
                    net.send(node(n), node(1 + (n % 5)), round);
                }
                for env in net.advance(Duration::from_millis(10)) {
                    trace.push((env.due.millis(), env.to.get()));
                }
            }
            trace
        }

        assert_eq!(run(42), run(42), "the same seed diverged");
        assert_ne!(run(42), run(43), "different seeds produced identical runs");
    }

    #[test]
    fn a_partition_drops_every_message_across_it() {
        let mut net: Network<u8> = Network::new(1, NetworkFaults::default());
        net.partition(&[node(1), node(2)], &[node(3)]);

        assert!(!net.reachable(node(1), node(3)));
        assert!(!net.reachable(node(3), node(1)), "partitions are symmetric");
        assert!(
            net.reachable(node(1), node(2)),
            "same side stayed reachable"
        );

        net.send(node(1), node(3), 0);
        net.send(node(3), node(1), 0);
        net.send(node(1), node(2), 0);

        let delivered = net.advance(Duration::from_millis(100));
        assert_eq!(delivered.len(), 1, "a message crossed the partition");
        assert_eq!(delivered[0].to, node(2));
    }

    #[test]
    fn healing_restores_delivery() {
        let mut net: Network<u8> = Network::new(1, NetworkFaults::default());
        net.partition(&[node(1)], &[node(2)]);
        net.send(node(1), node(2), 0);
        assert!(net.advance(Duration::from_millis(10)).is_empty());

        net.heal();
        net.send(node(1), node(2), 0);
        assert_eq!(net.advance(Duration::from_millis(10)).len(), 1);
    }

    #[test]
    fn a_partitioned_message_is_dropped_not_deferred() {
        // If it queued instead, healing would deliver a burst of stale messages
        // that a real partition would have lost, and the simulation would be
        // kinder than reality.
        let mut net: Network<u8> = Network::new(1, NetworkFaults::default());
        net.partition(&[node(1)], &[node(2)]);
        net.send(node(1), node(2), 0);
        net.heal();

        assert!(
            net.advance(Duration::from_millis(1000)).is_empty(),
            "a message survived the partition it was sent during"
        );
        assert_eq!(net.stats().1, 1, "it was not counted as dropped");
    }

    #[test]
    fn nothing_is_delivered_before_it_is_due() {
        let mut net: Network<u8> = Network::new(
            7,
            NetworkFaults {
                drop_percent: 0,
                max_delay_ms: 100,
                reorder_percent: 0,
            },
        );
        for _ in 0..20 {
            net.send(node(1), node(2), 0);
        }

        let now = net.now();
        for env in net.advance(Duration::from_millis(50)) {
            assert!(env.due.millis() <= now.millis() + 50, "delivered early");
        }
    }

    #[test]
    fn dropping_is_governed_by_the_configured_rate() {
        let mut net: Network<u8> = Network::new(
            99,
            NetworkFaults {
                drop_percent: 50,
                max_delay_ms: 1,
                reorder_percent: 0,
            },
        );
        for _ in 0..1_000 {
            net.send(node(1), node(2), 0);
        }
        net.advance(Duration::from_millis(10));

        let (delivered, dropped) = net.stats();
        assert!(
            (300..700).contains(&dropped),
            "dropped {dropped} of 1000 at a 50% rate"
        );
        assert_eq!(delivered + dropped, 1_000, "messages went missing");
    }

    #[test]
    fn a_perfect_network_drops_nothing() {
        // The default, so a test that does not opt into chaos gets none and a
        // failure is attributable.
        let mut net: Network<u8> = Network::new(3, NetworkFaults::default());
        for _ in 0..500 {
            net.send(node(1), node(2), 0);
        }
        net.advance(Duration::from_millis(100));
        assert_eq!(net.stats().1, 0, "the default network dropped a message");
    }

    #[test]
    fn time_advances_only_when_told() {
        let mut net: Network<u8> = Network::new(1, NetworkFaults::default());
        assert_eq!(net.now(), SimTime::default());

        for _ in 0..1_000 {
            net.send(node(1), node(2), 0);
        }
        assert_eq!(net.now(), SimTime::default(), "sending advanced the clock");

        net.advance(Duration::from_millis(5));
        assert_eq!(net.now().millis(), 5);
    }

    #[test]
    fn messages_still_in_flight_are_visible() {
        let mut net: Network<u8> = Network::new(
            1,
            NetworkFaults {
                drop_percent: 0,
                max_delay_ms: 100,
                reorder_percent: 0,
            },
        );
        for _ in 0..10 {
            net.send(node(1), node(2), 0);
        }
        assert_eq!(net.in_flight(), 10);
        net.advance(Duration::from_millis(200));
        assert_eq!(net.in_flight(), 0);
    }

    #[test]
    fn the_generator_is_stable_and_never_absorbs() {
        // A zero seed would freeze xorshift at zero forever, which would make
        // one seed silently useless.
        let mut zero = Rng::new(0);
        assert_ne!(zero.next_u64(), 0);

        let mut once = Rng::new(12345);
        let first: Vec<u64> = (0..5).map(|_| once.next_u64()).collect();
        let mut twice = Rng::new(12345);
        let repeated: Vec<u64> = (0..5).map(|_| twice.next_u64()).collect();
        assert_eq!(first, repeated);
    }

    #[test]
    fn below_and_chance_stay_in_range() {
        let mut rng = Rng::new(5);
        for _ in 0..1_000 {
            assert!(rng.below(10) < 10);
        }
        assert_eq!(rng.below(0), 0, "a zero bound must not divide by zero");
        assert!(!rng.chance(0), "a zero chance fired");
        assert!(rng.chance(100), "a certain chance did not fire");
    }
}
