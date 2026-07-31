//! Session parameters, recorded once and replayed onto whichever connection
//! the session lands on next.
//!
//! # Why this exists
//!
//! `SET search_path = tenant_acme` is one of the most common things an
//! application does on connect, and under transaction pooling the connection it
//! ran on is gone by the next statement. Without replay every such session
//! would have to pin, and pinning on `search_path` alone would cost most of the
//! multiplexing ratio.
//!
//! So a small set of parameters is recorded here and reissued on acquire.
//! Everything else pins, in [`crate::pin`].
//!
//! # Only what actually differs
//!
//! Replay compares against the target connection's current values and issues
//! only the parameters that differ. A warm connection that already has the
//! right `search_path` costs nothing, which matters because acquire is a
//! declared hot path and a session with eight recorded parameters would
//! otherwise pay eight round trips to reach its first query.
//!
//! # `SET LOCAL` is not recorded
//!
//! It is scoped to the transaction, so by the time the connection is released
//! it has already been undone. Recording it would replay a setting the client
//! deliberately made temporary onto a connection where its transaction no
//! longer exists.

use std::collections::BTreeMap;

use crate::pin::Replayable;

/// A session's recorded parameters.
///
/// Ordered, so replay is deterministic and two connections given the same
/// session receive the same statements in the same order. Debugging a
/// parameter-related bug across two nodes is hard enough without the ordering
/// varying between them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionParams {
    values: BTreeMap<String, String>,
}

/// What a statement did to the session's parameters.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ParamChange {
    /// A parameter was recorded, and must be replayed from now on.
    Recorded {
        /// The normalised parameter name.
        name: String,
    },
    /// A parameter was cleared, so it returns to the server's default.
    Cleared {
        /// The normalised parameter name.
        name: String,
    },
    /// Every parameter was cleared, as `RESET ALL` and `DISCARD ALL` do.
    ClearedAll,
    /// The statement was transaction-scoped, so nothing was recorded.
    ///
    /// Distinguished from "not a `SET`" so a caller can tell that a `SET LOCAL`
    /// was understood and deliberately ignored rather than missed.
    TransactionScoped,
}

impl SessionParams {
    /// A session with no recorded parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many parameters are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The recorded value of a parameter.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(&normalise(name)).map(String::as_str)
    }

    /// Records a parameter directly.
    ///
    /// Used for values learned from a `ParameterStatus` rather than from
    /// client SQL, which is how `client_encoding` and `DateStyle` arrive.
    pub fn record(&mut self, name: &str, value: &str) {
        self.values.insert(normalise(name), value.to_owned());
    }

    /// Clears one parameter.
    pub fn clear(&mut self, name: &str) {
        self.values.remove(&normalise(name));
    }

    /// Clears everything.
    pub fn clear_all(&mut self) {
        self.values.clear();
    }

    /// Records what a statement did, if it was a parameter change worth
    /// recording.
    ///
    /// Only parameters on `allowlist` are recorded. Anything else is left
    /// alone here and pins the session instead, so a parameter is never both
    /// replayed and pinned on, and never neither.
    pub fn observe_statement(&mut self, sql: &str, allowlist: Replayable) -> Option<ParamChange> {
        let statement = ParsedSet::parse(sql)?;
        match statement {
            ParsedSet::Local => Some(ParamChange::TransactionScoped),
            ParsedSet::ResetAll => {
                self.clear_all();
                Some(ParamChange::ClearedAll)
            }
            ParsedSet::Reset(name) => {
                let name = normalise(&name);
                if !allowed(&name, allowlist) {
                    return None;
                }
                self.clear(&name);
                Some(ParamChange::Cleared { name })
            }
            ParsedSet::Set(name, value) => {
                let name = normalise(&name);
                if !allowed(&name, allowlist) {
                    return None;
                }
                self.record(&name, &value);
                Some(ParamChange::Recorded { name })
            }
        }
    }

    /// The statements needed to make `current` match this session.
    ///
    /// Only the differences, so a warm connection that already matches costs
    /// nothing. A parameter this session recorded and the connection does not
    /// have is set; one the connection carries from a previous session and this
    /// one never mentioned is reset, because leaving it would leak the previous
    /// session's state into this one.
    #[must_use]
    pub fn replay_onto(&self, current: &Self) -> Vec<String> {
        let mut statements = Vec::new();

        for (name, value) in &self.values {
            if current.values.get(name) != Some(value) {
                statements.push(format!("SET {name} = {}", quote(value)));
            }
        }

        // The direction that is easy to forget. A connection carrying
        // `search_path` from whoever used it last would silently give this
        // session another tenant's schema.
        for name in current.values.keys() {
            if !self.values.contains_key(name) {
                statements.push(format!("RESET {name}"));
            }
        }

        statements
    }

    /// The recorded parameters, in replay order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Whether a parameter may be replayed rather than pinned on.
fn allowed(name: &str, allowlist: Replayable) -> bool {
    allowlist.contains(name)
}

/// Parameter names are case-insensitive in Postgres, so `TimeZone` and
/// `timezone` are one parameter and must not become two entries that replay
/// twice and disagree.
fn normalise(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

/// Quotes a value for reissuing.
///
/// Anything not a bare identifier or a plain number is single-quoted, with
/// embedded quotes doubled. A `search_path` of `"my schema"` has to survive the
/// round trip intact, and this text is built from client input, so an unescaped
/// quote here would be the proxy issuing SQL a client composed.
fn quote(value: &str) -> String {
    let bare = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
    if bare {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "''"))
}

/// A parsed parameter statement.
enum ParsedSet {
    /// `SET x = y`, or `SET x TO y`.
    Set(String, String),
    /// `RESET x`.
    Reset(String),
    /// `RESET ALL`, or `DISCARD ALL`.
    ResetAll,
    /// `SET LOCAL ...`, or another transaction-scoped form.
    Local,
}

impl ParsedSet {
    /// Reads a parameter statement, or [`None`] if it is not one.
    fn parse(sql: &str) -> Option<Self> {
        let trimmed = strip_leading_trivia(sql);
        let mut words = trimmed.split_whitespace();
        let verb = words.next()?.to_ascii_lowercase();

        if verb == "discard" {
            // `DISCARD ALL` resets parameters among much else. The narrower
            // forms leave them alone.
            let what = words.next()?.to_ascii_lowercase();
            return (what == "all").then_some(Self::ResetAll);
        }

        if verb == "reset" {
            let name = words.next()?;
            if name.eq_ignore_ascii_case("all") {
                return Some(Self::ResetAll);
            }
            return Some(Self::Reset(name.trim_end_matches(';').to_owned()));
        }

        if verb != "set" {
            return None;
        }

        // Everything after `SET`, so `SET x=y` with no spaces still parses.
        let rest = trimmed.get(verb.len()..)?.trim_start();
        let (scope, rest) = split_word(rest);

        let rest = if scope.eq_ignore_ascii_case("local") {
            // Transaction-scoped, and gone before the connection is released.
            return Some(Self::Local);
        } else if scope.eq_ignore_ascii_case("session") {
            rest.trim_start()
        } else {
            trimmed.get(verb.len()..)?.trim_start()
        };

        let (name, rest) = split_name(rest);
        if name.is_empty() {
            return None;
        }
        // `SET TRANSACTION ...` and `SET CONSTRAINTS ...` are not parameters.
        if name.eq_ignore_ascii_case("transaction") || name.eq_ignore_ascii_case("constraints") {
            return Some(Self::Local);
        }

        let rest = rest.trim_start();
        let value = if let Some(after) = rest.strip_prefix('=') {
            after
        } else {
            let (word, after) = split_word(rest);
            if !word.eq_ignore_ascii_case("to") {
                return None;
            }
            after
        };

        let value = value.trim().trim_end_matches(';').trim();
        Some(Self::Set(name.to_owned(), unquote(value).to_owned()))
    }
}

/// Splits off the first whitespace-delimited word.
fn split_word(input: &str) -> (&str, &str) {
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    (&input[..end], &input[end..])
}

/// Splits off a parameter name, which ends at whitespace or `=`.
fn split_name(input: &str) -> (&str, &str) {
    let end = input
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(input.len());
    (&input[..end], &input[end..])
}

/// Strips leading comments and whitespace.
///
/// Delegates to [`pgprox_core::sql`], which owns where comments end. This
/// module used to carry a third copy of that logic; see that module's docs for
/// what having two of them cost.
fn strip_leading_trivia(sql: &str) -> &str {
    let mut lexer = pgprox_core::sql::Lexer::new(sql);
    lexer.skip_trivia();
    lexer.rest()
}

/// Strips one layer of quotes from a value.
fn unquote(value: &str) -> &str {
    for quote in ['\'', '"'] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// `M14.21`. Three mutants survived in this file, all in the two functions
    /// that decide how a value is written back out. That matters more than a
    /// parser usually would: these run when a session's `SET` state is replayed
    /// onto a fresh upstream connection, which is what makes transaction
    /// pooling transparent. Quoting a value wrongly is not a crash. It is a
    /// session that comes back with a different setting than it asked for.
    #[test]
    fn a_value_is_left_bare_only_when_every_character_allows_it() {
        // `quote`'s guard is a chain of alternatives:
        //   is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
        // Turning any `||` into `&&` makes the test unsatisfiable, because no
        // character is alphanumeric *and* an underscore, so every value gets
        // quoted. Nothing noticed, because no test asked for a bare value
        // containing a dot or a dash.
        for bare in [
            "utf8",
            "read_committed",
            "on",
            "ISO",
            "9",
            "a_b",
            "1.5",        // the `.` alternative
            "read-only",  // the `-` alternative
            "UTC.0-1_x9", // all four at once
        ] {
            assert_eq!(quote(bare), bare, "{bare} should need no quoting");
        }

        // And anything else is quoted, with embedded quotes doubled.
        assert_eq!(quote(""), "''");
        assert_eq!(quote("has space"), "'has space'");
        assert_eq!(quote("it's"), "'it''s'");
        assert_eq!(quote("semi;colon"), "'semi;colon'");
    }

    #[test]
    fn only_a_properly_paired_quote_is_stripped() {
        // `unquote` requires length, a leading quote and a matching trailing
        // one, all three. Any `&&` becoming `||` accepts a value that is not
        // actually quoted and then slices a character off each end regardless.
        assert_eq!(unquote("'utf8'"), "utf8");
        assert_eq!(unquote("\"utf8\""), "utf8");

        // Unpaired: nothing is stripped. This is the case the mutants pass.
        assert_eq!(unquote("'utf8"), "'utf8");
        assert_eq!(unquote("utf8'"), "utf8'");
        assert_eq!(unquote("\"utf8"), "\"utf8");

        // Mismatched kinds are not a pair either.
        assert_eq!(unquote("'utf8\""), "'utf8\"");

        // Too short to be a pair. A single quote character is its own start
        // and end, and stripping it would slice past the string.
        assert_eq!(unquote("'"), "'");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(unquote(""), "");

        // The empty quoted string is a pair, and unwraps to nothing.
        assert_eq!(unquote("''"), "");
    }

    fn observe(params: &mut SessionParams, sql: &str) -> Option<ParamChange> {
        params.observe_statement(sql, Replayable::DEFAULT)
    }

    #[test]
    fn a_set_of_an_allowed_parameter_is_recorded() {
        let mut params = SessionParams::new();
        assert_eq!(
            observe(&mut params, "SET search_path = tenant_acme"),
            Some(ParamChange::Recorded {
                name: "search_path".to_owned()
            })
        );
        assert_eq!(params.get("search_path"), Some("tenant_acme"));
        assert_eq!(params.len(), 1);
        assert!(!params.is_empty());
    }

    #[test]
    fn the_spellings_a_client_actually_uses_all_parse() {
        for sql in [
            "SET search_path = tenant_acme",
            "SET search_path='tenant_acme'",
            "SET search_path TO tenant_acme",
            "set SEARCH_PATH to 'tenant_acme'",
            "SET SESSION search_path = tenant_acme",
            "  SET   search_path   =   tenant_acme  ;",
            "/* orm comment */ SET search_path = tenant_acme",
        ] {
            let mut params = SessionParams::new();
            observe(&mut params, sql);
            assert_eq!(params.get("search_path"), Some("tenant_acme"), "{sql:?}");
        }
    }

    #[test]
    fn parameter_names_are_case_insensitive() {
        // Otherwise TimeZone and timezone become two entries that replay twice
        // and disagree about the answer.
        let mut params = SessionParams::new();
        observe(&mut params, "SET TimeZone = 'UTC'");
        observe(&mut params, "SET timezone = 'Europe/London'");
        assert_eq!(params.len(), 1, "one parameter became two entries");
        assert_eq!(params.get("TIMEZONE"), Some("Europe/London"));
    }

    #[test]
    fn a_parameter_outside_the_allowlist_is_not_recorded() {
        // It pins instead. A parameter must never be both replayed and pinned
        // on, and never neither.
        let mut params = SessionParams::new();
        assert_eq!(observe(&mut params, "SET work_mem = '256MB'"), None);
        assert!(params.is_empty());

        assert_eq!(
            crate::pin::pin_reason("SET work_mem = '256MB'", Replayable::DEFAULT),
            Some(crate::pin::PinReason::UnreplayableSet),
            "a parameter was neither replayed nor pinned on"
        );
    }

    #[test]
    fn every_replayable_parameter_is_recorded_rather_than_pinning() {
        // The other direction of the same invariant, across the whole list.
        for name in Replayable::DEFAULT.names() {
            let sql = format!("SET {name} = 'x'");
            let mut params = SessionParams::new();
            assert!(
                observe(&mut params, &sql).is_some(),
                "{name} is on the allowlist but was not recorded"
            );
            assert_eq!(
                crate::pin::pin_reason(&sql, Replayable::DEFAULT),
                None,
                "{name} was recorded and also pinned"
            );
        }
    }

    #[test]
    fn set_local_is_understood_and_deliberately_not_recorded() {
        // It is undone by the time the connection is released, so replaying it
        // would reissue a setting the client made temporary onto a connection
        // where its transaction no longer exists.
        let mut params = SessionParams::new();
        assert_eq!(
            observe(&mut params, "SET LOCAL search_path = other"),
            Some(ParamChange::TransactionScoped)
        );
        assert!(params.is_empty());
    }

    #[test]
    fn transaction_scoped_forms_are_not_parameters() {
        let mut params = SessionParams::new();
        assert_eq!(
            observe(&mut params, "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"),
            Some(ParamChange::TransactionScoped)
        );
        assert_eq!(
            observe(&mut params, "SET CONSTRAINTS ALL DEFERRED"),
            Some(ParamChange::TransactionScoped)
        );
        assert!(params.is_empty());
    }

    #[test]
    fn reset_clears_one_and_reset_all_clears_everything() {
        let mut params = SessionParams::new();
        observe(&mut params, "SET search_path = acme");
        observe(&mut params, "SET TimeZone = 'UTC'");
        assert_eq!(params.len(), 2);

        assert_eq!(
            observe(&mut params, "RESET search_path"),
            Some(ParamChange::Cleared {
                name: "search_path".to_owned()
            })
        );
        assert_eq!(params.len(), 1);

        assert_eq!(
            observe(&mut params, "RESET ALL"),
            Some(ParamChange::ClearedAll)
        );
        assert!(params.is_empty());
    }

    #[test]
    fn discard_all_clears_parameters_and_narrower_discards_do_not() {
        let mut params = SessionParams::new();
        observe(&mut params, "SET search_path = acme");

        assert_eq!(observe(&mut params, "DISCARD PLANS"), None);
        assert_eq!(params.len(), 1, "DISCARD PLANS cleared parameters");

        assert_eq!(
            observe(&mut params, "DISCARD ALL"),
            Some(ParamChange::ClearedAll)
        );
        assert!(params.is_empty());
    }

    #[test]
    fn an_ordinary_statement_changes_nothing() {
        let mut params = SessionParams::new();
        for sql in ["SELECT 1", "BEGIN", "", "SET", "RESET", "DISCARD"] {
            assert_eq!(observe(&mut params, sql), None, "{sql:?}");
        }
        assert!(params.is_empty());
    }

    #[test]
    fn replay_issues_only_what_differs() {
        // Acquire is a hot path. A session with eight parameters would
        // otherwise pay eight round trips before its first query.
        let mut session = SessionParams::new();
        session.record("search_path", "acme");
        session.record("timezone", "UTC");

        let mut warm = SessionParams::new();
        warm.record("search_path", "acme");
        warm.record("timezone", "UTC");
        assert!(
            session.replay_onto(&warm).is_empty(),
            "a connection that already matched was reconfigured anyway"
        );

        let mut partial = SessionParams::new();
        partial.record("search_path", "acme");
        assert_eq!(session.replay_onto(&partial), vec!["SET timezone = UTC"]);
    }

    #[test]
    fn replay_resets_what_the_previous_session_left_behind() {
        // The direction that is easy to forget. A connection still carrying
        // search_path from whoever used it last would silently hand this
        // session another tenant's schema.
        let session = SessionParams::new();
        let mut used = SessionParams::new();
        used.record("search_path", "someone_else");

        assert_eq!(
            session.replay_onto(&used),
            vec!["RESET search_path"],
            "another tenant's search_path survived into this session"
        );
    }

    #[test]
    fn replay_is_deterministic() {
        // Two nodes given the same session must issue the same statements in
        // the same order, or a parameter bug reproduces on one and not the
        // other.
        let mut session = SessionParams::new();
        session.record("timezone", "UTC");
        session.record("search_path", "acme");
        session.record("application_name", "app");

        let empty = SessionParams::new();
        let first = session.replay_onto(&empty);
        assert_eq!(first, session.replay_onto(&empty));
        assert_eq!(
            first,
            vec![
                "SET application_name = app",
                "SET search_path = acme",
                "SET timezone = UTC",
            ]
        );
    }

    #[test]
    fn a_value_needing_quotes_gets_them_and_keeps_its_content() {
        // This text becomes SQL the proxy issues, built from client input. An
        // unescaped quote here would be the proxy running SQL a client wrote.
        let mut session = SessionParams::new();
        session.record("search_path", "my schema");
        session.record("application_name", "it's mine");

        let statements = session.replay_onto(&SessionParams::new());
        assert_eq!(
            statements,
            vec![
                "SET application_name = 'it''s mine'",
                "SET search_path = 'my schema'",
            ]
        );
    }

    #[test]
    fn a_value_that_needs_no_quotes_does_not_get_them() {
        let mut session = SessionParams::new();
        session.record("search_path", "public");
        session.record("statement_timeout", "5000");
        session.record("timezone", "Europe/London");

        let statements = session.replay_onto(&SessionParams::new());
        assert!(
            statements.contains(&"SET search_path = public".to_owned()),
            "{statements:?}"
        );
        assert!(statements.contains(&"SET statement_timeout = 5000".to_owned()));
        assert!(
            statements.contains(&"SET timezone = 'Europe/London'".to_owned()),
            "a slash is not bare, so the value must be quoted: {statements:?}"
        );
    }

    #[test]
    fn an_empty_value_is_quoted_rather_than_emitted_bare() {
        // `SET search_path = ` is a syntax error; `SET search_path = ''` is a
        // real and different thing a client can ask for.
        let mut session = SessionParams::new();
        session.record("search_path", "");
        assert_eq!(
            session.replay_onto(&SessionParams::new()),
            vec!["SET search_path = ''"]
        );
    }

    #[test]
    fn a_quoted_value_is_stored_unquoted() {
        // Storing the quotes would double them on replay.
        let mut params = SessionParams::new();
        observe(&mut params, "SET search_path = 'acme'");
        assert_eq!(params.get("search_path"), Some("acme"));
        assert_eq!(
            params.replay_onto(&SessionParams::new()),
            vec!["SET search_path = acme"]
        );
    }

    #[test]
    fn recorded_parameters_can_be_read_back_in_order() {
        let mut params = SessionParams::new();
        params.record("timezone", "UTC");
        params.record("search_path", "acme");

        let pairs: Vec<(&str, &str)> = params.iter().collect();
        assert_eq!(pairs, vec![("search_path", "acme"), ("timezone", "UTC")]);
    }

    #[test]
    fn clearing_by_hand_matches_clearing_by_statement() {
        let mut a = SessionParams::new();
        a.record("search_path", "acme");
        a.clear("SEARCH_PATH");
        assert!(a.is_empty(), "clear did not normalise the name");

        let mut b = SessionParams::new();
        b.record("search_path", "acme");
        b.clear_all();
        assert_eq!(a, b);
    }
}
