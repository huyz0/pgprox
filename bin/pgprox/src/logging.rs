//! What the node says, and what it must never say.
//!
//! # Why this is a module rather than three lines in main
//!
//! Two of the rules in `standards/observability.md` are enforceable only where
//! the subscriber is built: what the default filter is, and that a credential
//! cannot reach a line. A subscriber configured inline in `main.rs` would put
//! both in the one file no test can call.
//!
//! # JSON when there is no terminal
//!
//! A pod's logs are read by a collector, and a developer's are read by a
//! developer. The format follows the destination rather than a flag nobody
//! remembers to set, which is the same reasoning as colour in the check
//! scripts.
//!
//! # Field names come from `pgprox-observe`
//!
//! `spans::field` is the list, and [`pgprox_observe::spans::is_recordable`]
//! is the rule about which of them may carry a value. Anything logged here
//! uses those names, so a query across the fleet's logs is a query over one
//! vocabulary rather than over whatever each call site called it.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing_subscriber::EnvFilter;

/// The filter used when `RUST_LOG` says nothing.
///
/// `info` for the proxy and `warn` for everything else. A dependency logging
/// at info into a proxy's log stream is noise an operator has to filter out
/// under load, which is when they can least afford to.
pub const DEFAULT_FILTER: &str = "info,pgprox=info,tower=warn,hyper=warn,h2=warn,rustls=warn";

/// Whether a subscriber has been installed.
///
/// A process may only install one, and a test binary runs many tests in one
/// process. Without this the second test to call [`init`] panics inside the
/// tracing crate, which is a failure with nothing to do with what was under
/// test.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Installs the process-wide subscriber.
///
/// Idempotent: the second call does nothing and says so by returning `false`.
pub fn init() -> bool {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return false;
    }

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    // A terminal means a person is reading. Anything else is a collector, and a
    // collector wants one JSON object per line.
    let to_terminal = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true);

    if to_terminal {
        builder.init();
    } else {
        builder.json().flatten_event(true).init();
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_observe::spans::{Span, field, is_recordable};

    #[test]
    fn every_field_this_binary_logs_is_one_the_conventions_name() {
        // The point of a shared vocabulary is that a query over the fleet's
        // logs is a query over one set of names. A field invented at a call
        // site is one nobody else can search for.
        for field in [
            field::TENANT,
            field::NODE,
            field::CONN,
            field::SQLSTATE,
            field::DURATION_MS,
        ] {
            assert!(
                is_recordable(field),
                "{field} is logged by this crate and the conventions refuse it"
            );
        }
    }

    #[test]
    fn a_credential_field_is_refused_by_the_conventions() {
        // Enforced in `pgprox-observe` and asserted here, because this is the
        // crate that does the logging and the rule is worth failing loudly in
        // the place it would be broken.
        for field in ["password", "token", "authorization", "secret"] {
            assert!(!is_recordable(field), "{field} was recordable");
        }
    }

    #[test]
    fn the_default_filter_names_this_crate() {
        // A default that left the proxy at `warn` would mean a node that
        // started, drained and refused clients in silence.
        assert!(DEFAULT_FILTER.contains("pgprox=info"));
    }

    #[test]
    fn the_spans_this_binary_opens_are_in_the_registry() {
        // A span name built from data has unbounded cardinality, so the names
        // are a fixed list and this is what holds the binary to it.
        for span in [Span::Connection, Span::Gossip, Span::ConfigReload] {
            assert!(Span::all().contains(&span));
        }
    }

    #[test]
    fn installing_twice_is_not_a_panic() {
        // A test binary runs many tests in one process, and a second install
        // would panic inside tracing with nothing to do with what was tested.
        let first = init();
        let second = init();
        assert!(!second, "the second install was not refused");

        // And the first did install. `M17.4`: `init` returning a constant
        // `false` survived, because this used to leave `first` unasserted on
        // the grounds that test ordering decided it. It does not: this is the
        // only caller of `init` in the test binary, so within this process the
        // first call is the first call whether the suite runs one test per
        // process or many per thread. A node whose subscriber never installs
        // starts and serves in complete silence.
        assert!(first, "the first install was refused");
    }
}
