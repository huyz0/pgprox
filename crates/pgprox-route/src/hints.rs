//! Explicit routing instructions from the client.
//!
//! Two forms, because they answer different needs. `SET pgprox.route` changes
//! the session until it is changed back, which suits an application that knows
//! a whole connection is for reporting. A leading `/* pgprox:replica */`
//! comment applies to one statement, which suits an ORM that cannot easily
//! issue a `SET` but can prepend a comment.
//!
//! # What a hint can and cannot do
//!
//! A hint is a request, not an assertion. It says where the client would prefer
//! the statement to go; it does not claim the statement is safe there, and it
//! cannot, because the client does not know how far each replica has replayed.
//!
//! So [`RouteHint::Replica`] can admit a statement the classifier called
//! [`Unknown`](pgprox_core::route::StmtClass::Unknown), which is the client
//! saying it knows more about its own SQL than a lexical scan does. It can never
//! admit a [`Write`](pgprox_core::route::StmtClass::Write), and it never
//! overrides the watermark check. Both of
//! those live in [`pgprox_core::route::decide`], which is the single place the
//! rule is written down.
//!
//! [`RouteHint::Primary`] has no such limit. Asking for the primary is always
//! honoured, because it is always safe.

use pgprox_core::route::RouteHint;

/// The prefix a per-statement hint comment must start with.
const COMMENT_PREFIX: &str = "pgprox:";

/// The parameter name a session-scoped hint is set through.
///
/// A `pgprox.` prefix rather than a bare name: Postgres accepts assignment to
/// any dotted parameter it does not recognise, so this reaches the proxy
/// without the server rejecting it, and it cannot collide with a real setting.
pub const ROUTE_PARAMETER: &str = "pgprox.route";

/// Parses a hint value, as written on the right of `SET pgprox.route = ...`.
///
/// Returns [`None`] for anything unrecognised rather than a default, so a
/// caller can tell "the client asked for something we do not understand" from
/// "the client asked for auto" and reject the former. Silently accepting a
/// typo would leave a client believing its reads were on replicas.
///
/// ```
/// use pgprox_core::route::RouteHint;
/// use pgprox_route::hints::parse_hint_value;
///
/// assert_eq!(parse_hint_value("replica"), Some(RouteHint::Replica));
/// assert_eq!(parse_hint_value("'primary'"), Some(RouteHint::Primary));
/// assert_eq!(parse_hint_value("relpica"), None);
/// ```
#[must_use]
pub fn parse_hint_value(value: &str) -> Option<RouteHint> {
    let trimmed = unquote(value.trim());
    match trimmed.to_ascii_lowercase().as_str() {
        "auto" | "default" => Some(RouteHint::Auto),
        "primary" | "writer" | "master" => Some(RouteHint::Primary),
        "replica" | "reader" | "standby" => Some(RouteHint::Replica),
        _ => None,
    }
}

/// Strips one layer of single or double quotes, as `SET` values may carry.
fn unquote(value: &str) -> &str {
    for quote in ['\'', '"'] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// What a statement did to the session's route setting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RouteAssignment {
    /// `SET pgprox.route = <value>`, with a value that parsed.
    Set(RouteHint),
    /// `SET pgprox.route = <value>` where the value meant nothing.
    ///
    /// Distinguished from no assignment at all so the caller can tell the
    /// client it made a mistake. Failing loudly matters here: the alternative
    /// is a client that believes its reads are on replicas and never finds out
    /// otherwise.
    Invalid,
    /// `RESET pgprox.route`, or `RESET ALL`, which returns to `Auto`.
    Reset,
}

/// Reads a `SET` or `RESET` of the route parameter.
///
/// Returns [`None`] for any other statement, including a `SET` of something
/// else. The caller consumes what this recognises rather than forwarding it:
/// Postgres would accept `pgprox.route` as a custom parameter and store it, but
/// nothing there reads it, and a value in two places that can disagree is worse
/// than a value in one.
///
/// Recognising it here also keeps it out of the pin path. A `SET` outside the
/// replayable allowlist pins the session, and this one must not, since it
/// changes no server-side state at all.
///
/// ```
/// use pgprox_core::route::RouteHint;
/// use pgprox_route::hints::{RouteAssignment, parse_route_assignment};
///
/// assert_eq!(
///     parse_route_assignment("SET pgprox.route = 'replica'"),
///     Some(RouteAssignment::Set(RouteHint::Replica)),
/// );
/// assert_eq!(parse_route_assignment("RESET pgprox.route"), Some(RouteAssignment::Reset));
/// assert_eq!(parse_route_assignment("SET work_mem = '1MB'"), None);
/// ```
#[must_use]
pub fn parse_route_assignment(sql: &str) -> Option<RouteAssignment> {
    let mut words = sql.split_whitespace();
    let verb = words.next()?;

    if verb.eq_ignore_ascii_case("reset") {
        let target = words.next()?;
        // `RESET ALL` clears every parameter, this one included.
        let matched = target.eq_ignore_ascii_case(ROUTE_PARAMETER)
            || (target.eq_ignore_ascii_case("all") && words.next().is_none());
        return matched.then_some(RouteAssignment::Reset);
    }

    if !verb.eq_ignore_ascii_case("set") {
        return None;
    }

    // `SET pgprox.route = x`, `SET pgprox.route TO x`, and the unspaced
    // `SET pgprox.route=x` all reach the server, so all three reach here.
    let rest = sql.get(verb.len()..)?.trim_start();
    let rest = strip_prefix_ignore_ascii_case(rest, ROUTE_PARAMETER)?;
    let rest = rest.trim_start();

    // `SET pgprox.routex = ...` names a different parameter, so a separator
    // has to follow the name for this to be an assignment to ours.
    let value = match rest.strip_prefix('=') {
        Some(after) => after,
        None => strip_prefix_ignore_ascii_case(rest, "to")?,
    };

    // A trailing semicolon is part of the statement, not of the value.
    let value = value.trim().trim_end_matches(';').trim();
    Some(parse_hint_value(value).map_or(RouteAssignment::Invalid, RouteAssignment::Set))
}

/// Strips a prefix case-insensitively, returning what follows.
fn strip_prefix_ignore_ascii_case<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = input.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &input[prefix.len()..])
}

/// Reads a per-statement hint from a leading comment.
///
/// Only leading comments count, and only up to the first real SQL token. A
/// `/* pgprox:replica */` in the middle of a statement, or inside a string, is
/// data as far as this is concerned. Scanning the whole statement would let a
/// row's contents change where a query runs.
///
/// ```
/// use pgprox_core::route::RouteHint;
/// use pgprox_route::hints::statement_hint;
///
/// assert_eq!(statement_hint("/* pgprox:replica */ SELECT 1"), Some(RouteHint::Replica));
/// assert_eq!(statement_hint("SELECT '/* pgprox:replica */'"), None);
/// ```
#[must_use]
pub fn statement_hint(sql: &str) -> Option<RouteHint> {
    let mut rest = sql;
    loop {
        let trimmed = rest.trim_start();
        if trimmed.len() != rest.len() {
            rest = trimmed;
            continue;
        }

        if let Some(after) = rest.strip_prefix("--") {
            let (line, remainder) = after.split_once('\n').unwrap_or((after, ""));
            if let Some(hint) = hint_from_comment(line) {
                return Some(hint);
            }
            rest = remainder;
            continue;
        }

        if rest.starts_with("/*") {
            // Nesting is Postgres behaviour, and a hint is only a hint at the
            // outermost level: `/* /* pgprox:replica */ */` is a comment about
            // a comment.
            let (body, remainder) = split_block_comment(rest)?;
            if let Some(hint) = hint_from_comment(body) {
                return Some(hint);
            }
            rest = remainder;
            continue;
        }

        // Real SQL has started. Anything further along is not a leading
        // comment, whatever it says.
        return None;
    }
}

/// Splits a leading block comment into its body and what follows it.
///
/// [`None`] if it is never closed. The server rejects that as a syntax error,
/// so there is no statement for a hint to apply to, and taking routing
/// instructions from malformed input is a habit worth not starting.
fn split_block_comment(input: &str) -> Option<(&str, &str)> {
    let bytes = input.as_bytes();
    let mut depth = 0_u32;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"/*") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b"*/") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                // Body excludes the delimiters at both ends.
                return Some((&input[2..i - 2], &input[i..]));
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Reads `pgprox:<value>` out of a comment body.
fn hint_from_comment(body: &str) -> Option<RouteHint> {
    let trimmed = body.trim();
    let value = trimmed.strip_prefix(COMMENT_PREFIX)?;
    // A nested comment's delimiters would otherwise end up in the value.
    parse_hint_value(value.trim_end_matches(['*', '/']).trim())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pgprox_core::ids::Lsn;
    use pgprox_core::route::{ReplicaState, RouteCtx, RouteTarget, StmtClass, decide};

    #[test]
    fn the_three_values_parse_with_their_aliases() {
        assert_eq!(parse_hint_value("auto"), Some(RouteHint::Auto));
        assert_eq!(parse_hint_value("default"), Some(RouteHint::Auto));
        for primary in ["primary", "writer", "master", "PRIMARY", "Primary"] {
            assert_eq!(
                parse_hint_value(primary),
                Some(RouteHint::Primary),
                "{primary}"
            );
        }
        for replica in ["replica", "reader", "standby", "REPLICA"] {
            assert_eq!(
                parse_hint_value(replica),
                Some(RouteHint::Replica),
                "{replica}"
            );
        }
    }

    #[test]
    fn quotes_and_whitespace_around_the_value_are_stripped() {
        for value in ["'replica'", "\"replica\"", "  replica  ", " 'replica' "] {
            assert_eq!(
                parse_hint_value(value),
                Some(RouteHint::Replica),
                "{value:?}"
            );
        }
    }

    #[test]
    fn an_unrecognised_value_is_none_rather_than_a_default() {
        // A typo must be reportable. Quietly treating `relpica` as `auto` would
        // leave a client believing its reads were on replicas.
        for value in ["relpica", "", "'", "replicas", "primary replica", "1"] {
            assert_eq!(parse_hint_value(value), None, "{value:?}");
        }
    }

    #[test]
    fn a_leading_comment_carries_a_hint() {
        assert_eq!(
            statement_hint("/* pgprox:replica */ SELECT 1"),
            Some(RouteHint::Replica)
        );
        assert_eq!(
            statement_hint("/*pgprox:primary*/SELECT 1"),
            Some(RouteHint::Primary)
        );
        assert_eq!(
            statement_hint("-- pgprox:replica\nSELECT 1"),
            Some(RouteHint::Replica)
        );
        assert_eq!(
            statement_hint("  \n /* pgprox:replica */ \n SELECT 1"),
            Some(RouteHint::Replica)
        );
    }

    #[test]
    fn a_hint_after_the_sql_has_started_is_not_a_hint() {
        // Scanning the whole statement would let a row's contents change where
        // a query runs, since a hint could then arrive inside a literal.
        assert_eq!(statement_hint("SELECT 1 /* pgprox:replica */"), None);
        assert_eq!(statement_hint("SELECT '/* pgprox:replica */'"), None);
        assert_eq!(
            statement_hint("SELECT * FROM t WHERE note = '-- pgprox:replica'"),
            None
        );
    }

    #[test]
    fn several_leading_comments_are_all_considered() {
        // ORMs and query loggers both prepend comments, so a hint is rarely the
        // only one there.
        assert_eq!(
            statement_hint("/* app:orders */ /* pgprox:replica */ SELECT 1"),
            Some(RouteHint::Replica)
        );
        assert_eq!(
            statement_hint("-- traceparent: 00-abc\n/* pgprox:primary */ SELECT 1"),
            Some(RouteHint::Primary)
        );
    }

    #[test]
    fn an_unrelated_comment_yields_no_hint() {
        assert_eq!(statement_hint("/* just a comment */ SELECT 1"), None);
        assert_eq!(statement_hint("SELECT 1"), None);
        assert_eq!(statement_hint(""), None);
    }

    #[test]
    fn an_unrecognised_hint_value_in_a_comment_is_no_hint() {
        assert_eq!(statement_hint("/* pgprox:relpica */ SELECT 1"), None);
        assert_eq!(statement_hint("/* pgprox: */ SELECT 1"), None);
    }

    #[test]
    fn a_nested_comment_does_not_end_early_or_smuggle_a_hint() {
        // The outer comment is what a hint would be in. A hint inside a nested
        // one is a comment about a comment.
        assert_eq!(
            statement_hint("/* outer /* inner */ still outer */ SELECT 1"),
            None
        );
        assert_eq!(
            statement_hint("/* /* pgprox:replica */ */ SELECT 1"),
            None,
            "a nested hint was honoured"
        );
    }

    #[test]
    fn an_unterminated_comment_yields_no_hint() {
        // The server rejects it as a syntax error, so there is no statement to
        // route. Taking instructions from malformed input is a habit worth not
        // starting.
        assert_eq!(statement_hint("/* pgprox:replica"), None);
        assert_eq!(statement_hint("/*"), None);

        // A well-formed comment earlier in the statement still counts. The
        // rule is about not reading a hint out of malformed text, not about
        // discarding one that was written properly.
        assert_eq!(
            statement_hint("/* pgprox:replica */ /* unterminated"),
            Some(RouteHint::Replica)
        );
    }

    #[test]
    fn a_set_of_the_route_parameter_is_recognised_in_its_spellings() {
        for sql in [
            "SET pgprox.route = 'replica'",
            "SET pgprox.route='replica'",
            "SET pgprox.route TO replica",
            "set PGPROX.ROUTE to 'REPLICA'",
            "SET   pgprox.route   =   replica  ;",
        ] {
            assert_eq!(
                parse_route_assignment(sql),
                Some(RouteAssignment::Set(RouteHint::Replica)),
                "{sql:?}"
            );
        }
    }

    #[test]
    fn a_bad_value_is_reported_rather_than_ignored() {
        // The client must be able to find out. Treating a typo as no
        // assignment leaves it believing its reads are on replicas.
        assert_eq!(
            parse_route_assignment("SET pgprox.route = 'relpica'"),
            Some(RouteAssignment::Invalid)
        );
        assert_eq!(
            parse_route_assignment("SET pgprox.route = "),
            Some(RouteAssignment::Invalid)
        );
    }

    #[test]
    fn a_reset_returns_to_auto() {
        assert_eq!(
            parse_route_assignment("RESET pgprox.route"),
            Some(RouteAssignment::Reset)
        );
        assert_eq!(
            parse_route_assignment("reset ALL"),
            Some(RouteAssignment::Reset),
            "RESET ALL clears every parameter, this one included"
        );
    }

    #[test]
    fn another_parameter_is_left_alone() {
        // Anything not recognised here is forwarded upstream untouched.
        for sql in [
            "SET work_mem = '1MB'",
            "SET pgprox.routex = 'replica'",
            "SET pgprox.other = 'replica'",
            "RESET work_mem",
            "RESET ALL EXTRA",
            "SELECT 1",
            "SET",
            "",
        ] {
            assert_eq!(parse_route_assignment(sql), None, "{sql:?}");
        }
    }

    #[test]
    fn a_replica_hint_admits_an_unknown_statement_but_never_a_write() {
        // The one thing a hint buys: the client asserting it knows its own SQL
        // better than a lexical scan. It stops there.
        let replica = [ReplicaState {
            replayed: Lsn::new(100),
            healthy: true,
        }];

        let unknown = RouteCtx {
            class: StmtClass::Unknown,
            hint: RouteHint::Replica,
            ..RouteCtx::default()
        };
        assert_eq!(decide(&unknown, &replica), RouteTarget::Replica(0));

        let write = RouteCtx {
            class: StmtClass::Write,
            hint: RouteHint::Replica,
            ..RouteCtx::default()
        };
        assert_eq!(
            decide(&write, &replica),
            RouteTarget::Primary,
            "a hint sent a write to a replica"
        );
    }

    #[test]
    fn a_replica_hint_never_overrides_the_watermark() {
        // Read-your-writes outranks a preference. The client cannot know how
        // far a replica has replayed, so its hint cannot speak to this.
        let behind = [ReplicaState {
            replayed: Lsn::new(499),
            healthy: true,
        }];
        let ctx = RouteCtx {
            class: StmtClass::ReadOnly,
            hint: RouteHint::Replica,
            watermark: Some(Lsn::new(500)),
            ..RouteCtx::default()
        };
        assert_eq!(
            decide(&ctx, &behind),
            RouteTarget::Primary,
            "a hint overrode the watermark and served a stale read"
        );
    }

    #[test]
    fn a_primary_hint_is_always_honoured() {
        // Asking for the primary is always safe, so it has none of the limits
        // the replica hint has.
        let replica = [ReplicaState {
            replayed: Lsn::new(9_999),
            healthy: true,
        }];
        let ctx = RouteCtx {
            class: StmtClass::ReadOnly,
            hint: RouteHint::Primary,
            ..RouteCtx::default()
        };
        assert_eq!(decide(&ctx, &replica), RouteTarget::Primary);
    }
}
