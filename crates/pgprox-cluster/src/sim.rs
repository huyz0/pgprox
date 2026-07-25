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
