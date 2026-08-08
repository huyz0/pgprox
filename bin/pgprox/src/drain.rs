//! Taking a node out of service without dropping anyone's transaction.
//!
//! # The order is the whole thing
//!
//! 1. `/readyz` fails, so Kubernetes stops sending new clients here.
//! 2. Gossip announces it, so peers stop counting this node's capacity and
//!    stop homing tenants to it.
//! 3. Only then do clients start leaving, and only between transactions.
//! 4. Whatever is still connected when the grace runs out is closed.
//!
//! Every pair in that list is a bug the other way round. Closing clients
//! before the probe fails sends them straight back to the node they just left.
//! Closing them before gossip announces leaves peers believing this node still
//! has room, so the replacements land on a node that is going away. Waiting for
//! in-flight transactions without a bound means one `BEGIN; ...` with nobody at
//! the keyboard holds a rolling deploy forever.
//!
//! # What starts it
//!
//! Anything that makes the node draining: a `POST /v1/drain`, a `ConfigMap`
//! naming this node, or `SIGTERM` on the way to stopping. The run loop notices
//! the state changed rather than every caller remembering to run the sequence,
//! which is what stops the three paths diverging.
//!
//! # It is reversible until the last step
//!
//! An undrain, or a drain TTL that expires, clears the signals and the node
//! goes back to serving. The clients that already left are gone, which is what
//! a drain means, but the node is not poisoned.

use std::sync::Arc;
use std::time::Duration;

use pgprox_cluster::service::GossipCoordinator;
use pgprox_core::cluster::NodeMode;

use crate::run::Shutdown;
use crate::sessions::Sessions;

/// How often the sequence checks whether the last client has gone.
///
/// Fine-grained, because the common case is that everyone leaves in well under
/// a second and the sequence should not sit out the rest of a poll interval
/// after the node is already empty.
pub const SETTLE_INTERVAL: Duration = Duration::from_millis(50);

/// One step of the sequence, recorded in the order it happened.
///
/// Returned rather than logged, because the order is the property this exists
/// to guarantee, and an order can only be asserted if something carries it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Step {
    /// `/readyz` now fails, so no new client is sent here.
    Unready,
    /// The fleet has been told, or told as far as this node can tell.
    Announced,
    /// Clients between transactions were asked to leave.
    Closing,
    /// Everyone had gone before the grace ran out.
    Settled,
    /// The grace ran out with this many clients still connected.
    Forced(u32),
}

/// What the sequence acts on.
///
/// Borrowed rather than owned: every field belongs to the node, and a drain
/// that owned any of them would be a second place they live.
pub struct Drain<'a> {
    /// The fleet, for the announcement.
    pub cluster: &'a Arc<GossipCoordinator>,
    /// Who is still here.
    pub sessions: &'a Arc<Sessions>,
    /// Peers to announce to, so the news does not wait for the next tick.
    pub peers: &'a [String],
    /// Fired to ask idle clients to leave.
    pub draining: &'a Shutdown,
    /// Fired when the grace runs out.
    pub closing: &'a Shutdown,
    /// How long in-flight transactions get.
    pub grace: Duration,
}

impl std::fmt::Debug for Drain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Drain")
            .field("grace", &self.grace)
            .field("clients", &self.sessions.len())
            .finish_non_exhaustive()
    }
}

impl Drain<'_> {
    /// Runs the sequence, returning the steps in the order they happened.
    ///
    /// The caller has already made the node draining: that is what `/readyz`
    /// reads, and it is what makes [`Step::Unready`] true before this is
    /// called rather than because of it. Recording it here anyway is the point
    /// of returning the order at all.
    pub async fn run(&self) -> Vec<Step> {
        let mut steps = vec![Step::Unready];

        // Before any client is closed. A peer that still believes this node
        // has room will send it the very clients that are leaving.
        self.cluster.set_mode(NodeMode::Draining);
        crate::gossip::round(self.peers, self.cluster).await;
        steps.push(Step::Announced);

        self.draining.fire();
        steps.push(Step::Closing);

        if self.settled().await {
            steps.push(Step::Settled);
            return steps;
        }

        // The bound on "in-flight transactions finish". One idle `BEGIN` with
        // nobody at the keyboard must not hold a rolling deploy open.
        let remaining = self.sessions.len();
        self.closing.fire();
        steps.push(Step::Forced(remaining));
        steps
    }

    /// Waits for the last client to leave, or for the grace to run out.
    ///
    /// Returns whether everyone had gone. Polled rather than woken, because
    /// the registry is what a report reads and giving it a waiter list would
    /// make every session's end a notification nothing else needs.
    async fn settled(&self) -> bool {
        let deadline = tokio::time::Instant::now() + self.grace;
        while tokio::time::Instant::now() < deadline {
            if self.sessions.is_empty() {
                return true;
            }
            tokio::time::sleep(SETTLE_INTERVAL.min(self.grace)).await;
        }
        self.sessions.is_empty()
    }
}

/// Puts a node back into service.
///
/// The signals are cleared and the fleet is told. Whoever left has left; this
/// is about the node being usable again rather than about undoing anything.
pub fn undrain(cluster: &GossipCoordinator, draining: &Shutdown, closing: &Shutdown) {
    draining.clear();
    closing.clear();
    cluster.set_mode(NodeMode::Active);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_cluster::coordinator::CoordinatorConfig;
    use pgprox_core::clock::FakeClock;
    use pgprox_core::ids::{ConnId, NodeId, TenantId};
    use std::time::Instant;

    fn cluster() -> Arc<GossipCoordinator> {
        GossipCoordinator::new(
            NodeId::new(1),
            CoordinatorConfig::default(),
            Arc::new(FakeClock::new()),
        )
    }

    fn drain_over<'a>(
        cluster: &'a Arc<GossipCoordinator>,
        sessions: &'a Arc<Sessions>,
        draining: &'a Shutdown,
        closing: &'a Shutdown,
        grace: Duration,
    ) -> Drain<'a> {
        Drain {
            cluster,
            sessions,
            peers: &[],
            draining,
            closing,
            grace,
        }
    }

    #[tokio::test]
    async fn an_empty_node_settles_without_forcing_anyone() {
        let (cluster, sessions) = (cluster(), Sessions::new());
        let (draining, closing) = (Shutdown::new(), Shutdown::new());

        let steps = drain_over(
            &cluster,
            &sessions,
            &draining,
            &closing,
            Duration::from_secs(5),
        )
        .run()
        .await;

        assert_eq!(
            steps,
            vec![Step::Unready, Step::Announced, Step::Closing, Step::Settled]
        );
        assert!(
            !closing.fired(),
            "a node with nobody on it force-closed somebody"
        );
    }

    #[tokio::test]
    async fn the_fleet_is_told_before_any_client_is_asked_to_leave() {
        // The order that matters most: a peer that still believes this node
        // has room sends it the very clients that are leaving.
        let (cluster, sessions) = (cluster(), Sessions::new());
        let (draining, closing) = (Shutdown::new(), Shutdown::new());

        let steps = drain_over(
            &cluster,
            &sessions,
            &draining,
            &closing,
            Duration::from_secs(5),
        )
        .run()
        .await;

        let announced = steps.iter().position(|step| *step == Step::Announced);
        let asked = steps.iter().position(|step| *step == Step::Closing);
        assert!(
            announced < asked,
            "clients were asked to leave before the fleet was told: {steps:?}"
        );
        assert_eq!(cluster.outgoing().digest.mode, NodeMode::Draining);
    }

    #[tokio::test]
    async fn a_client_that_will_not_leave_is_closed_when_the_grace_runs_out() {
        // One idle BEGIN with nobody at the keyboard must not hold a rolling
        // deploy open.
        let (cluster, sessions) = (cluster(), Sessions::new());
        let (draining, closing) = (Shutdown::new(), Shutdown::new());
        let _stubborn = sessions.register(
            ConnId::new(NodeId::new(1), 1),
            TenantId::new("acme"),
            NodeId::new(1),
            Instant::now(),
            16,
            Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        let steps = drain_over(
            &cluster,
            &sessions,
            &draining,
            &closing,
            Duration::from_millis(120),
        )
        .run()
        .await;

        assert_eq!(steps.last(), Some(&Step::Forced(1)));
        assert!(closing.fired(), "the grace expired and nothing was closed");
    }

    #[tokio::test]
    async fn a_client_that_leaves_within_the_grace_is_never_forced() {
        // The property the whole sequence exists for: a transaction in flight
        // finishes rather than failing.
        let (cluster, sessions) = (cluster(), Sessions::new());
        let (draining, closing) = (Shutdown::new(), Shutdown::new());
        let leaving = sessions.register(
            ConnId::new(NodeId::new(1), 1),
            TenantId::new("acme"),
            NodeId::new(1),
            Instant::now(),
            16,
            Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            drop(leaving);
        });

        let steps = drain_over(
            &cluster,
            &sessions,
            &draining,
            &closing,
            Duration::from_secs(5),
        )
        .run()
        .await;

        assert_eq!(steps.last(), Some(&Step::Settled));
        assert!(!closing.fired());
    }

    #[tokio::test]
    async fn an_undrain_puts_the_node_back() {
        let (cluster, sessions) = (cluster(), Sessions::new());
        let (draining, closing) = (Shutdown::new(), Shutdown::new());
        drain_over(
            &cluster,
            &sessions,
            &draining,
            &closing,
            Duration::from_millis(50),
        )
        .run()
        .await;
        assert!(draining.fired());

        undrain(&cluster, &draining, &closing);

        assert!(!draining.fired());
        assert!(!closing.fired());
        assert_eq!(cluster.outgoing().digest.mode, NodeMode::Active);
    }

    #[tokio::test]
    async fn a_drain_prints_its_grace_and_how_many_clients_it_is_waiting_on() {
        // `M17.4`: this `Debug` could return an empty string, because nothing
        // read it. It is what an operator sees if the drain task panics, and
        // the two fields on it are the two questions worth asking at that
        // moment: how long the grace was, and how many clients had not left.
        // The same shape as `Deps`, `Context` and `Probes`, all of which had
        // the same mutant and all of which were only ever asserted for what
        // they must *not* say.
        let (cluster, sessions) = (cluster(), Sessions::new());
        let (draining, closing) = (Shutdown::new(), Shutdown::new());
        let _held = sessions.register(
            pgprox_core::ids::ConnId::new(NodeId::new(1), 1),
            TenantId::new("acme"),
            NodeId::new(1),
            std::time::Instant::now(),
            16,
            Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        let rendered = format!(
            "{:?}",
            drain_over(
                &cluster,
                &sessions,
                &draining,
                &closing,
                Duration::from_secs(7),
            )
        );

        assert!(rendered.contains("Drain"), "{rendered}");
        assert!(
            rendered.contains("7s"),
            "the grace is not named: {rendered}"
        );
        assert!(
            rendered.contains("clients: 1"),
            "the client count is not named: {rendered}"
        );
    }
}
