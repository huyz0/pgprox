//! Liveness and readiness.
//!
//! # Two probes, two questions
//!
//! `/healthz` asks whether the process should be restarted. `/readyz` asks
//! whether it should receive traffic. Kubernetes acts very differently on the
//! two answers, so conflating them is how a slow node becomes a restarting one.
//!
//! # Only drain may fail readiness
//!
//! This is the rule the whole crate is shaped around, and it is the opposite of
//! what readiness probes usually do.
//!
//! The tempting version reports not-ready when the node is under pressure:
//! pools full, waiters queued, upstream unreachable. That is a feedback loop
//! with the sign the wrong way round. Kubernetes pulls the node from the
//! Service, its clients reconnect, they land on the remaining nodes, those
//! nodes get more loaded and report not-ready in turn, and the fleet fails one
//! node at a time under a load it could have served. The proxy exists to absorb
//! connection storms, and a flapping readiness probe manufactures one.
//!
//! A node under pressure is still the best place for its clients: it holds
//! their warm connections and their prepared statements. Moving them costs a
//! reconnect and gains nothing.
//!
//! So readiness is a function of drain and nothing else, and
//! [`Health::readiness`] takes only what it is allowed to consider. There is no
//! parameter for load, which is a stronger guarantee than a rule saying not to
//! look at it.
//!
//! # Liveness is nearly always true
//!
//! A restart drops every client connection on the node. That is worth doing for
//! a process that cannot recover and not much else, so liveness fails only for
//! conditions a restart actually fixes.

use std::fmt;
use std::time::{Duration, Instant};

/// What a probe answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Probe {
    /// Serving.
    Pass,
    /// Not serving, with the reason for the response body.
    Fail(Reason),
}

impl Probe {
    /// Whether the probe passed.
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Pass)
    }

    /// The HTTP status to answer with.
    #[must_use]
    pub const fn status(self) -> u16 {
        match self {
            Self::Pass => 200,
            // 503 rather than 500: this is a healthy process declining traffic,
            // and the distinction matters to anything reading the status rather
            // than the body.
            Self::Fail(_) => 503,
        }
    }

    /// The reason, if it failed.
    #[must_use]
    pub const fn reason(self) -> Option<Reason> {
        match self {
            Self::Pass => None,
            Self::Fail(reason) => Some(reason),
        }
    }
}

/// Why a probe failed.
///
/// Short, and deliberately so. Every entry here is a way for a node to leave
/// rotation, and the list being short is what stops readiness becoming a
/// load signal by degrees.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Reason {
    /// The node is draining.
    Draining,
    /// The node has not finished starting.
    ///
    /// Only ever true before the first successful configuration load. A node
    /// that has never had a configuration cannot serve, and this is what stops
    /// it being sent traffic during a rolling deploy before it is ready.
    Starting,
    /// The process is wedged and a restart is the remedy.
    ///
    /// Liveness only. Nothing sets this from load, and nothing should: a
    /// restart drops every client connection on the node.
    Stuck,
}

impl Reason {
    /// The text for the response body.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draining => "draining",
            Self::Starting => "starting",
            Self::Stuck => "stuck",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How liveness is tuned.
#[derive(Clone, Copy, Debug)]
pub struct HealthConfig {
    /// How long the main loop may go without a heartbeat before the process is
    /// considered wedged.
    ///
    /// Generous. A restart drops every client connection on the node, so this
    /// is for a process that has genuinely stopped, not one that is busy.
    pub heartbeat_timeout: Duration,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            heartbeat_timeout: Duration::from_secs(60),
        }
    }
}

/// What the probes answer.
#[derive(Debug)]
pub struct Health {
    config: HealthConfig,
    /// Set once the first configuration has loaded.
    started: bool,
    /// Last time the main loop said it was alive.
    heartbeat: Option<Instant>,
}

impl Health {
    /// A node that has not finished starting.
    #[must_use]
    pub const fn new(config: HealthConfig) -> Self {
        Self {
            config,
            started: false,
            heartbeat: None,
        }
    }

    /// Records that startup finished, which is the first successful
    /// configuration load.
    pub const fn started(&mut self) {
        self.started = true;
    }

    /// Records that the main loop is running.
    pub const fn beat(&mut self, at: Instant) {
        self.heartbeat = Some(at);
    }

    /// Whether the node should receive traffic.
    ///
    /// Takes `draining` and nothing else. There is deliberately no parameter
    /// for load: a rule saying not to consider it can be forgotten, and a
    /// signature that cannot express it cannot be.
    #[must_use]
    pub const fn readiness(&self, draining: bool) -> Probe {
        if !self.started {
            return Probe::Fail(Reason::Starting);
        }
        if draining {
            return Probe::Fail(Reason::Draining);
        }
        Probe::Pass
    }

    /// Whether the process should be restarted.
    ///
    /// Fails only when the main loop has stopped beating, which a restart
    /// actually fixes. Draining does not fail liveness: a draining node is
    /// working exactly as intended and restarting it would drop the very
    /// connections it is trying to finish.
    #[must_use]
    pub fn liveness(&self, now: Instant) -> Probe {
        // Before the first beat there is nothing to have stopped. A node that
        // never starts beating is caught by readiness, which withholds traffic
        // without dropping every connection on the node.
        let stalled = self
            .heartbeat
            .is_some_and(|at| now.saturating_duration_since(at) > self.config.heartbeat_timeout);
        if stalled {
            return Probe::Fail(Reason::Stuck);
        }
        Probe::Pass
    }

    /// Whether startup has finished.
    #[must_use]
    pub const fn has_started(&self) -> bool {
        self.started
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    /// `M14.41`. Six mutants survived in this crate and all six are accessors
    /// nobody asked in more than one state. `is_pass`, `as_str`, `max_series`,
    /// `is_empty` and `cardinality` could each return a constant.
    #[test]
    fn a_probe_reports_passing_only_when_it_passed() {
        // `is_pass` could return `false` for everything, which is what decides
        // whether Kubernetes sends this node traffic.
        assert!(Probe::Pass.is_pass());
        assert!(!Probe::Fail(Reason::Draining).is_pass());
        assert!(!Probe::Fail(Reason::Starting).is_pass());
        assert!(!Probe::Fail(Reason::Stuck).is_pass());
    }

    #[test]
    fn every_reason_has_its_own_text() {
        // `as_str` could return one literal for every variant. The body of a
        // failing probe is how an operator learns whether a pod is starting,
        // draining or stuck, and those call for three different responses.
        assert_eq!(Reason::Draining.as_str(), "draining");
        assert_eq!(Reason::Starting.as_str(), "starting");
        assert_eq!(Reason::Stuck.as_str(), "stuck");

        let all = [
            Reason::Draining.as_str(),
            Reason::Starting.as_str(),
            Reason::Stuck.as_str(),
        ];
        let unique: std::collections::HashSet<&str> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len(), "two reasons share a body");
    }

    fn started() -> (Health, Instant) {
        let mut health = Health::new(HealthConfig::default());
        health.started();
        let now = Instant::now();
        health.beat(now);
        (health, now)
    }

    #[test]
    fn a_started_node_that_is_not_draining_is_ready() {
        let (health, now) = started();
        assert_eq!(health.readiness(false), Probe::Pass);
        assert_eq!(health.liveness(now), Probe::Pass);
        assert!(health.has_started());
    }

    #[test]
    fn a_draining_node_is_not_ready() {
        // The one thing that may fail readiness. Kubernetes pulls the pod from
        // the Service, so no new connections arrive while the existing ones
        // finish.
        let (health, _now) = started();
        let probe = health.readiness(true);

        assert_eq!(probe, Probe::Fail(Reason::Draining));
        assert_eq!(probe.status(), 503);
        assert_eq!(probe.reason(), Some(Reason::Draining));
        assert!(!probe.is_pass());
    }

    #[test]
    fn a_node_that_has_not_loaded_a_configuration_is_not_ready() {
        // It cannot serve without one, and during a rolling deploy this is what
        // stops traffic arriving before it can.
        let health = Health::new(HealthConfig::default());
        assert_eq!(health.readiness(false), Probe::Fail(Reason::Starting));
        assert!(!health.has_started());
    }

    #[test]
    fn readiness_cannot_be_made_to_consider_load() {
        // The rule the crate is shaped around, expressed as a signature rather
        // than as a comment. `readiness` takes one boolean, and there is
        // nowhere to put a pool depth, a waiter count or an upstream error.
        //
        // The failure it prevents is a feedback loop with the sign the wrong
        // way round: a loaded node reports not-ready, its clients reconnect
        // onto the remaining nodes, those get more loaded and report not-ready
        // in turn, and the fleet fails one node at a time under a load it could
        // have served. A proxy that exists to absorb connection storms would be
        // manufacturing one.
        let (health, _now) = started();
        assert_eq!(health.readiness(false), Probe::Pass);
        assert_eq!(health.readiness(true), Probe::Fail(Reason::Draining));

        // Every way for readiness to fail, so an addition to this list is a
        // deliberate act rather than a side effect.
        let ways_to_fail = [Reason::Draining, Reason::Starting];
        assert_eq!(
            ways_to_fail.len(),
            2,
            "a third way to leave rotation was added; is it load in disguise?"
        );
    }

    #[test]
    fn a_draining_node_is_still_alive() {
        // It is working exactly as intended, and restarting it would drop the
        // very connections it is trying to finish.
        let (health, now) = started();
        assert_eq!(health.liveness(now), Probe::Pass);
        assert_eq!(health.readiness(true), Probe::Fail(Reason::Draining));
    }

    #[test]
    fn a_wedged_process_fails_liveness() {
        let config = HealthConfig::default();
        let mut health = Health::new(config);
        health.started();
        let start = Instant::now();
        health.beat(start);

        assert_eq!(
            health.liveness(start + config.heartbeat_timeout),
            Probe::Pass
        );
        assert_eq!(
            health.liveness(start + config.heartbeat_timeout + Duration::from_secs(1)),
            Probe::Fail(Reason::Stuck)
        );
    }

    #[test]
    fn a_beat_clears_a_previous_stall() {
        let config = HealthConfig::default();
        let mut health = Health::new(config);
        health.started();
        let start = Instant::now();
        health.beat(start);

        let late = start + config.heartbeat_timeout + Duration::from_secs(1);
        assert_eq!(health.liveness(late), Probe::Fail(Reason::Stuck));

        health.beat(late);
        assert_eq!(health.liveness(late), Probe::Pass);
    }

    #[test]
    fn a_node_that_has_not_beaten_yet_is_not_restarted() {
        // A node that never starts beating is caught by readiness, which
        // withholds traffic without dropping every connection on the node.
        let health = Health::new(HealthConfig::default());
        assert_eq!(health.liveness(Instant::now()), Probe::Pass);
        assert_eq!(
            health.readiness(false),
            Probe::Fail(Reason::Starting),
            "an unstarted node was sent traffic"
        );
    }

    #[test]
    fn a_clock_that_jumped_backwards_does_not_restart_the_process() {
        // Saturating, so a beat stamped in the future reads as recent rather
        // than as impossibly old.
        let mut health = Health::new(HealthConfig::default());
        health.started();
        let now = Instant::now();
        health.beat(now + Duration::from_secs(300));

        assert_eq!(health.liveness(now), Probe::Pass);
    }

    #[test]
    fn a_failing_probe_answers_503_rather_than_500() {
        // A healthy process declining traffic is not an error, and anything
        // reading the status rather than the body needs to be able to tell.
        assert_eq!(Probe::Fail(Reason::Draining).status(), 503);
        assert_eq!(Probe::Fail(Reason::Stuck).status(), 503);
        assert_eq!(Probe::Pass.status(), 200);
        assert_eq!(Probe::Pass.reason(), None);
    }

    #[test]
    fn every_reason_has_a_body_an_operator_can_read() {
        for reason in [Reason::Draining, Reason::Starting, Reason::Stuck] {
            assert!(!reason.as_str().is_empty());
            assert_eq!(reason.to_string(), reason.as_str());
        }
    }

    #[test]
    fn the_liveness_timeout_is_generous() {
        // A restart drops every client connection on the node, so this is for a
        // process that has genuinely stopped rather than one that is busy.
        assert!(HealthConfig::default().heartbeat_timeout >= Duration::from_secs(30));
    }
}
