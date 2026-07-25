//! Draining a node, declaratively and imperatively.
//!
//! # Two ways in, one meaning
//!
//! A node drains because the config document says so, or because somebody
//! posted to `/v1/drain`. Both end up as the same desired state, which is what
//! makes a drain survive a pod restart and show up in git rather than being a
//! side effect somebody ran once.
//!
//! # The imperative one expires
//!
//! A drain requested through the API carries a TTL. Without one, a node drained
//! at 2am during an incident is still drained at 9am, and nobody can tell
//! whether that was deliberate or forgotten. The two look identical, and the
//! only way to find out is to ask the person who did it.
//!
//! With a TTL the node comes back on its own and the fleet returns to what the
//! document says. If the drain was deliberate, it belongs in the document,
//! where it is reviewable and survives a restart.
//!
//! # Precedence
//!
//! The document wins. An overlay may drain a node the document calls active,
//! because that is the incident case, but it may not bring back a node the
//! document drains: that would let an API call quietly undo a reviewed change,
//! and the next config poll would flip it back anyway.

use std::time::{Duration, Instant};

use pgprox_core::cluster::NodeMode;
use pgprox_core::config::Config;

/// Bounds on how long an imperative drain may last.
#[derive(Clone, Copy, Debug)]
pub struct DrainConfig {
    /// Used when a caller gives no TTL.
    pub default_ttl: Duration,
    /// The longest a caller may ask for.
    ///
    /// An unbounded TTL is the same problem as no TTL with more steps: a drain
    /// set to a year is one nobody will remember either.
    pub max_ttl: Duration,
}

impl Default for DrainConfig {
    fn default() -> Self {
        Self {
            // Long enough to finish a node replacement, short enough that a
            // forgotten one resolves itself inside a working day.
            default_ttl: Duration::from_secs(30 * 60),
            max_ttl: Duration::from_secs(4 * 60 * 60),
        }
    }
}

/// An imperative override of this node's mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Overlay {
    mode: NodeMode,
    expires_at: Instant,
}

/// This node's mode, from the document and any overlay on top.
#[derive(Debug)]
pub struct DrainState {
    config: DrainConfig,
    node_name: String,
    overlay: Option<Overlay>,
}

/// Why a node is in the mode it is in.
///
/// An operator asking "why is this node draining" needs to know whether to edit
/// the document or wait, and those are different answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ModeSource {
    /// From the config document.
    Document,
    /// From an API call, expiring at some point.
    Overlay,
}

impl DrainState {
    /// State for a node with no overlay.
    #[must_use]
    pub fn new(node_name: impl Into<String>, config: DrainConfig) -> Self {
        Self {
            config,
            node_name: node_name.into(),
            overlay: None,
        }
    }

    /// Which node this is.
    #[must_use]
    pub fn node_name(&self) -> &str {
        &self.node_name
    }

    /// The mode in force, and where it came from.
    ///
    /// The document wins. An overlay may drain a node the document calls
    /// active; it may not activate one the document drains.
    #[must_use]
    pub fn mode(&self, config: &Config, now: Instant) -> (NodeMode, ModeSource) {
        let from_document = config.mode_for(&self.node_name);
        if from_document == NodeMode::Draining {
            return (NodeMode::Draining, ModeSource::Document);
        }

        match self.live_overlay(now) {
            Some(overlay) => (overlay.mode, ModeSource::Overlay),
            None => (from_document, ModeSource::Document),
        }
    }

    /// Whether the node should be draining.
    #[must_use]
    pub fn is_draining(&self, config: &Config, now: Instant) -> bool {
        self.mode(config, now).0 == NodeMode::Draining
    }

    /// Sets an overlay, returning when it expires.
    ///
    /// A `ttl` of [`None`] takes the configured default, which is still an
    /// expiry. There is deliberately no way to ask for an overlay that never
    /// lapses: that is what the config document is for, and
    /// [`pgprox_core::admin::Observatory::drain`] takes a plain `Duration` for
    /// the same reason. The two used to disagree about what `None` meant, one
    /// reading it as "forever" and the other as "the default", which would have
    /// produced a drain that silently expired when the API said it would not.
    ///
    /// Anything longer than `max_ttl` is clamped rather than refused: a caller
    /// asking for a week wants the node drained, and refusing outright during
    /// an incident helps nobody. The clamp is visible in the returned instant.
    pub fn set(&mut self, mode: NodeMode, ttl: Option<Duration>, now: Instant) -> Instant {
        let ttl = ttl
            .unwrap_or(self.config.default_ttl)
            .min(self.config.max_ttl);
        let expires_at = now + ttl;
        self.overlay = Some(Overlay { mode, expires_at });
        expires_at
    }

    /// Removes the overlay, returning to what the document says.
    pub const fn clear(&mut self) {
        self.overlay = None;
    }

    /// When the overlay expires, if one is live.
    #[must_use]
    pub fn expires_at(&self, now: Instant) -> Option<Instant> {
        self.live_overlay(now).map(|overlay| overlay.expires_at)
    }

    /// How long the overlay has left, if one is live.
    #[must_use]
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.expires_at(now)
            .map(|at| at.saturating_duration_since(now))
    }

    /// The overlay, if it has not expired.
    ///
    /// Expiry is read here rather than swept, so an overlay is never counted
    /// after its time whether or not anything remembered to clean up. Same
    /// reasoning as `QuotaLease::count` in `pgprox-cluster`.
    fn live_overlay(&self, now: Instant) -> Option<Overlay> {
        self.overlay.filter(|overlay| now < overlay.expires_at)
    }

    /// Drops an expired overlay. Housekeeping only.
    pub fn reap(&mut self, now: Instant) {
        if self.live_overlay(now).is_none() {
            self.overlay = None;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_core::config::NodeOverride;

    fn config_with(node: &str, mode: NodeMode) -> Config {
        let mut config = Config::default();
        config.nodes.insert(node.to_owned(), NodeOverride { mode });
        config
    }

    fn state() -> (DrainState, Instant) {
        (
            DrainState::new("pgprox-2", DrainConfig::default()),
            Instant::now(),
        )
    }

    #[test]
    fn a_node_with_no_entry_anywhere_is_active() {
        // Forgetting to list a node must not accidentally drain it.
        let (state, now) = state();
        let config = Config::default();

        assert_eq!(
            state.mode(&config, now),
            (NodeMode::Active, ModeSource::Document)
        );
        assert!(!state.is_draining(&config, now));
        assert_eq!(state.node_name(), "pgprox-2");
    }

    #[test]
    fn the_document_drains_a_node_and_says_so() {
        // An operator asking why a node is draining needs to know whether to
        // edit the document or to wait.
        let (state, now) = state();
        let config = config_with("pgprox-2", NodeMode::Draining);

        assert_eq!(
            state.mode(&config, now),
            (NodeMode::Draining, ModeSource::Document)
        );
    }

    #[test]
    fn an_overlay_drains_a_node_the_document_calls_active() {
        // The incident case: somebody needs this node out of rotation now, and
        // a pull request is not the tool.
        let (mut state, now) = state();
        let config = Config::default();

        state.set(NodeMode::Draining, Some(Duration::from_secs(600)), now);
        assert_eq!(
            state.mode(&config, now),
            (NodeMode::Draining, ModeSource::Overlay)
        );
    }

    #[test]
    fn an_overlay_expires_on_its_own() {
        // A node drained at 2am during an incident is still drained at 9am
        // otherwise, and nobody can tell whether that was deliberate.
        let (mut state, now) = state();
        let config = Config::default();
        let ttl = Duration::from_secs(600);

        state.set(NodeMode::Draining, Some(ttl), now);
        let just_before = (now + ttl).checked_sub(Duration::from_secs(1)).unwrap();
        assert!(state.is_draining(&config, just_before));
        assert!(
            !state.is_draining(&config, now + ttl),
            "the overlay outlived its TTL"
        );
        assert_eq!(
            state.mode(&config, now + ttl),
            (NodeMode::Active, ModeSource::Document),
            "an expired overlay did not hand back to the document"
        );
    }

    #[test]
    fn an_expired_overlay_is_ignored_whether_or_not_anything_swept_it() {
        // Expiry is read rather than swept, so forgetting to reap cannot leave
        // a node drained past its time.
        let (mut state, now) = state();
        let config = Config::default();
        let ttl = Duration::from_secs(60);
        state.set(NodeMode::Draining, Some(ttl), now);

        let later = now + ttl + Duration::from_secs(1);
        assert!(!state.is_draining(&config, later));
        assert_eq!(state.remaining(later), None);
        assert_eq!(state.expires_at(later), None);

        state.reap(later);
        assert!(!state.is_draining(&config, later));
    }

    #[test]
    fn a_document_drain_does_not_expire() {
        // The declarative one is reviewed and survives a restart, so there is
        // nothing to forget.
        let (state, now) = state();
        let config = config_with("pgprox-2", NodeMode::Draining);

        assert!(state.is_draining(&config, now + Duration::from_secs(86_400 * 365)));
    }

    #[test]
    fn an_overlay_cannot_undo_a_drain_the_document_asked_for() {
        // That would let an API call quietly reverse a reviewed change, and the
        // next config poll would flip it back anyway, so the node would
        // oscillate.
        let (mut state, now) = state();
        let config = config_with("pgprox-2", NodeMode::Draining);

        state.set(NodeMode::Active, Some(Duration::from_secs(600)), now);
        assert_eq!(
            state.mode(&config, now),
            (NodeMode::Draining, ModeSource::Document),
            "an API call undrained a node the document drains"
        );
    }

    #[test]
    fn clearing_an_overlay_returns_to_the_document() {
        let (mut state, now) = state();
        let config = Config::default();

        state.set(NodeMode::Draining, None, now);
        assert!(state.is_draining(&config, now));

        state.clear();
        assert!(!state.is_draining(&config, now));
        assert_eq!(state.remaining(now), None);
    }

    #[test]
    fn a_caller_that_gives_no_ttl_gets_the_configured_default() {
        let config = DrainConfig::default();
        let mut state = DrainState::new("pgprox-2", config);
        let now = Instant::now();

        let expires = state.set(NodeMode::Draining, None, now);
        assert_eq!(expires, now + config.default_ttl);
        assert_eq!(state.remaining(now), Some(config.default_ttl));
    }

    #[test]
    fn an_over_long_ttl_is_clamped_rather_than_refused() {
        // A caller asking for a week wants the node drained. Refusing outright
        // during an incident helps nobody; the clamp is reported back so they
        // can see what they actually got.
        let config = DrainConfig::default();
        let mut state = DrainState::new("pgprox-2", config);
        let now = Instant::now();

        let expires = state.set(
            NodeMode::Draining,
            Some(Duration::from_secs(86_400 * 7)),
            now,
        );
        assert_eq!(expires, now + config.max_ttl);
        assert_eq!(state.remaining(now), Some(config.max_ttl));
    }

    #[test]
    fn setting_an_overlay_twice_replaces_it_rather_than_extending_forever() {
        let (mut state, now) = state();
        state.set(NodeMode::Draining, Some(Duration::from_secs(600)), now);
        let second = state.set(NodeMode::Draining, Some(Duration::from_secs(60)), now);

        assert_eq!(
            state.remaining(now),
            Some(Duration::from_secs(60)),
            "a shorter TTL did not replace a longer one"
        );
        assert_eq!(second, now + Duration::from_secs(60));
    }

    #[test]
    fn there_is_no_way_to_set_an_overlay_that_never_expires() {
        // The ambiguity that would have bitten at M6: this API and
        // Observatory::drain must not disagree about what an absent TTL means.
        // Every path through `set` produces an expiry.
        let config = DrainConfig::default();
        let mut state = DrainState::new("pgprox-2", config);
        let now = Instant::now();

        for ttl in [None, Some(Duration::from_secs(1)), Some(Duration::MAX)] {
            state.set(NodeMode::Draining, ttl, now);
            let remaining = state
                .remaining(now)
                .unwrap_or_else(|| panic!("{ttl:?} produced an overlay with no expiry"));
            assert!(
                remaining <= config.max_ttl,
                "{ttl:?} produced an overlay lasting {remaining:?}"
            );
        }
    }

    #[test]
    fn the_default_ttl_matches_the_one_the_admin_api_applies() {
        // pgprox-admin cannot depend on this crate, so it repeats the value.
        // This is the test that stops the two drifting: an API drain and a
        // configured one must last the same time, or an operator's mental model
        // is wrong for one of them.
        assert_eq!(
            DrainConfig::default().default_ttl,
            Duration::from_secs(30 * 60),
            "pgprox-admin's DEFAULT_DRAIN_TTL mirrors this; change both together"
        );
    }

    #[test]
    fn the_defaults_resolve_a_forgotten_drain_inside_a_working_day() {
        // The whole point of the TTL. A default measured in days would be no
        // better than none.
        let config = DrainConfig::default();
        assert!(config.default_ttl <= Duration::from_secs(60 * 60));
        assert!(config.max_ttl <= Duration::from_secs(24 * 60 * 60));
        assert!(config.default_ttl <= config.max_ttl);
    }
}
