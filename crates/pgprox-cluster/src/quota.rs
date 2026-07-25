//! How a connection cap is divided across nodes.
//!
//! # The shape of the answer
//!
//! Each live node gets a guaranteed share it may use with no coordination at
//! all. What is left over is a free pool the leader hands out as short-lived
//! leases. A node that wants more than its share asks; a node that is cut off
//! keeps its share and loses only what it leased.
//!
//! # The invariant
//!
//! `guaranteed × live_nodes + outstanding_leases ≤ cap`, always, for every
//! membership size, including while membership is disagreed upon.
//!
//! Integer division is what makes this safe rather than merely likely. The
//! division remainder is deliberately given to nobody, neither spread across
//! nodes nor added to the free pool, because either is how the sum creeps over
//! the cap.
//!
//! # Why the free pool ignores the remainder
//!
//! The free pool is `cap − floor(cap × fraction)`, which does not mention the
//! node count. That independence is the point. Leases outlive the membership
//! view they were granted under, so a pool that grew when membership shrank
//! would still be outstanding when membership grew back and the guaranteed
//! total rose to meet it. At cap 100 and fraction 0.5 that is a free pool of 52
//! granted at three nodes, still live against a guaranteed total of 50 at five,
//! for 102 against a cap of 100.
//!
//! The cost is at most `nodes − 1` connections left unused. The alternative is a
//! cap breach during ordinary scale-up, which is the failure this crate exists
//! to prevent.

use pgprox_core::ids::ServerId;

/// How a server's cap is split.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuotaSplit {
    /// Connections each live node may open without asking.
    pub guaranteed_per_node: u32,
    /// Connections the leader may lease out.
    ///
    /// Independent of [`QuotaSplit::nodes`], so a membership change never
    /// changes how much the leader may grant. See the module docs.
    pub free_pool: u32,
    /// The cap this was derived from.
    pub cap: u32,
    /// How many live nodes it was divided among.
    pub nodes: u32,
}

impl QuotaSplit {
    /// Total permitted if every node used its share and the pool were fully
    /// leased. Must never exceed the cap.
    ///
    /// Saturating, because the fields are public and a hand-built value could
    /// otherwise overflow. Saturating can only ever over-report, never
    /// under-report, so a caller comparing this against the cap still gets the
    /// safe answer.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.guaranteed_per_node
            .saturating_mul(self.nodes)
            .saturating_add(self.free_pool)
    }
}

/// Divides a cap between guaranteed shares and a leasable free pool.
///
/// `guaranteed_fraction` is clamped to `0.0..=1.0`. A value outside that range
/// is a configuration error caught elsewhere, and clamping here means a bad
/// config cannot produce a split that breaches the cap.
///
/// With zero live nodes everything goes to the free pool, which nobody can lease
/// because leasing requires a leader and a leader requires a live node. That is
/// the safe direction: no capacity is granted rather than all of it.
#[must_use]
pub fn split(cap: u32, nodes: u32, guaranteed_fraction: f64) -> QuotaSplit {
    if nodes == 0 {
        return QuotaSplit {
            guaranteed_per_node: 0,
            free_pool: cap,
            cap,
            nodes: 0,
        };
    }

    let fraction = guaranteed_fraction.clamp(0.0, 1.0);

    // Round down at every step. Whatever the division leaves over is reserved
    // for nobody: spreading it across nodes lets each act on it independently,
    // and adding it to the free pool makes the pool depend on the node count,
    // which is the membership-change breach described in the module docs.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let guaranteed_total = (f64::from(cap) * fraction).floor() as u32;
    let guaranteed_per_node = guaranteed_total / nodes;

    // Derived by subtraction, not by a second multiplication, so the two halves
    // cannot drift apart through rounding.
    let free_pool = cap - guaranteed_total;

    QuotaSplit {
        guaranteed_per_node,
        free_pool,
        cap,
        nodes,
    }
}

/// What one node believes it may hold for one server.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeAllowance {
    /// Which server.
    pub server: ServerId,
    /// The guaranteed share, usable without coordination.
    pub guaranteed: u32,
    /// Currently leased on top of that.
    pub leased: u32,
}

impl NodeAllowance {
    /// Everything this node may currently hold.
    ///
    /// Saturating for the same reason as [`QuotaSplit::total`]: over-reporting
    /// refuses a connection, under-reporting would permit one past the cap.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.guaranteed.saturating_add(self.leased)
    }

    /// Whether opening one more connection is permitted.
    #[must_use]
    pub const fn permits(&self, current: u32) -> bool {
        current < self.total()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_split_never_exceeds_the_cap_for_any_small_membership() {
        // Exhaustive where exhaustive is cheap. This is the invariant, and it
        // is worth checking every combination rather than sampling.
        for cap in 0_u32..=200 {
            for nodes in 0_u32..=12 {
                for percent in 0..=100 {
                    let fraction = f64::from(percent) / 100.0;
                    let s = split(cap, nodes, fraction);
                    assert!(
                        s.total() <= cap,
                        "cap={cap} nodes={nodes} fraction={fraction}: \
                         {} guaranteed x {} nodes + {} pool = {} > {cap}",
                        s.guaranteed_per_node,
                        s.nodes,
                        s.free_pool,
                        s.total()
                    );
                }
            }
        }
    }

    #[test]
    fn the_remainder_is_given_to_nobody() {
        // Distributing it is how the sum creeps over the cap: each node would
        // act on its extra independently. Putting it in the free pool is
        // subtler and is what `the_free_pool_does_not_move_with_membership`
        // covers.
        let s = split(100, 3, 0.5);
        assert_eq!(s.guaranteed_per_node, 16, "50 / 3 rounds down");
        assert_eq!(s.free_pool, 50, "the pool is cap minus the guaranteed half");
        assert_eq!(s.total(), 98, "two connections are deliberately stranded");
    }

    #[test]
    fn the_free_pool_does_not_move_with_membership() {
        // The regression. A pool that grew when membership shrank would still
        // be outstanding when membership grew back, because leases outlive the
        // view they were granted under. At cap 100 and fraction 0.5 that was a
        // pool of 52 granted at three nodes against a guaranteed total of 50 at
        // five nodes: 102 against a cap of 100.
        for cap in [10_u32, 100, 4096, 65535] {
            for fraction in [0.0, 0.25, 0.5, 0.8, 1.0] {
                let pools: Vec<u32> = (1_u32..=12)
                    .map(|n| split(cap, n, fraction).free_pool)
                    .collect();
                assert!(
                    pools.windows(2).all(|w| w[0] == w[1]),
                    "cap={cap} fraction={fraction}: free pool moved with membership: {pools:?}"
                );
            }
        }
    }

    #[test]
    fn the_worst_case_waste_is_one_connection_short_of_the_node_count() {
        // What the membership-independent pool costs. At a realistic cap this
        // is four connections out of thousands; the alternative is a breach
        // during ordinary scale-up.
        for cap in [1_u32, 7, 99, 100, 4096, 65535] {
            for nodes in 1_u32..=9 {
                let s = split(cap, nodes, 0.5);
                let stranded = cap - s.total();
                assert!(
                    stranded < nodes,
                    "cap={cap} nodes={nodes} stranded {stranded} connections"
                );
            }
        }
    }

    #[test]
    fn a_fraction_of_zero_puts_everything_in_the_pool() {
        // Every connection then requires a lease, which is slower but never
        // over-subscribed.
        let s = split(100, 5, 0.0);
        assert_eq!(s.guaranteed_per_node, 0);
        assert_eq!(s.free_pool, 100);
    }

    #[test]
    fn a_fraction_of_one_leaves_nothing_to_lease() {
        // Every connection is guaranteed, so there is no leader involvement at
        // all. The division remainder is stranded rather than leasable.
        let s = split(100, 3, 1.0);
        assert_eq!(s.guaranteed_per_node, 33);
        assert_eq!(s.free_pool, 0, "a fully guaranteed cap has no free pool");
        assert_eq!(s.total(), 99);
    }

    #[test]
    fn an_out_of_range_fraction_is_clamped_rather_than_trusted() {
        // Configuration validation rejects these, but a split that breached the
        // cap because of a bad config would be the worst possible failure mode
        // for the one property with no graceful degradation.
        for bad in [-5.0, -0.001, 1.001, 100.0, f64::INFINITY] {
            let s = split(100, 4, bad);
            assert!(s.total() <= 100, "fraction {bad} produced {}", s.total());
        }
    }

    #[test]
    fn a_nan_fraction_does_not_breach_the_cap() {
        // clamp propagates NaN, so this checks the arithmetic downstream of it
        // rather than assuming clamp saved us.
        let s = split(100, 4, f64::NAN);
        assert!(s.total() <= 100, "NaN produced {}", s.total());
    }

    #[test]
    fn no_live_nodes_grants_nothing_rather_than_everything() {
        // The free pool needs a leader to hand it out, and a leader needs a
        // live node, so this is unusable capacity rather than free capacity.
        let s = split(100, 0, 0.5);
        assert_eq!(s.guaranteed_per_node, 0);
        assert_eq!(s.total(), 100);
        assert_eq!(s.nodes, 0);
    }

    #[test]
    fn a_cap_smaller_than_the_node_count_guarantees_nothing() {
        // Five nodes and three connections: nobody gets a guaranteed share, and
        // what is left is leased one at a time. Slow, and never over the cap.
        let s = split(3, 5, 0.5);
        assert_eq!(s.guaranteed_per_node, 0);
        assert_eq!(s.free_pool, 2, "floor(3 x 0.5) is stranded");
        assert!(s.total() <= 3);
    }

    #[test]
    fn a_zero_cap_grants_nothing() {
        let s = split(0, 5, 0.5);
        assert_eq!(s.total(), 0);
        assert_eq!(s.guaranteed_per_node, 0);
        assert_eq!(s.free_pool, 0);
    }

    #[test]
    fn losing_a_node_never_raises_the_total_above_the_cap() {
        // The transient that matters: two nodes disagree about membership, so
        // one computes a split for N and the other for N-1. Neither may exceed
        // the cap on its own, since they act independently.
        for cap in [10_u32, 100, 4096] {
            for nodes in 2_u32..=10 {
                let before = split(cap, nodes, 0.5);
                let after = split(cap, nodes - 1, 0.5);
                assert!(before.total() <= cap);
                assert!(after.total() <= cap);
                assert!(
                    after.guaranteed_per_node >= before.guaranteed_per_node,
                    "a smaller cluster should not shrink each node's share"
                );
            }
        }
    }

    #[test]
    fn a_hand_built_split_cannot_overflow_the_total() {
        // The fields are public, so nothing stops a caller assembling one
        // directly. Saturating means that reports an absurd total rather than
        // panicking or wrapping to a small number that would look under the cap.
        let absurd = QuotaSplit {
            guaranteed_per_node: u32::MAX,
            free_pool: u32::MAX,
            cap: 10,
            nodes: u32::MAX,
        };
        assert_eq!(absurd.total(), u32::MAX);
        assert!(absurd.total() > absurd.cap, "an absurd split looked safe");
    }

    #[test]
    fn an_allowance_permits_up_to_its_total_and_no_further() {
        let allowance = NodeAllowance {
            server: ServerId::new("db-1", 5432),
            guaranteed: 10,
            leased: 5,
        };
        assert_eq!(allowance.total(), 15);
        assert!(allowance.permits(14), "one below the total was refused");
        assert!(!allowance.permits(15), "the total itself was permitted");
        assert!(!allowance.permits(100));
    }

    #[test]
    fn an_allowance_with_nothing_permits_nothing() {
        let allowance = NodeAllowance {
            server: ServerId::new("db-1", 5432),
            guaranteed: 0,
            leased: 0,
        };
        assert!(
            !allowance.permits(0),
            "a zero allowance permitted a connection"
        );
    }
}
