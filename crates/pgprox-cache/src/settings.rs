//! The session settings a cached answer depends on.
//!
//! # Why this is a list rather than "everything the session set"
//!
//! A session may set any parameter on `pgprox-pool`'s replay allowlist without
//! pinning, and it keeps that setting across a connection change. Keying on all
//! of them would be safe and would also be useless: `application_name` is on
//! that list and is routinely set per process, so every application instance
//! would get its own copy of every entry and the cache would hold one hit's
//! worth of everything.
//!
//! Keying on none of them is what was happening, and it is wrong for six of
//! them. So the list is the settings that change the answer, and the reason
//! each is here is written beside it.
//!
//! # This is about bytes, not rows
//!
//! What the store holds is the server's reply verbatim, so the question is not
//! "would another session see the same rows" but "would it see the same bytes".
//! `TimeZone` changes neither the rows nor which rows; it changes how every
//! `timestamptz` in them is written down, and a client handed somebody else's
//! rendering is being told the wrong time.
//!
//! # What is deliberately absent
//!
//! `application_name`, for the reason above: it reaches no answer, and
//! `current_setting` is refused by the cacheability rule, which is the only way
//! a statement could read it.
//!
//! `statement_timeout` and `lock_timeout` decide whether an answer arrives, not
//! what it says. A cached hit does not run, so it cannot time out, and serving
//! one to a session with a short timeout is the fast case rather than the wrong
//! one.
//!
//! `default_transaction_isolation`, `default_transaction_read_only` and
//! `default_transaction_deferrable` apply to transactions, and a statement
//! inside one is refused by [`crate::cacheable()`] before a key is built.
//!
//! `role` and `session_authorization` are absent from the list because they are
//! absent from the *replay* allowlist: a session that sets either is pinned,
//! and a pinned session is not cacheable. That is the right outcome by an
//! accident of two lists agreeing, so it is written down here where somebody
//! adding `role` to the replay list would read it.

/// Settings that change what a cached answer is, or how it reads.
///
/// Lowercase, because that is how `SessionParams` records a name.
///
/// Order is the key's canonical order and must not be sorted at the call site:
/// two sessions that set the same things have to produce the same string, and
/// the cheapest way to guarantee that is for the order to come from here.
pub const ANSWER_SHAPING: &[&str] = &[
    // What the SQL names. The original member, and the only one that changes
    // which rows come back rather than how they are written.
    "search_path",
    // Every `timestamptz` is rendered in it.
    "timezone",
    // `2026-08-10` or `10.08.2026`, for every date in the answer.
    "datestyle",
    // How an `interval` is spelled out.
    "intervalstyle",
    // How many digits a `float4` or `float8` is given.
    "extra_float_digits",
    // `\x00` or the escape form, for every `bytea`.
    "bytea_output",
    // The encoding of every string in the answer, which is to say of most of
    // the bytes in most answers. The worst of these to share: a client that
    // asked for LATIN1 and is handed UTF-8 gets a decode error or mojibake.
    "client_encoding",
    // Not a rendering setting. It changes what a backslash means inside a
    // string literal, so the same SQL text is a different statement under it.
    "standard_conforming_strings",
];

/// A canonical string naming what this session set, for [`CacheKey::settings`].
///
/// `lookup` answers what the session has set a parameter to, or [`None`] where
/// it has not set it and is therefore on the server's default, which every
/// session on that backend shares.
///
/// Empty when the session has set none of them, which is the common case and
/// costs nothing to key on.
///
/// ```
/// use pgprox_cache::settings::fingerprint;
///
/// assert_eq!(fingerprint(|_| None), "");
/// assert_eq!(
///     fingerprint(|name| (name == "timezone").then_some("UTC")),
///     "timezone=3:UTC"
/// );
/// ```
///
/// [`CacheKey::settings`]: pgprox_core::cache::CacheKey::settings
#[must_use]
pub fn fingerprint<'a>(lookup: impl Fn(&str) -> Option<&'a str>) -> String {
    let mut out = String::new();
    for name in ANSWER_SHAPING {
        let Some(value) = lookup(name) else {
            continue;
        };
        // Length-prefixed, not separated. A separator alone is forgeable: with
        // `name=value` joined by newlines, a session setting `TimeZone` to
        // `UTC\ndatestyle=ISO` produces the same string as one that set both,
        // and the two would share an entry. The value is a tenant's own text
        // and every delimiter it could hold is a delimiter it can write, so the
        // length is what makes this injective rather than the punctuation.
        //
        // Found by the test that asserts it, which failed against the
        // newline-joined version this replaced.
        out.push_str(name);
        out.push('=');
        // Bytes rather than chars: two different strings can agree on char
        // count and never on byte length plus bytes.
        out.push_str(&value.len().to_string());
        out.push(':');
        out.push_str(value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lookup over a fixed set of pairs.
    fn from<'a>(
        pairs: &'a [(&'a str, &'static str)],
    ) -> impl Fn(&str) -> Option<&'static str> + use<'a> {
        move |name| {
            pairs
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map(|(_, value)| *value)
        }
    }

    #[test]
    fn a_session_that_set_nothing_fingerprints_to_nothing() {
        // The common case, and the one that has to stay cheap: most sessions
        // set nothing and share the server's defaults with each other.
        assert_eq!(fingerprint(from(&[])), "");
    }

    #[test]
    fn every_shaping_setting_reaches_the_fingerprint() {
        // A list nothing reads is the same bug as no list. Each name is checked
        // by itself so a missing one names itself in the failure.
        for name in ANSWER_SHAPING {
            let printed = fingerprint(from(&[(name, "value")]));
            assert_eq!(printed, format!("{name}=5:value"), "{name} was dropped");
        }
    }

    #[test]
    fn two_sessions_that_set_the_same_things_agree() {
        // Whatever order they set them in. This is the property the key rests
        // on: same settings, same string, same entry.
        let one = fingerprint(from(&[("timezone", "UTC"), ("datestyle", "ISO")]));
        let other = fingerprint(from(&[("datestyle", "ISO"), ("timezone", "UTC")]));
        assert_eq!(one, other);
        assert!(one.contains("timezone=3:UTC") && one.contains("datestyle=3:ISO"));
    }

    #[test]
    fn two_sessions_that_differ_in_one_setting_do_not() {
        // The bug. These two sessions have the same tenant, database, role,
        // statement and search path, and their answers render every timestamp
        // differently.
        assert_ne!(
            fingerprint(from(&[("timezone", "UTC")])),
            fingerprint(from(&[("timezone", "America/New_York")]))
        );
    }

    #[test]
    fn a_setting_nobody_keys_on_is_not_in_the_fingerprint() {
        // `application_name` is replayable and is set per process by half the
        // drivers in existence. Keying on it would give every application
        // instance a private copy of every entry.
        assert_eq!(fingerprint(from(&[("application_name", "worker-17")])), "");
        assert!(!ANSWER_SHAPING.contains(&"application_name"));
    }

    #[test]
    fn a_value_cannot_forge_another_sessions_fingerprint() {
        // The separators are structural rather than parsed, so a value holding
        // one produces a different string rather than a colliding one.
        let honest = fingerprint(from(&[("timezone", "UTC"), ("datestyle", "ISO")]));
        let forged = fingerprint(from(&[("timezone", "UTC\ndatestyle=ISO")]));
        assert_ne!(honest, forged);
    }
}
