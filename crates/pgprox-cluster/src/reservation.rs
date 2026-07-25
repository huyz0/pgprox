//! Tenant reservations, use it or lose it.
//!
//! ADR 0005 decided affinity by quota reservation rather than by moving
//! clients. A tenant's home node reserves most of that tenant's upstream
//! budget; other nodes share the rest and, on hitting it, multiplex harder
//! rather than opening new connections.
//!
//! # Why reservations decay
//!
//! A home node that never uses its reservation would otherwise hold capacity
//! hostage forever. The tenant might be idle, or the load balancer might never
//! send it there. Either way, peers that could use the capacity cannot, and
//! nothing ever corrects it.
//!
//! Decay is what makes the reservation a hint rather than a lock.

use std::collections::HashMap;

use pgprox_core::ids::{NodeId, TenantId};

/// How reservations are tuned.
#[derive(Clone, Copy, Debug)]
pub struct ReservationConfig {
    /// Fraction of a tenant's budget its home node may reserve.
    pub home_share: f64,
    /// Gossip rounds of non-use before a reservation decays.
    pub decay_rounds: u32,
}

impl Default for ReservationConfig {
    fn default() -> Self {
        Self {
            home_share: 0.8,
            decay_rounds: 3,
        }
    }
}

/// What one node is entitled to for one tenant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TenantEntitlement {
    /// Upstream connections this node may open for the tenant.
    pub allowed: u32,
    /// Whether this node is the tenant's home.
    pub is_home: bool,
}

/// Tracks reservations and their decay.
#[derive(Debug, Default)]
pub struct Reservations {
    config: ReservationConfig,
    /// Consecutive rounds a home node has gone without using its reservation.
    idle_rounds: HashMap<TenantId, u32>,
}

impl Reservations {
    /// A tracker with the given configuration.
    #[must_use]
    pub fn new(config: ReservationConfig) -> Self {
        Self {
            config,
            idle_rounds: HashMap::new(),
        }
    }

    /// Records a gossip round's observation of a tenant's home-node usage.
    ///
    /// Any use at all resets the counter. A reservation is about whether the
    /// home node is active for this tenant, not about how heavily.
    pub fn observe(&mut self, tenant: &TenantId, home_usage: u32) {
        let entry = self.idle_rounds.entry(tenant.clone()).or_insert(0);
        if home_usage > 0 {
            *entry = 0;
        } else {
            *entry = entry.saturating_add(1);
        }
    }

    /// Whether a tenant's reservation has decayed.
    #[must_use]
    pub fn has_decayed(&self, tenant: &TenantId) -> bool {
        self.idle_rounds
            .get(tenant)
            .is_some_and(|rounds| *rounds >= self.config.decay_rounds)
    }

    /// Consecutive idle rounds observed, for diagnostics.
    #[must_use]
    pub fn idle_rounds(&self, tenant: &TenantId) -> u32 {
        self.idle_rounds.get(tenant).copied().unwrap_or(0)
    }

    /// What `node` may open for `tenant`, given the tenant's total budget.
    ///
    /// A decayed reservation splits the budget evenly, so peers can claim the
    /// slack without the home node losing everything: it remains one of the
    /// claimants.
    #[must_use]
    pub fn entitlement(
        &self,
        tenant: &TenantId,
        node: NodeId,
        home: Option<NodeId>,
        budget: u32,
        peers: u32,
    ) -> TenantEntitlement {
        let is_home = home == Some(node);
        let peers = peers.max(1);

        if home.is_none() || self.has_decayed(tenant) {
            // No home, or the reservation lapsed. Even split, rounded down, so
            // the parts can never sum above the budget.
            return TenantEntitlement {
                allowed: budget / peers,
                is_home,
            };
        }

        let share = self.config.home_share.clamp(0.0, 1.0);
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let reserved = (f64::from(budget) * share).floor() as u32;

        if is_home {
            return TenantEntitlement {
                allowed: reserved,
                is_home,
            };
        }

        // Everyone else divides what the home node did not reserve. Rounded
        // down, and the remainder is simply unused rather than handed to
        // someone, because handing it out is how the sum exceeds the budget.
        let others = peers.saturating_sub(1).max(1);
        TenantEntitlement {
            allowed: budget.saturating_sub(reserved) / others,
            is_home,
        }
    }

    /// Forgets a tenant, for eviction when a grant lapses.
    pub fn forget(&mut self, tenant: &TenantId) {
        self.idle_rounds.remove(tenant);
    }

    /// How many tenants are being tracked.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.idle_rounds.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tenant() -> TenantId {
        TenantId::new("acme")
    }

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    #[test]
    fn the_home_node_gets_the_reserved_share() {
        let r = Reservations::new(ReservationConfig::default());
        let e = r.entitlement(&tenant(), node(1), Some(node(1)), 100, 5);
        assert!(e.is_home);
        assert_eq!(e.allowed, 80, "0.8 of 100");
    }

    #[test]
    fn peers_divide_what_the_home_node_did_not_reserve() {
        let r = Reservations::new(ReservationConfig::default());
        // Five nodes, so four peers share the remaining 20.
        let e = r.entitlement(&tenant(), node(2), Some(node(1)), 100, 5);
        assert!(!e.is_home);
        assert_eq!(e.allowed, 5, "20 split four ways");
    }

    #[test]
    fn the_parts_never_sum_above_the_budget() {
        // The property that matters. Rounding down everywhere means the
        // remainder goes unused rather than to someone who would act on it.
        for budget in 0_u32..=200 {
            for peers in 1_u32..=8 {
                for share_pct in 0..=100 {
                    let r = Reservations::new(ReservationConfig {
                        home_share: f64::from(share_pct) / 100.0,
                        decay_rounds: 3,
                    });
                    let home = r
                        .entitlement(&tenant(), node(1), Some(node(1)), budget, peers)
                        .allowed;
                    let peer = r
                        .entitlement(&tenant(), node(2), Some(node(1)), budget, peers)
                        .allowed;
                    let total = home + peer * peers.saturating_sub(1);
                    assert!(
                        total <= budget,
                        "budget={budget} peers={peers} share={share_pct}%: \
                         {home} + {peer} x {} = {total}",
                        peers - 1
                    );
                }
            }
        }
    }

    #[test]
    fn a_reservation_decays_after_the_configured_idle_rounds() {
        let mut r = Reservations::new(ReservationConfig {
            home_share: 0.8,
            decay_rounds: 3,
        });

        for round in 1..3 {
            r.observe(&tenant(), 0);
            assert!(!r.has_decayed(&tenant()), "decayed after {round} rounds");
        }
        r.observe(&tenant(), 0);
        assert!(r.has_decayed(&tenant()), "did not decay after three rounds");
    }

    #[test]
    fn any_use_at_all_resets_the_decay() {
        // A reservation is about whether the home node is active for this
        // tenant, not how heavily. One connection is activity.
        let mut r = Reservations::new(ReservationConfig::default());
        for _ in 0..10 {
            r.observe(&tenant(), 0);
        }
        assert!(r.has_decayed(&tenant()));

        r.observe(&tenant(), 1);
        assert!(
            !r.has_decayed(&tenant()),
            "a single connection did not reset it"
        );
        assert_eq!(r.idle_rounds(&tenant()), 0);
    }

    #[test]
    fn a_decayed_reservation_splits_evenly_and_the_home_node_keeps_a_share() {
        // Peers claim the slack, but the home node remains a claimant: it may
        // become active again at any moment, and stripping it entirely would
        // make the next connection there fail.
        let mut r = Reservations::new(ReservationConfig {
            home_share: 0.8,
            decay_rounds: 1,
        });
        r.observe(&tenant(), 0);
        assert!(r.has_decayed(&tenant()));

        let home = r.entitlement(&tenant(), node(1), Some(node(1)), 100, 4);
        let peer = r.entitlement(&tenant(), node(2), Some(node(1)), 100, 4);

        assert_eq!(home.allowed, 25, "the home node kept an even share");
        assert_eq!(peer.allowed, 25, "peers claimed the slack");
        assert!(home.is_home, "it is still the home node");
    }

    #[test]
    fn a_tenant_with_no_home_splits_evenly() {
        // Every node draining, so rendezvous hashing has nobody to pick. The
        // budget still has to be divisible.
        let r = Reservations::new(ReservationConfig::default());
        let e = r.entitlement(&tenant(), node(2), None, 100, 4);
        assert_eq!(e.allowed, 25);
        assert!(!e.is_home);
    }

    #[test]
    fn a_single_node_cluster_gives_the_home_node_its_share() {
        // One peer means the "others" divisor would be zero. It must not
        // divide by zero, and the home node's own reservation still applies.
        let r = Reservations::new(ReservationConfig::default());
        let e = r.entitlement(&tenant(), node(1), Some(node(1)), 100, 1);
        assert_eq!(e.allowed, 80);

        // A non-home node in a one-node cluster is a contradiction gossip can
        // briefly produce. It must not panic.
        let odd = r.entitlement(&tenant(), node(2), Some(node(1)), 100, 1);
        assert!(odd.allowed <= 100);
    }

    #[test]
    fn zero_peers_is_treated_as_one_rather_than_dividing_by_zero() {
        let r = Reservations::new(ReservationConfig::default());
        let e = r.entitlement(&tenant(), node(1), Some(node(1)), 100, 0);
        assert!(e.allowed <= 100);
    }

    #[test]
    fn an_out_of_range_share_cannot_exceed_the_budget() {
        for bad in [-1.0, 1.5, f64::NAN, f64::INFINITY] {
            let r = Reservations::new(ReservationConfig {
                home_share: bad,
                decay_rounds: 3,
            });
            let e = r.entitlement(&tenant(), node(1), Some(node(1)), 100, 4);
            assert!(e.allowed <= 100, "share {bad} allowed {}", e.allowed);
        }
    }

    #[test]
    fn a_zero_budget_entitles_nobody() {
        let r = Reservations::new(ReservationConfig::default());
        assert_eq!(
            r.entitlement(&tenant(), node(1), Some(node(1)), 0, 5)
                .allowed,
            0
        );
        assert_eq!(
            r.entitlement(&tenant(), node(2), Some(node(1)), 0, 5)
                .allowed,
            0
        );
    }

    #[test]
    fn tenants_are_tracked_independently_and_can_be_forgotten() {
        let mut r = Reservations::new(ReservationConfig {
            home_share: 0.8,
            decay_rounds: 1,
        });
        let other = TenantId::new("globex");

        r.observe(&tenant(), 0);
        assert!(r.has_decayed(&tenant()));
        assert!(!r.has_decayed(&other), "decay leaked between tenants");
        assert_eq!(r.tracked(), 1);

        r.forget(&tenant());
        assert!(!r.has_decayed(&tenant()));
        assert_eq!(r.tracked(), 0);
    }

    #[test]
    fn an_unobserved_tenant_has_not_decayed() {
        // The default must be "reservation intact", or a tenant would lose its
        // reservation before anyone had a chance to observe it.
        let r = Reservations::new(ReservationConfig::default());
        assert!(!r.has_decayed(&tenant()));
        assert_eq!(r.idle_rounds(&tenant()), 0);
    }

    #[test]
    fn idle_rounds_saturate_rather_than_wrapping() {
        // A tenant idle for a very long time must not wrap back to zero and
        // silently regain its reservation.
        let mut r = Reservations::new(ReservationConfig {
            home_share: 0.8,
            decay_rounds: 3,
        });
        for _ in 0..1_000 {
            r.observe(&tenant(), 0);
        }
        assert!(r.has_decayed(&tenant()));
        assert_eq!(r.idle_rounds(&tenant()), 1_000);
    }
}
