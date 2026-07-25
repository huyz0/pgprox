//! What a node tells its peers, and how a peer's telling is merged in.
//!
//! # Merging must be order-independent
//!
//! Gossip delivers out of order, duplicates, and drops. Two nodes that receive
//! the same set of digests in different orders must reach the same view, or the
//! cluster never converges and `pgprox_cluster_view_hash` reports split brain
//! that is really just arrival order.
//!
//! The mechanism is a per-node version counter. A digest is accepted only if it
//! is newer than what is held, which makes merge idempotent, commutative, and
//! safe against a replayed old message.
//!
//! # Digests are an API
//!
//! They feed cluster-wide admin aggregates as well as control, so `SHOW POOLS`
//! on any pod answers from these. That makes the schema a public interface with
//! the care that implies, not an internal detail.

use std::collections::HashMap;

use pgprox_core::cluster::{ClusterDigest, NodeMode};
use pgprox_core::ids::{NodeId, ServerId};

/// A digest with the version that orders it against others from the same node.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VersionedDigest {
    /// What the node said about itself.
    pub digest: ClusterDigest,
    /// Monotonic per node. Compared only against digests from the same node,
    /// so nodes need no shared clock.
    pub version: u64,
}

/// Every node's most recent digest.
#[derive(Debug, Default)]
pub struct DigestStore {
    latest: HashMap<NodeId, VersionedDigest>,
}

/// What a merge did, for metrics and for tests that need to know a stale
/// message was recognised rather than silently applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeOutcome {
    /// First news of this node.
    Added,
    /// Newer than what was held.
    Updated,
    /// Older than or equal to what was held, so ignored.
    Stale,
}

impl DigestStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Merges a peer's digest.
    ///
    /// Equal versions are stale rather than applied. A node that re-sends the
    /// same version has nothing new to say, and treating it as an update would
    /// make merge order-dependent whenever two digests tie.
    pub fn merge(&mut self, incoming: VersionedDigest) -> MergeOutcome {
        match self.latest.get(&incoming.digest.node) {
            Some(held) if incoming.version <= held.version => MergeOutcome::Stale,
            Some(_) => {
                self.latest.insert(incoming.digest.node, incoming);
                MergeOutcome::Updated
            }
            None => {
                self.latest.insert(incoming.digest.node, incoming);
                MergeOutcome::Added
            }
        }
    }

    /// What is held for a node.
    #[must_use]
    pub fn get(&self, node: NodeId) -> Option<&ClusterDigest> {
        self.latest.get(&node).map(|v| &v.digest)
    }

    /// How many nodes are known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.latest.len()
    }

    /// Whether nothing is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.latest.is_empty()
    }

    /// Forgets a node, for when membership declares it gone.
    pub fn forget(&mut self, node: NodeId) {
        self.latest.remove(&node);
    }

    // No `membership` here on purpose. A view built from digests alone counts a
    // node that has been silent for an hour, and the coordinator's whole safety
    // argument rests on the view being liveness-filtered. One source of the
    // view, in `crate::membership::Membership`, means a caller cannot pick the
    // wrong one. Same reasoning as `pgprox_core::route::decide`: two
    // implementations of a rule is two chances to get it wrong.

    /// Total upstream connections every known node reports holding for a
    /// server.
    ///
    /// This is what makes `SHOW POOLS` answerable from any pod with no fan-out.
    #[must_use]
    pub fn cluster_usage(&self, server: &ServerId) -> u32 {
        self.latest
            .values()
            .flat_map(|v| v.digest.upstream_conns.iter())
            .filter(|(s, _)| s == server)
            .map(|(_, count)| *count)
            .fold(0_u32, u32::saturating_add)
    }

    /// Total client connections across the cluster.
    #[must_use]
    pub fn cluster_clients(&self) -> u32 {
        self.latest
            .values()
            .map(|v| v.digest.client_conns)
            .fold(0_u32, u32::saturating_add)
    }

    /// A hash of the view, for detecting split brain.
    ///
    /// Two pods holding the same membership produce the same value. A
    /// difference means their views have diverged, which is otherwise only
    /// visible by comparing pods by hand.
    #[must_use]
    pub fn view_hash(&self) -> u64 {
        // Order-independent by construction: node contributions are combined
        // with addition rather than by hashing a sequence, so insertion order
        // cannot change the result.
        self.latest
            .values()
            .map(|v| {
                let mode = match v.digest.mode {
                    NodeMode::Active => 1_u64,
                    NodeMode::Draining => 2,
                    _ => 3,
                };
                u64::from(v.digest.node.get())
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                    .wrapping_add(mode)
            })
            .fold(0_u64, u64::wrapping_add)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(n: u16) -> NodeId {
        NodeId::new(n)
    }

    fn digest(n: u16, version: u64, clients: u32) -> VersionedDigest {
        VersionedDigest {
            digest: ClusterDigest {
                node: node(n),
                mode: NodeMode::Active,
                client_conns: clients,
                upstream_conns: vec![(ServerId::new("db-1", 5432), clients / 10)],
                tenant_usage: Vec::new(),
            },
            version,
        }
    }

    #[test]
    fn merging_is_order_independent() {
        // The property the whole module exists for. Two nodes receiving the
        // same digests in different orders must agree, or the cluster never
        // converges and split-brain detection fires on arrival order.
        let messages: Vec<VersionedDigest> = (1..=4_u16)
            .flat_map(|n| {
                (1..=3_u64)
                    .map(move |v| digest(n, v, u32::from(n) * 10 + u32::try_from(v).unwrap_or(0)))
            })
            .collect();

        let mut forwards = DigestStore::new();
        for m in &messages {
            forwards.merge(m.clone());
        }

        let mut backwards = DigestStore::new();
        for m in messages.iter().rev() {
            backwards.merge(m.clone());
        }

        assert_eq!(forwards.view_hash(), backwards.view_hash());
        assert_eq!(forwards.len(), backwards.len());
        for n in 1..=4_u16 {
            assert_eq!(
                forwards.get(node(n)),
                backwards.get(node(n)),
                "node {n} differed by arrival order"
            );
        }
    }

    #[test]
    fn merging_is_idempotent() {
        // Gossip duplicates. Applying the same digest twice must change
        // nothing, or a duplicate would look like news.
        let mut store = DigestStore::new();
        assert_eq!(store.merge(digest(1, 5, 100)), MergeOutcome::Added);
        assert_eq!(
            store.merge(digest(1, 5, 100)),
            MergeOutcome::Stale,
            "a duplicate was treated as an update"
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn an_older_digest_never_overwrites_a_newer_one() {
        // Reordering delivers the past after the future.
        let mut store = DigestStore::new();
        store.merge(digest(1, 10, 500));
        assert_eq!(store.merge(digest(1, 3, 1)), MergeOutcome::Stale);
        assert_eq!(store.get(node(1)).unwrap().client_conns, 500);
    }

    #[test]
    fn an_equal_version_is_stale_rather_than_applied() {
        // Applying it would make merge order-dependent whenever two digests
        // tie, which is exactly when order-independence matters.
        let mut store = DigestStore::new();
        store.merge(digest(1, 7, 100));

        let mut different_content = digest(1, 7, 999);
        different_content.version = 7;
        assert_eq!(store.merge(different_content), MergeOutcome::Stale);
        assert_eq!(store.get(node(1)).unwrap().client_conns, 100);
    }

    #[test]
    fn a_newer_digest_updates() {
        let mut store = DigestStore::new();
        store.merge(digest(1, 1, 10));
        assert_eq!(store.merge(digest(1, 2, 20)), MergeOutcome::Updated);
        assert_eq!(store.get(node(1)).unwrap().client_conns, 20);
    }

    #[test]
    fn the_store_is_the_same_whatever_the_insertion_order() {
        // Two nodes that have heard the same things must agree, whatever order
        // gossip delivered them in, or they will disagree about the leader.
        let mut forwards = DigestStore::new();
        for n in [3_u16, 1, 4, 2] {
            forwards.merge(digest(n, 1, 0));
        }
        let mut backwards = DigestStore::new();
        for n in [2_u16, 4, 1, 3] {
            backwards.merge(digest(n, 1, 0));
        }

        assert_eq!(forwards.view_hash(), backwards.view_hash());
        for n in 1..=4_u16 {
            assert_eq!(forwards.get(node(n)), backwards.get(node(n)));
        }
    }

    #[test]
    fn a_draining_node_is_kept_with_its_mode() {
        // The store records what a node said about itself. Whether that node
        // still counts is a liveness question, answered elsewhere.
        let mut store = DigestStore::new();
        store.merge(digest(1, 1, 0));

        let mut draining = digest(2, 1, 0);
        draining.digest.mode = NodeMode::Draining;
        store.merge(draining);

        assert_eq!(store.len(), 2, "a draining node vanished from the store");
        assert_eq!(store.get(node(2)).map(|d| d.mode), Some(NodeMode::Draining));
    }

    #[test]
    fn cluster_usage_sums_across_nodes_without_a_fan_out() {
        // What makes SHOW POOLS answerable from any pod at no cost.
        let mut store = DigestStore::new();
        for n in 1..=4_u16 {
            store.merge(digest(n, 1, 100));
        }
        assert_eq!(store.cluster_usage(&ServerId::new("db-1", 5432)), 40);
        assert_eq!(store.cluster_clients(), 400);
        assert_eq!(
            store.cluster_usage(&ServerId::new("db-9", 5432)),
            0,
            "an unknown server reported usage"
        );
    }

    #[test]
    fn aggregates_saturate_rather_than_overflowing() {
        // A digest is a peer's claim about itself. A malformed or hostile one
        // must not panic the node reading it.
        let mut store = DigestStore::new();
        for n in 1..=4_u16 {
            store.merge(VersionedDigest {
                digest: ClusterDigest {
                    node: node(n),
                    mode: NodeMode::Active,
                    client_conns: u32::MAX,
                    upstream_conns: vec![(ServerId::new("db-1", 5432), u32::MAX)],
                    tenant_usage: Vec::new(),
                },
                version: 1,
            });
        }
        assert_eq!(store.cluster_clients(), u32::MAX);
        assert_eq!(store.cluster_usage(&ServerId::new("db-1", 5432)), u32::MAX);
    }

    #[test]
    fn the_view_hash_agrees_across_pods_and_differs_on_divergence() {
        // The whole point of exporting it: a mismatch means split brain rather
        // than something an operator has to infer by comparing pods by hand.
        let mut a = DigestStore::new();
        let mut b = DigestStore::new();
        for n in [1_u16, 2, 3] {
            a.merge(digest(n, 1, 0));
        }
        for n in [3_u16, 2, 1] {
            b.merge(digest(n, 1, 0));
        }
        assert_eq!(a.view_hash(), b.view_hash());

        b.merge(digest(4, 1, 0));
        assert_ne!(a.view_hash(), b.view_hash(), "divergence went unnoticed");
    }

    #[test]
    fn the_view_hash_changes_when_a_node_starts_draining() {
        // Drain changes placement, so two pods disagreeing about it is exactly
        // the split brain worth detecting.
        let mut store = DigestStore::new();
        store.merge(digest(1, 1, 0));
        let before = store.view_hash();

        let mut draining = digest(1, 2, 0);
        draining.digest.mode = NodeMode::Draining;
        store.merge(draining);

        assert_ne!(before, store.view_hash(), "a drain did not change the view");
    }

    #[test]
    fn forgetting_a_node_removes_it_from_every_answer() {
        let mut store = DigestStore::new();
        store.merge(digest(1, 1, 100));
        store.merge(digest(2, 1, 100));
        let with_both = store.view_hash();

        store.forget(node(2));
        assert_eq!(store.len(), 1);
        assert!(store.get(node(2)).is_none());
        assert_eq!(store.cluster_clients(), 100);
        assert_ne!(store.view_hash(), with_both);
    }

    #[test]
    fn an_empty_store_answers_rather_than_failing() {
        let store = DigestStore::new();
        assert!(store.is_empty());
        assert_eq!(store.cluster_clients(), 0);
        assert_eq!(store.cluster_usage(&ServerId::new("db-1", 5432)), 0);
        assert_eq!(store.view_hash(), 0);
        assert_eq!(store.get(node(1)), None);
    }

    #[test]
    fn a_forgotten_node_can_return() {
        // A node that left and rejoined must not be permanently ignored by a
        // version counter that outlived it.
        let mut store = DigestStore::new();
        store.merge(digest(1, 10, 100));
        store.forget(node(1));
        assert_eq!(store.merge(digest(1, 1, 5)), MergeOutcome::Added);
        assert_eq!(store.get(node(1)).unwrap().client_conns, 5);
    }
}
