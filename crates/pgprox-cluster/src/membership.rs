//! Who is alive, decided by when we last heard from them.
//!
//! [`crate::digest::DigestStore`] holds what peers said. This holds when they
//! said it, which is the part the quota rules actually depend on: a node that
//! has stopped receiving gossip must stop believing it leads, or a partition
//! produces two leaders granting from the same free pool.
//!
//! # Heard from, not sent to
//!
//! Liveness is counted from digests that arrived. That is deliberate and it is
//! what makes a one-way network failure safe: a node that can still send but no
//! longer receives ages its peers out and steps down, exactly as a node cut off
//! in both directions does. Counting sends instead would leave it convinced it
//! still leads while the other side elected a replacement.
//!
//! # Three states, and why suspect exists
//!
//! A suspect node still counts toward the membership view but not toward
//! quorum. So during the suspicion window the lowest suspect node still holds
//! office, and nobody else can take it, while quorum has already lapsed and
//! nobody grants at all. Both sides of a flapping link therefore pause rather
//! than race, which is the under-subscription this crate prefers.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use pgprox_core::cluster::{Member, MembershipView, NodeMode};
use pgprox_core::ids::NodeId;

/// How long silence is tolerated before a peer is doubted, then dropped.
#[derive(Clone, Copy, Debug)]
pub struct MembershipConfig {
    /// Silence after which a peer stops counting toward quorum.
    pub suspect_after: Duration,
    /// Silence after which a peer leaves the view entirely.
    pub dead_after: Duration,
}

impl Default for MembershipConfig {
    fn default() -> Self {
        // Roughly three and ten gossip rounds at the one-second protocol period
        // in ADR 0004. Long enough that a single dropped round is not a failure,
        // short enough that capacity returns well inside a human's attention.
        Self {
            suspect_after: Duration::from_secs(3),
            dead_after: Duration::from_secs(10),
        }
    }
}

impl MembershipConfig {
    /// Whether the two windows are ordered sensibly.
    ///
    /// Public so configuration validation can reject an inversion rather than
    /// discovering it as a node that goes straight from alive to dead.
    #[must_use]
    pub fn is_safe(&self) -> bool {
        self.suspect_after <= self.dead_after
    }
}

/// How much confidence we have in a peer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum NodeState {
    /// Heard from recently.
    Alive,
    /// Silent long enough to doubt, not long enough to drop.
    Suspect,
    /// Silent long enough to drop.
    Dead,
}

/// One peer's last contact.
#[derive(Clone, Copy, Debug)]
struct Contact {
    mode: NodeMode,
    at: Instant,
}

/// Liveness for the cluster, from this node's point of view.
#[derive(Debug)]
pub struct Membership {
    local: NodeId,
    config: MembershipConfig,
    peers: HashMap<NodeId, Contact>,
}

impl Membership {
    /// Tracks liveness for `local`.
    ///
    /// The local node is not seeded as alive. It becomes alive on the first
    /// [`Self::heard`] for itself, which the gossip loop issues every round.
    /// Seeding it would make a node that has stopped running its own loop look
    /// healthy to itself forever.
    #[must_use]
    pub fn new(local: NodeId, config: MembershipConfig) -> Self {
        Self {
            local,
            config,
            peers: HashMap::new(),
        }
    }

    /// Which node this is.
    #[must_use]
    pub const fn local(&self) -> NodeId {
        self.local
    }

    /// Records contact from a node.
    ///
    /// Contact only ever moves forward, so a gossip message that took a long
    /// path cannot revive a node that a fresher message already aged out.
    pub fn heard(&mut self, node: NodeId, mode: NodeMode, at: Instant) {
        self.peers
            .entry(node)
            .and_modify(|c| {
                if at >= c.at {
                    c.at = at;
                    c.mode = mode;
                }
            })
            .or_insert(Contact { mode, at });
    }

    /// Drops a node immediately, as an explicit leave announcement does.
    pub fn forget(&mut self, node: NodeId) {
        self.peers.remove(&node);
    }

    /// How much confidence we have in a node right now.
    #[must_use]
    pub fn state(&self, node: NodeId, now: Instant) -> NodeState {
        let Some(contact) = self.peers.get(&node) else {
            return NodeState::Dead;
        };
        // `saturating_duration_since` rather than subtraction: a contact stamped
        // in the future, which a reordered message can produce, reads as zero
        // silence rather than panicking.
        let silence = now.saturating_duration_since(contact.at);
        if silence >= self.config.dead_after {
            NodeState::Dead
        } else if silence >= self.config.suspect_after {
            NodeState::Suspect
        } else {
            NodeState::Alive
        }
    }

    /// The membership view, excluding nodes that have gone dead.
    ///
    /// Suspects are included. They may still be serving, and excluding them
    /// early would hand their share to everyone else while they are still
    /// spending it.
    #[must_use]
    pub fn view(&self, now: Instant) -> MembershipView {
        let members = self
            .peers
            .iter()
            .filter(|(id, _)| self.state(**id, now) != NodeState::Dead)
            .map(|(id, contact)| Member {
                id: *id,
                mode: contact.mode,
            })
            .collect();
        MembershipView::new(self.local, members)
    }

    /// How many nodes are alive and taking work.
    ///
    /// The quorum count. Suspects are excluded here even though the view keeps
    /// them, so doubt suspends granting before it changes anyone's share.
    #[must_use]
    pub fn alive_count(&self, now: Instant) -> usize {
        self.peers
            .iter()
            .filter(|(id, contact)| {
                contact.mode == NodeMode::Active && self.state(**id, now) == NodeState::Alive
            })
            .count()
    }

    /// Drops dead nodes. Housekeeping only: [`Self::view`] and
    /// [`Self::alive_count`] already ignore them, so skipping this cannot make
    /// a dead node count.
    pub fn reap(&mut self, now: Instant) {
        let dead: Vec<NodeId> = self
            .peers
            .keys()
            .copied()
            .filter(|id| self.state(*id, now) == NodeState::Dead)
            .collect();
        for id in dead {
            self.peers.remove(&id);
        }
    }

    /// How many nodes are tracked, dead ones included until reaped.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.peers.len()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn tracker(now: Instant, count: u16) -> Membership {
        let mut m = Membership::new(node(1), MembershipConfig::default());
        for n in 1..=count {
            m.heard(node(n), NodeMode::Active, now);
        }
        m
    }

    #[test]
    fn a_node_just_heard_from_is_alive() {
        let now = Instant::now();
        let m = tracker(now, 3);
        assert_eq!(m.state(node(2), now), NodeState::Alive);
        assert_eq!(m.alive_count(now), 3);
        assert_eq!(m.view(now).members().len(), 3);
    }

    #[test]
    fn a_node_never_heard_from_is_dead_rather_than_absent() {
        // Returning Dead rather than an Option means a caller cannot forget to
        // handle the unknown case, and the unknown case is the safe one.
        let now = Instant::now();
        let m = tracker(now, 3);
        assert_eq!(m.state(node(99), now), NodeState::Dead);
    }

    #[test]
    fn silence_moves_a_node_through_suspect_to_dead() {
        let config = MembershipConfig::default();
        let start = Instant::now();
        let m = tracker(start, 3);

        assert_eq!(m.state(node(2), start), NodeState::Alive);
        assert_eq!(
            m.state(node(2), start + config.suspect_after),
            NodeState::Suspect,
            "the suspect window is inclusive at its boundary"
        );
        assert_eq!(
            m.state(node(2), start + config.dead_after),
            NodeState::Dead,
            "the dead window is inclusive at its boundary"
        );
    }

    #[test]
    fn a_suspect_stays_in_the_view_but_leaves_the_quorum_count() {
        // The ordering that makes a flapping link pause both sides rather than
        // race them: granting stops before any share changes.
        let config = MembershipConfig::default();
        let start = Instant::now();
        let mut m = tracker(start, 3);
        m.heard(node(1), NodeMode::Active, start + config.suspect_after);

        let now = start + config.suspect_after;
        assert_eq!(m.state(node(2), now), NodeState::Suspect);
        assert_eq!(m.view(now).members().len(), 3, "a suspect left the view");
        assert_eq!(m.alive_count(now), 1, "a suspect counted toward quorum");
    }

    #[test]
    fn a_dead_node_leaves_the_view() {
        let config = MembershipConfig::default();
        let start = Instant::now();
        let mut m = tracker(start, 3);
        m.heard(node(1), NodeMode::Active, start + config.dead_after);

        let now = start + config.dead_after;
        assert_eq!(m.view(now).members().len(), 1);
        assert_eq!(m.view(now).leader(), Some(node(1)));
    }

    #[test]
    fn hearing_again_revives_a_suspect() {
        let config = MembershipConfig::default();
        let start = Instant::now();
        let mut m = tracker(start, 3);
        let now = start + config.suspect_after;
        assert_eq!(m.state(node(2), now), NodeState::Suspect);

        m.heard(node(2), NodeMode::Active, now);
        assert_eq!(m.state(node(2), now), NodeState::Alive);
    }

    #[test]
    fn a_stale_message_cannot_revive_a_node_a_fresher_one_aged_out() {
        // Gossip reorders. Contact must only move forward, or a message that
        // took a slow path would resurrect a node the cluster has moved past.
        let config = MembershipConfig::default();
        let start = Instant::now();
        let mut m = tracker(start, 2);
        let later = start + config.dead_after;

        m.heard(node(2), NodeMode::Active, later);
        m.heard(node(2), NodeMode::Active, start);
        assert_eq!(
            m.state(node(2), later),
            NodeState::Alive,
            "a stale message rolled contact backwards"
        );
    }

    #[test]
    fn a_contact_stamped_in_the_future_reads_as_no_silence() {
        // Reordering can produce this. It must not panic, and treating it as
        // fresh is the direction that does not invent a failure.
        let start = Instant::now();
        let mut m = Membership::new(node(1), MembershipConfig::default());
        m.heard(node(2), NodeMode::Active, start + Duration::from_secs(60));
        assert_eq!(m.state(node(2), start), NodeState::Alive);
    }

    #[test]
    fn a_draining_node_stays_in_the_view_and_leaves_the_quorum_count() {
        // It is still holding connections, so it is still a member. It is not
        // taking work, so it cannot be counted on to hold up a quorum.
        let now = Instant::now();
        let mut m = tracker(now, 3);
        m.heard(node(3), NodeMode::Draining, now);

        assert_eq!(m.view(now).members().len(), 3);
        assert_eq!(m.alive_count(now), 2);
        assert_eq!(m.view(now).active_count(), 2);
    }

    #[test]
    fn forgetting_drops_a_node_at_once() {
        let now = Instant::now();
        let mut m = tracker(now, 3);
        m.forget(node(3));
        assert_eq!(m.state(node(3), now), NodeState::Dead);
        assert_eq!(m.tracked(), 2);
    }

    #[test]
    fn reaping_is_housekeeping_and_changes_no_answer() {
        let config = MembershipConfig::default();
        let start = Instant::now();
        let mut m = tracker(start, 3);
        m.heard(node(1), NodeMode::Active, start + config.dead_after);
        let now = start + config.dead_after;

        let before = m.view(now);
        let alive_before = m.alive_count(now);
        assert_eq!(m.tracked(), 3);

        m.reap(now);
        assert_eq!(m.tracked(), 1, "reaping did not drop the dead");
        assert_eq!(m.view(now), before, "reaping changed the view");
        assert_eq!(m.alive_count(now), alive_before);
    }

    #[test]
    fn a_node_does_not_assume_itself_alive() {
        // A node whose own gossip loop has stopped is not healthy, and seeding
        // itself as alive would hide exactly that failure.
        let now = Instant::now();
        let m = Membership::new(node(1), MembershipConfig::default());
        assert_eq!(m.state(node(1), now), NodeState::Dead);
        assert_eq!(m.alive_count(now), 0);
        assert_eq!(m.local(), node(1));
    }

    #[test]
    fn inverted_windows_are_rejected_rather_than_silently_skipping_suspicion() {
        assert!(MembershipConfig::default().is_safe());
        let inverted = MembershipConfig {
            suspect_after: Duration::from_secs(10),
            dead_after: Duration::from_secs(3),
        };
        assert!(!inverted.is_safe());
    }

    #[test]
    fn an_inverted_config_still_never_reports_a_dead_node_as_alive() {
        // Configuration validation rejects it, but the ordering of the checks
        // here means the unsafe direction is impossible even if it slips past.
        let start = Instant::now();
        let mut m = Membership::new(
            node(1),
            MembershipConfig {
                suspect_after: Duration::from_secs(10),
                dead_after: Duration::from_secs(3),
            },
        );
        m.heard(node(2), NodeMode::Active, start);
        assert_eq!(
            m.state(node(2), start + Duration::from_secs(5)),
            NodeState::Dead,
            "an inverted config reported a long-silent node as alive"
        );
    }
}
