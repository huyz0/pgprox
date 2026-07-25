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
//! remainder is deliberately left in the free pool instead of being distributed,
//! because distributing it is exactly how the sum creeps over the cap.

use pgprox_core::ids::ServerId;

/// How a server's cap is split.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuotaSplit {
    /// Connections each live node may open without asking.
    pub guaranteed_per_node: u32,
    /// Connections the leader may lease out, including the division remainder.
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

    // Round down at every step. The remainder stays in the free pool, where it
    // is handed out one lease at a time under the leader's accounting, rather
    // than being spread across nodes that each act on it independently.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let guaranteed_total = (f64::from(cap) * fraction).floor() as u32;
    let guaranteed_per_node = guaranteed_total / nodes;

    // Derived by subtraction, not by a second multiplication, so the two halves
    // cannot drift apart through rounding.
    let free_pool = cap - guaranteed_per_node * nodes;

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
    fn the_remainder_stays_in_the_free_pool() {
        // Distributing it is how the sum creeps over the cap: each node would
        // act on its extra independently.
        let s = split(100, 3, 0.5);
        assert_eq!(s.guaranteed_per_node, 16, "50 / 3 rounds down");
        assert_eq!(s.free_pool, 100 - 48, "the remainder went to the pool");
        assert_eq!(s.total(), 100, "nothing was lost or invented");
    }

    #[test]
    fn nothing_is_lost_to_rounding() {
        // The free pool is derived by subtraction rather than a second
        // multiplication, so the halves cannot drift apart.
        for cap in [1_u32, 7, 99, 100, 4096, 65535] {
            for nodes in 1_u32..=9 {
                let s = split(cap, nodes, 0.5);
                assert_eq!(
                    s.guaranteed_per_node * nodes + s.free_pool,
                    cap,
                    "cap={cap} nodes={nodes} did not account for every connection"
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
    fn a_fraction_of_one_still_leaves_the_remainder_leasable() {
        let s = split(100, 3, 1.0);
        assert_eq!(s.guaranteed_per_node, 33);
        assert_eq!(s.free_pool, 1, "99 guaranteed, one left over");
        assert_eq!(s.total(), 100);
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
        // all three are leased one at a time. Slow, and never over the cap.
        let s = split(3, 5, 0.5);
        assert_eq!(s.guaranteed_per_node, 0);
        assert_eq!(s.free_pool, 3);
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
