//! Deciding when a session must keep its upstream connection.
//!
//! # Pinning is not the release rule
//!
//! `pgprox_proto::session::SessionState` already answers whether *this moment*
//! is safe to release at: transaction status, extended-query sequence,
//! COPY in progress. That is a question about now, and it resolves itself.
//!
//! This module answers a different one. Some features attach state to the
//! connection itself, so that from the moment they are used, no moment is safe
//! any more. A `LISTEN` registers interest on one backend; hand the session a
//! different connection and its notifications simply never arrive. A temp table
//! lives in one backend's temp schema; the next statement cannot find it.
//!
//! Once pinned, a session stays pinned until it disconnects. There is no
//! unpinning: `UNLISTEN *` looks like it should undo a `LISTEN`, but a
//! notification may already be queued, and a temp table survives until the
//! session ends whatever else happens. Guessing wrong here hands a client
//! someone else's state.
//!
//! # Why this costs so much, and what pays for it
//!
//! A pinned session holds an upstream connection for its entire life, which is
//! exactly what transaction pooling exists to avoid. Every pin moves the ratio
//! back toward session pooling, which is why `pgprox_pin_total{reason}` is
//! instrumented by reason: a rising pin rate is the early warning that
//! multiplexing is degrading, and the reason says which feature to go and look
//! at. See ADR 0001.
//!
//! Protocol-level prepared statements are the one big case *not* here, and that
//! is what makes the rest affordable. Every modern driver uses named `Parse`,
//! so pinning on it would pin nearly every real session. Handled by mapping
//! instead, by the statement map. See ADR 0011.

use pgprox_core::ids::TenantId;

/// Why a session is pinned to one upstream connection.
///
/// The label on `pgprox_pin_total{reason}`, so an operator asking "why is
/// multiplexing degrading" gets an answer rather than a number.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[non_exhaustive]
pub enum PinReason {
    /// `LISTEN`, or a `NotificationResponse` arriving from the server.
    ///
    /// Notifications are delivered to the backend that registered the interest.
    /// A session that moved would silently stop receiving them, which looks
    /// like the feature is broken rather than like the proxy is at fault.
    Listen,
    /// A session-scoped advisory lock.
    ///
    /// Held until unlocked or the session ends, so it belongs to a connection
    /// rather than a transaction. The `_xact_` variants release at commit and
    /// do not pin.
    AdvisoryLock,
    /// A temporary table, or anything else in the temp schema.
    ///
    /// It lives in one backend's temp schema and is invisible from any other
    /// connection.
    TempTable,
    /// A cursor declared `WITH HOLD`.
    ///
    /// It outlives its transaction on purpose, which means it outlives the
    /// point at which the connection would otherwise be released.
    WithHold,
    /// SQL-level `PREPARE`, as opposed to a protocol-level `Parse`.
    ///
    /// Named at the SQL level, so the proxy cannot rewrite it the way it
    /// rewrites protocol statements, and the name lives on one backend.
    Prepare,
    /// `SET` of a parameter outside the replayable allowlist.
    ///
    /// The allowlist is replayed on acquire; anything else is not, and a
    /// session that moved would silently lose the setting.
    UnreplayableSet,
    /// A `COPY` stream is in progress.
    ///
    /// Naturally pinned until the stream ends. Included so the reason is
    /// reportable, since an operator seeing a held connection wants to know.
    Copy,
    /// The session asked for it, through `SET pgprox.pin`.
    ///
    /// An escape hatch for a tenant using something this list has not learned.
    /// Better than the alternative, which is them discovering the gap as data
    /// corruption.
    Requested,
}

impl PinReason {
    /// The metric label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Listen => "listen",
            Self::AdvisoryLock => "advisory_lock",
            Self::TempTable => "temp_table",
            Self::WithHold => "with_hold",
            Self::Prepare => "prepare",
            Self::UnreplayableSet => "unreplayable_set",
            Self::Copy => "copy",
            Self::Requested => "requested",
        }
    }
}

/// Whether a session is pinned, and why.
///
/// Once set, never cleared. See the module docs: no observed statement proves a
/// session has stopped needing its connection, and guessing wrong hands a
/// client someone else's state.
#[derive(Clone, Debug, Default)]
pub struct PinState {
    reason: Option<PinReason>,
    tenant: Option<TenantId>,
}

impl PinState {
    /// An unpinned session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the session is pinned.
    #[must_use]
    pub const fn is_pinned(&self) -> bool {
        self.reason.is_some()
    }

    /// Why, if it is.
    ///
    /// The first reason, not the latest. A session that pins on `LISTEN` and
    /// later creates a temp table was already unmovable, and reporting the temp
    /// table would send an operator after the wrong feature.
    #[must_use]
    pub const fn reason(&self) -> Option<PinReason> {
        self.reason
    }

    /// Which tenant's session this is, for the metric.
    #[must_use]
    pub const fn tenant(&self) -> Option<&TenantId> {
        self.tenant.as_ref()
    }

    /// Attaches a tenant, so a pin can be attributed.
    pub fn set_tenant(&mut self, tenant: TenantId) {
        self.tenant = Some(tenant);
    }

    /// Pins the session, keeping the first reason if already pinned.
    ///
    /// Returns whether this call was the one that pinned it, so the caller
    /// increments `pgprox_pin_total` exactly once per session.
    pub fn pin(&mut self, reason: PinReason) -> bool {
        if self.reason.is_some() {
            return false;
        }
        self.reason = Some(reason);
        true
    }

    /// Inspects a statement and pins if it requires it.
    ///
    /// Returns the reason if this statement was the one that pinned.
    pub fn observe_statement(&mut self, sql: &str, allowlist: &[&str]) -> Option<PinReason> {
        if self.is_pinned() {
            // Already unmovable. Scanning further would cost time on every
            // statement of a session that has nothing left to learn.
            return None;
        }
        let reason = pin_reason(sql, allowlist)?;
        self.pin(reason).then_some(reason)
    }

    /// Records a `NotificationResponse` arriving from the server.
    ///
    /// A session can receive notifications without ever issuing `LISTEN`
    /// itself, since a trigger or another session's `NOTIFY` reaches whoever is
    /// listening. If one arrives, this connection is the one registered.
    pub fn observe_notification(&mut self) -> Option<PinReason> {
        self.pin(PinReason::Listen).then_some(PinReason::Listen)
    }

    /// Records that a `COPY` stream has begun.
    pub fn observe_copy(&mut self) -> Option<PinReason> {
        self.pin(PinReason::Copy).then_some(PinReason::Copy)
    }
}

/// Parameters replayed on acquire rather than pinning the session.
///
/// Each is a session setting the proxy can reproduce on a different connection
/// by issuing the same `SET`. Anything not here changes state the proxy cannot
/// see or cannot reproduce, so a session that used it cannot be moved.
///
/// Kept deliberately small. Every addition is a promise that replaying the
/// parameter is enough, and a wrong promise is a session silently losing a
/// setting rather than an error anyone sees.
pub const REPLAYABLE_PARAMETERS: &[&str] = &[
    "search_path",
    "timezone",
    "application_name",
    "statement_timeout",
    "datestyle",
    "extra_float_digits",
    "client_encoding",
    "lock_timeout",
    "idle_in_transaction_session_timeout",
    "default_transaction_isolation",
    "default_transaction_read_only",
    "default_transaction_deferrable",
    "intervalstyle",
    "bytea_output",
];

/// Why this statement pins the session, if it does.
///
/// A lexical scan, like the classifier: it must be right about which text is
/// SQL and which is data, and it errs toward pinning. A false pin costs one
/// session's share of multiplexing. A missed pin hands a client another
/// client's temp table.
#[must_use]
pub fn pin_reason(sql: &str, allowlist: &[&str]) -> Option<PinReason> {
    // Every statement, not just the first. The simple query protocol allows
    // several in one message, so checking only the leading word would let
    // `SELECT 1; LISTEN c` through unpinned, and the session would silently
    // stop receiving its notifications.
    statements_of(sql)
        .into_iter()
        .find_map(|words| statement_pin_reason(&words, allowlist))
}

/// Why one statement pins, given its words.
fn statement_pin_reason(words: &[String], allowlist: &[&str]) -> Option<PinReason> {
    let first = words.first()?.as_str();

    // `LISTEN` and `UNLISTEN` both pin. Unlistening does not undo a pin: a
    // notification may already be queued for this backend.
    if first == "listen" || first == "unlisten" {
        return Some(PinReason::Listen);
    }

    // SQL-level PREPARE, which is not the protocol-level Parse that mapping
    // handles. `PREPARE TRANSACTION` is two-phase commit and is a different
    // thing entirely, but it also cannot be multiplexed.
    if first == "prepare" {
        return Some(PinReason::Prepare);
    }

    // `DECLARE ... CURSOR WITH HOLD` outlives its transaction. A cursor without
    // WITH HOLD dies at commit, so it needs no pin.
    if first == "declare" && has_adjacent_pair(words, "with", "hold") {
        return Some(PinReason::WithHold);
    }

    // Temp tables, and anything else in the temp schema. A qualified name
    // arrives as one word, so `pg_temp.t` needs the prefix check rather than
    // an equality one.
    if words
        .iter()
        .any(|w| w == "temp" || w == "temporary" || w == "pg_temp" || w.starts_with("pg_temp."))
    {
        return Some(PinReason::TempTable);
    }

    // Session-scoped advisory locks. The `_xact_` variants release at commit,
    // so they are excluded by name rather than by a prefix match.
    if words.iter().any(|w| is_session_advisory_lock(w)) {
        return Some(PinReason::AdvisoryLock);
    }

    // A SET the proxy cannot replay elsewhere. `SET LOCAL` is
    // transaction-scoped and disappears at commit, so it never pins.
    if first == "set" {
        return set_pin_reason(words, allowlist);
    }

    None
}

/// Whether a `SET` pins, given the replay allowlist.
fn set_pin_reason(words: &[String], allowlist: &[&str]) -> Option<PinReason> {
    let mut rest = &words[1..];

    // `SET SESSION x` is the same as `SET x`.
    if rest.first().is_some_and(|w| w == "session") {
        rest = &rest[1..];
    } else if rest.first().is_some_and(|w| w == "local") {
        // Transaction-scoped, gone at commit.
        return None;
    }

    let name = rest.first()?;

    // `SET TRANSACTION ...` and `SET CONSTRAINTS ...` are transaction-scoped.
    if name == "transaction" || name == "constraints" {
        return None;
    }

    // The proxy's own settings never reach the server and change no server-side
    // state, so they must not pin.
    if name.starts_with("pgprox.") {
        return None;
    }

    if allowlist.iter().any(|allowed| name == allowed) {
        return None;
    }

    Some(PinReason::UnreplayableSet)
}

/// Whether a word names a session-scoped advisory lock function.
fn is_session_advisory_lock(word: &str) -> bool {
    // `_xact_` locks release at commit and do not pin. Checking for the
    // infix rather than listing every function keeps this correct as
    // Postgres adds variants.
    word.starts_with("pg_advisory") && !word.contains("_xact_")
        || word.starts_with("pg_try_advisory") && !word.contains("_xact_")
}

/// Whether two words appear next to each other, in order.
fn has_adjacent_pair(words: &[String], first: &str, second: &str) -> bool {
    words.windows(2).any(|w| w[0] == first && w[1] == second)
}

/// The lowercase bare words of each statement, skipping comments and quoted
/// text.
///
/// Same reasoning as the classifier's scanner, and the same danger: text that
/// is data must not be read as SQL, and a quoted region whose end is misjudged
/// swallows what follows it. Splitting on `;` matters for the same reason it
/// does there, and a `;` inside a string must not split anything.
fn statements_of(sql: &str) -> Vec<Vec<String>> {
    let mut statements = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut rest = sql;

    loop {
        let trimmed = rest.trim_start();
        if trimmed.len() != rest.len() {
            rest = trimmed;
            continue;
        }
        let Some(first) = rest.chars().next() else {
            if !words.is_empty() {
                statements.push(words);
            }
            return statements;
        };

        if first == ';' {
            if !words.is_empty() {
                statements.push(std::mem::take(&mut words));
            }
            rest = &rest[1..];
            continue;
        }

        if rest.starts_with("--") {
            rest = rest.find('\n').map_or("", |i| &rest[i + 1..]);
            continue;
        }
        if rest.starts_with("/*") {
            rest = skip_block_comment(rest);
            continue;
        }
        if first == '\'' || first == '"' {
            rest = skip_quoted(rest, first);
            continue;
        }
        if first == '$' {
            let (skipped, remainder) = skip_dollar_quoted(rest);
            if skipped {
                rest = remainder;
                continue;
            }
            rest = &rest[first.len_utf8()..];
            continue;
        }

        if is_word_char(first) {
            let end = rest.find(|c: char| !is_word_char(c)).unwrap_or(rest.len());
            words.push(rest[..end].to_ascii_lowercase());
            rest = &rest[end..];
            continue;
        }

        rest = &rest[first.len_utf8()..];
    }
}

/// Whether a character can appear in a bare word.
///
/// A dot is included so `pgprox.route` and `pg_catalog.nextval` arrive as one
/// word, which is what lets the `SET` check compare against a qualified name.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || !c.is_ascii()
}

/// Skips a block comment, which nests in Postgres.
fn skip_block_comment(input: &str) -> &str {
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
                break;
            }
        } else {
            i += 1;
        }
    }
    &input[i.min(input.len())..]
}

/// Skips a quoted region, honouring the doubled-quote escape.
fn skip_quoted(input: &str, quote: char) -> &str {
    let bytes = input.as_bytes();
    let quote = quote as u8;
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            i += 1;
            break;
        }
        i += 1;
    }
    &input[i.min(input.len())..]
}

/// Skips a dollar-quoted string, reporting whether one was there.
///
/// A tag follows the rules for an unquoted identifier, so `$1` is a placeholder
/// rather than a tag. Getting this wrong swallows the rest of the statement,
/// which is the bug the classifier's property test found in M5.7.
fn skip_dollar_quoted(input: &str) -> (bool, &str) {
    let after = &input[1..];
    let Some(offset) = after.find('$') else {
        return (false, input);
    };
    let inner = &after[..offset];
    let valid = inner.chars().next().is_none_or(|first| {
        (first.is_alphabetic() || first == '_' || !first.is_ascii())
            && inner
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || !c.is_ascii())
    });
    if !valid {
        return (false, input);
    }

    let tag = &input[..offset + 2];
    let body_at = tag.len();
    let end = input[body_at..]
        .find(tag)
        .map_or(input.len(), |i| body_at + i + tag.len());
    (true, &input[end..])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn reason(sql: &str) -> Option<PinReason> {
        pin_reason(sql, REPLAYABLE_PARAMETERS)
    }

    #[test]
    fn listen_and_unlisten_both_pin() {
        // Unlistening does not undo the pin: a notification may already be
        // queued for this backend.
        assert_eq!(reason("LISTEN channel"), Some(PinReason::Listen));
        assert_eq!(reason("listen chan"), Some(PinReason::Listen));
        assert_eq!(reason("UNLISTEN channel"), Some(PinReason::Listen));
        assert_eq!(reason("UNLISTEN *"), Some(PinReason::Listen));
    }

    #[test]
    fn a_notification_from_the_server_pins_even_without_a_listen() {
        // A trigger or another session's NOTIFY reaches whoever is registered.
        // If one arrives here, this connection is the one registered.
        let mut state = PinState::new();
        assert_eq!(state.observe_notification(), Some(PinReason::Listen));
        assert!(state.is_pinned());
    }

    #[test]
    fn notify_alone_does_not_pin() {
        // Sending a notification attaches nothing to this backend. Only
        // listening does.
        assert_eq!(reason("NOTIFY channel"), None);
        assert_eq!(reason("SELECT pg_notify('c', 'p')"), None);
    }

    #[test]
    fn a_session_advisory_lock_pins_and_a_transaction_one_does_not() {
        // The distinction that matters: `_xact_` locks release at commit, so
        // they live and die inside a transaction the pool already holds for.
        for sql in [
            "SELECT pg_advisory_lock(1)",
            "SELECT pg_advisory_lock_shared(1)",
            "SELECT pg_try_advisory_lock(1)",
            "SELECT pg_advisory_unlock(1)",
        ] {
            assert_eq!(reason(sql), Some(PinReason::AdvisoryLock), "{sql}");
        }

        for sql in [
            "SELECT pg_advisory_xact_lock(1)",
            "SELECT pg_advisory_xact_lock_shared(1)",
            "SELECT pg_try_advisory_xact_lock(1)",
        ] {
            assert_eq!(reason(sql), None, "{sql} pinned but releases at commit");
        }
    }

    #[test]
    fn a_temp_table_pins() {
        for sql in [
            "CREATE TEMP TABLE t (a int)",
            "CREATE TEMPORARY TABLE t (a int)",
            "create temp table if not exists t (a int)",
            "SELECT * FROM pg_temp.t",
            "CREATE TEMP VIEW v AS SELECT 1",
        ] {
            assert_eq!(reason(sql), Some(PinReason::TempTable), "{sql}");
        }
    }

    #[test]
    fn a_with_hold_cursor_pins_and_a_plain_one_does_not() {
        // WITH HOLD outlives its transaction, which is exactly the point at
        // which the connection would otherwise be released.
        assert_eq!(
            reason("DECLARE c CURSOR WITH HOLD FOR SELECT 1"),
            Some(PinReason::WithHold)
        );
        assert_eq!(
            reason("declare c cursor with hold for select 1"),
            Some(PinReason::WithHold)
        );
        assert_eq!(
            reason("DECLARE c CURSOR FOR SELECT 1"),
            None,
            "a cursor that dies at commit does not need a pin"
        );
        assert_eq!(reason("DECLARE c CURSOR WITHOUT HOLD FOR SELECT 1"), None);
    }

    #[test]
    fn sql_level_prepare_pins() {
        // Named at the SQL level, so the proxy cannot rewrite it the way it
        // rewrites protocol-level statements.
        assert_eq!(reason("PREPARE p AS SELECT 1"), Some(PinReason::Prepare));
        assert_eq!(
            reason("PREPARE TRANSACTION 'x'"),
            Some(PinReason::Prepare),
            "two-phase commit cannot be multiplexed either"
        );
    }

    #[test]
    fn a_replayable_set_does_not_pin() {
        // These are reproduced on the next connection by issuing the same SET,
        // which is what makes them affordable.
        for sql in [
            "SET search_path = public",
            "SET TimeZone = 'UTC'",
            "set application_name to 'app'",
            "SET statement_timeout = 5000",
            "SET SESSION search_path = public",
        ] {
            assert_eq!(reason(sql), None, "{sql}");
        }
    }

    #[test]
    fn an_unreplayable_set_pins() {
        for sql in [
            "SET work_mem = '256MB'",
            "SET role = admin",
            "SET session_authorization = other",
            "SET custom.setting = 1",
        ] {
            assert_eq!(reason(sql), Some(PinReason::UnreplayableSet), "{sql}");
        }
    }

    #[test]
    fn set_local_never_pins() {
        // Transaction-scoped, so it is gone by the time the connection would
        // be released.
        assert_eq!(reason("SET LOCAL work_mem = '256MB'"), None);
        assert_eq!(reason("set local role = admin"), None);
    }

    #[test]
    fn transaction_scoped_set_forms_do_not_pin() {
        assert_eq!(reason("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"), None);
        assert_eq!(reason("SET CONSTRAINTS ALL DEFERRED"), None);
    }

    #[test]
    fn the_proxys_own_settings_never_pin() {
        // They never reach the server and change no server-side state, so a
        // session that used one is still perfectly movable.
        assert_eq!(reason("SET pgprox.route = 'replica'"), None);
        assert_eq!(reason("SET pgprox.anything = 1"), None);
    }

    #[test]
    fn an_ordinary_statement_does_not_pin() {
        for sql in [
            "SELECT * FROM orders",
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = 1",
            "BEGIN",
            "COMMIT",
            "",
            "   ",
        ] {
            assert_eq!(reason(sql), None, "{sql:?}");
        }
    }

    #[test]
    fn a_keyword_inside_a_string_or_comment_does_not_pin() {
        // Same danger as the classifier: text that is data must not be read as
        // SQL. Here a false pin costs one session's multiplexing rather than
        // correctness, but at 100k sessions it is not a small cost.
        for sql in [
            "SELECT * FROM t WHERE a = 'LISTEN'",
            "SELECT * FROM t WHERE a = 'CREATE TEMP TABLE x'",
            "-- LISTEN channel\nSELECT 1",
            "/* CREATE TEMP TABLE t */ SELECT 1",
            "SELECT $$ LISTEN channel $$",
            r#"SELECT "temp" FROM t"#,
        ] {
            assert_eq!(reason(sql), None, "{sql:?}");
        }
    }

    #[test]
    fn several_statements_in_one_message_are_all_checked() {
        // The simple query protocol allows several statements per message.
        // Checking only the leading word would let this through unpinned, and
        // the session would silently stop receiving its notifications.
        assert_eq!(reason("SELECT 1; LISTEN c"), Some(PinReason::Listen));
        assert_eq!(
            reason("SELECT 1; CREATE TEMP TABLE t (a int); SELECT 2"),
            Some(PinReason::TempTable)
        );
        assert_eq!(reason("SELECT 1; SELECT 2;"), None);
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_start_a_statement() {
        // Otherwise a row's contents could pin a session, which is a denial of
        // service a tenant could trigger with their own data.
        assert_eq!(reason("SELECT 'x; LISTEN c'"), None);
        assert_eq!(
            reason("SELECT 'x; y'; LISTEN c"),
            Some(PinReason::Listen),
            "a real second statement was swallowed by a string"
        );
    }

    #[test]
    fn an_invalid_dollar_tag_does_not_swallow_a_later_statement() {
        // The M5.7 bug, in this scanner too. `$1 AS a, $` is not a valid tag,
        // and accepting it would consume the rest of the message, hiding the
        // LISTEN and leaving the session unpinned. That direction costs
        // correctness rather than throughput.
        assert_eq!(
            reason("SELECT $1 AS a, $2 AS b; LISTEN c"),
            Some(PinReason::Listen)
        );
    }

    #[test]
    fn a_valid_dollar_quoted_body_is_still_data() {
        assert_eq!(reason("SELECT $$ LISTEN c $$"), None);
        assert_eq!(reason("SELECT $tag$ LISTEN c $tag$"), None);
        assert_eq!(
            reason("SELECT $$ x $$; LISTEN c"),
            Some(PinReason::Listen),
            "a statement after a dollar-quoted string was lost"
        );
    }

    #[test]
    fn an_unterminated_construct_terminates() {
        for sql in [
            "SELECT 'unterminated",
            "SELECT \"unterminated",
            "SELECT $$unterminated",
            "/* unterminated",
            "$",
            "--",
        ] {
            let _ = reason(sql);
        }
    }

    #[test]
    fn the_first_reason_is_the_one_kept() {
        // A session that pins on LISTEN and later makes a temp table was
        // already unmovable. Reporting the temp table would send an operator
        // after the wrong feature.
        let mut state = PinState::new();
        assert_eq!(
            state.observe_statement("LISTEN c", REPLAYABLE_PARAMETERS),
            Some(PinReason::Listen)
        );
        assert_eq!(
            state.observe_statement("CREATE TEMP TABLE t (a int)", REPLAYABLE_PARAMETERS),
            None,
            "an already-pinned session reported a second pin"
        );
        assert_eq!(state.reason(), Some(PinReason::Listen));
    }

    #[test]
    fn pinning_reports_once_so_the_metric_counts_sessions() {
        let mut state = PinState::new();
        assert!(
            state.pin(PinReason::Listen),
            "the first pin was not reported"
        );
        assert!(!state.pin(PinReason::TempTable));
        assert!(!state.pin(PinReason::Listen));
        assert_eq!(state.reason(), Some(PinReason::Listen));
    }

    #[test]
    fn a_pin_is_never_lifted() {
        // UNLISTEN * looks like it should undo a LISTEN, and a temp table
        // survives whatever else happens. There is no statement that proves a
        // session has stopped needing its connection.
        let mut state = PinState::new();
        state.observe_statement("LISTEN c", REPLAYABLE_PARAMETERS);
        state.observe_statement("UNLISTEN *", REPLAYABLE_PARAMETERS);
        state.observe_statement("SELECT 1", REPLAYABLE_PARAMETERS);
        assert!(state.is_pinned());
    }

    #[test]
    fn a_copy_stream_pins_with_its_own_reason() {
        let mut state = PinState::new();
        assert_eq!(state.observe_copy(), Some(PinReason::Copy));
        assert_eq!(state.reason(), Some(PinReason::Copy));
    }

    #[test]
    fn every_reason_has_a_distinct_label() {
        // They are metric label values, so a collision would silently merge two
        // causes into one series.
        let all = [
            PinReason::Listen,
            PinReason::AdvisoryLock,
            PinReason::TempTable,
            PinReason::WithHold,
            PinReason::Prepare,
            PinReason::UnreplayableSet,
            PinReason::Copy,
            PinReason::Requested,
        ];
        let mut labels: Vec<&str> = all.iter().map(|r| r.as_str()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count, "two reasons share a metric label");
        assert!(labels.iter().all(|l| !l.is_empty()));
    }

    #[test]
    fn a_tenant_can_be_attached_for_attribution() {
        let mut state = PinState::new();
        assert_eq!(state.tenant(), None);
        state.set_tenant(TenantId::new("acme"));
        assert_eq!(state.tenant(), Some(&TenantId::new("acme")));
    }

    #[test]
    fn an_empty_allowlist_pins_every_set() {
        // The allowlist is configuration, and an operator who empties it gets
        // maximum safety at maximum cost rather than an error.
        assert_eq!(
            pin_reason("SET search_path = public", &[]),
            Some(PinReason::UnreplayableSet)
        );
    }
}
