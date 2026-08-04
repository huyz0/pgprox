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
use pgprox_core::sql::Token;

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
    /// The session asked for it, through `SET pgprox.pin = on`.
    ///
    /// An escape hatch for a tenant using something this list has not learned
    /// yet. Better than the alternative, which is them discovering the gap as
    /// another session's state appearing in theirs.
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
    pub fn observe_statement(&mut self, sql: &str, allowlist: Replayable) -> Option<PinReason> {
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
const REPLAYABLE_NAMES: &[&str] = &[
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
    // One of the five pgbouncer tracks by default, and absent here until
    // `M15.11`. It is an ordinary GUC reproduced by re-issuing the `SET`, so it
    // meets the criterion above; its absence pinned any session that set it.
    //
    // The comment above says additions should be rare and each is a promise,
    // and that is right. It is not an argument against this one: `bytea_output`
    // and `intervalstyle` are on the list and are exactly as rare, so the rule
    // being applied is "can it be replayed", not "is it common".
    "standard_conforming_strings",
];

/// The set of parameters a session may set and still be moved.
///
/// A type rather than the `&[&str]` this was, because two different things
/// consult it and they have to agree. `PinState::observe_statement` decides
/// whether a `SET` pins the session; `SessionParams::observe_statement`
/// decides whether the same `SET` is recorded for replay. Given different
/// lists they disagree silently, and the shape of that bug is a session
/// recorded as movable whose settings are never replayed: the client's
/// `search_path` quietly reverts between statements and nothing errors.
///
/// Nothing else in the workspace can construct one except through
/// [`Replayable::DEFAULT`] and [`Replayable::from_names`], and the second
/// exists for tests that need to ask what an operator's narrower list would
/// do. ADR 0001 named this type; it took until M8 to exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Replayable {
    names: &'static [&'static str],
}

impl Replayable {
    /// The shipped list.
    ///
    /// Kept deliberately small. Every addition is a promise that replaying the
    /// parameter is enough, and a wrong promise is a session silently losing a
    /// setting rather than an error anyone sees.
    pub const DEFAULT: Self = Self {
        names: REPLAYABLE_NAMES,
    };

    /// Nothing is replayable, so every `SET` pins.
    ///
    /// What an operator who empties the list gets: maximum safety at maximum
    /// cost, rather than an error.
    pub const NONE: Self = Self { names: &[] };

    /// A narrower or wider list, for a caller that has one.
    #[must_use]
    pub const fn from_names(names: &'static [&'static str]) -> Self {
        Self { names }
    }

    /// Whether this parameter is replayed rather than pinned.
    ///
    /// The name is compared as given. Both callers normalise before asking,
    /// and doing it again here would hide a caller that had not.
    #[must_use]
    pub fn contains(self, name: &str) -> bool {
        self.names.contains(&name)
    }

    /// Every name on the list, in the order it was written.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        self.names.iter().copied()
    }
}

impl Default for Replayable {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why this statement pins the session, if it does.
///
/// A lexical scan, like the classifier: it must be right about which text is
/// SQL and which is data, and it errs toward pinning. A false pin costs one
/// session's share of multiplexing. A missed pin hands a client another
/// client's temp table.
#[must_use]
pub fn pin_reason(sql: &str, allowlist: Replayable) -> Option<PinReason> {
    // Every statement, not just the first. The simple query protocol allows
    // several in one message, so checking only the leading word would let
    // `SELECT 1; LISTEN c` through unpinned, and the session would silently
    // stop receiving its notifications.
    pgprox_core::sql::statements(sql)
        .into_iter()
        .find_map(|statement| statement_pin_reason(statement, allowlist))
}

/// Why one statement pins.
///
/// Takes the text as well as deriving words from it, because one case cannot be
/// decided from words alone: see [`quoted_parameter_name`].
fn statement_pin_reason(statement: &str, allowlist: Replayable) -> Option<PinReason> {
    let words = words_of(statement);
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
    if first == "declare" && has_adjacent_pair(&words, "with", "hold") {
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
        return set_pin_reason(statement, &words, allowlist);
    }

    None
}

/// Whether a `SET` pins, given the replay allowlist.
fn set_pin_reason(statement: &str, words: &[String], allowlist: Replayable) -> Option<PinReason> {
    let mut rest = &words[1..];

    // `SET SESSION x` is the same as `SET x`.
    if rest.first().is_some_and(|w| w == "session") {
        rest = &rest[1..];
    } else if rest.first().is_some_and(|w| w == "local") {
        // Transaction-scoped, gone at commit, whatever it names and however
        // that name is spelled. Checked before the quoting below for exactly
        // that reason.
        return None;
    }

    // A parameter named in quotes, which the words cannot show: `Token::Quoted`
    // carries no text on purpose, so `SET "search_path" = 'tenant1'` reduces to
    // the single word `set` and there is no name here to compare against the
    // allowlist. `params.rs` does not record it either, because the quotes
    // survive its own reading and the name misses the list.
    //
    // A name this cannot read is a name it cannot promise to replay, so it
    // pins. `M24.2`. The alternative is to teach the lexer to hand out the
    // text inside quotes, which it declines to do for a good reason: a caller
    // that can search quoted text is a caller whose behaviour a tenant's own
    // data can change.
    if quoted_parameter_name(statement) {
        return Some(PinReason::UnreplayableSet);
    }

    let name = rest.first()?;

    // `SET TRANSACTION ...` and `SET CONSTRAINTS ...` are transaction-scoped.
    if name == "transaction" || name == "constraints" {
        return None;
    }

    // The proxy's own settings never reach the server and change no server-side
    // state, so they must not pin. Except the one that asks to.
    if name == PIN_PARAMETER {
        return Some(PinReason::Requested);
    }
    if name.starts_with("pgprox.") {
        return None;
    }

    if allowlist.contains(name) {
        return None;
    }

    Some(PinReason::UnreplayableSet)
}

/// The parameter a session pins itself with.
///
/// `SET pgprox.pin = on`. A `pgprox.` name because Postgres accepts assignment
/// to any dotted parameter it does not recognise, so the statement is valid SQL
/// whether or not it went through a proxy, and an application can issue it
/// unconditionally.
///
/// # The value is not read
///
/// Setting this parameter pins, whatever it is set to. That looks sloppy and is
/// deliberate.
///
/// Unpinning is not offered: no statement proves a session has stopped needing
/// its connection, and the whole reason a tenant reaches for this is that the
/// proxy cannot see what makes theirs unmovable. So `= off` cannot mean what it
/// appears to, and honouring it would be a promise this module cannot keep.
/// Reading the value would also mean reading quoted text, and the scanner drops
/// that on purpose so a row's contents can never pin a session.
///
/// Mentioning the parameter is therefore the request, and the only way to stop
/// being pinned is to open a new connection.
pub const PIN_PARAMETER: &str = "pgprox.pin";

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

/// Whether this `SET` names its parameter in quotes.
///
/// The one thing [`set_pin_reason`] cannot decide from words. Reads tokens
/// rather than words, because the distinction it needs is precisely the one
/// words throw away: whether something was there at all.
///
/// The caller has established the first word is `set`, so the first token is
/// that word, and `SET SESSION x` is `SET x` with one more.
fn quoted_parameter_name(statement: &str) -> bool {
    let mut lexer = pgprox_core::sql::Lexer::new(statement);
    lexer.next();
    let mut token = lexer.next();
    if matches!(token, Some(Token::Word(word)) if word.eq_ignore_ascii_case("session")) {
        token = lexer.next();
    }
    matches!(token, Some(Token::Quoted))
}

/// The lowercase bare words of one statement.
///
/// [`pgprox_core::sql`] owns the hard part: which text is SQL and which is
/// data. This crate used to carry its own copy, and the two diverged. See that
/// module's docs for what it cost.
///
/// Dots are kept, so `pgprox.pin` and `pg_temp.t` arrive as one word each and
/// can be compared against a qualified name.
///
/// Empty when the statement holds no bare words at all, which `SET "a" = 'b'`
/// does: both of its tokens are quoted.
fn words_of(statement: &str) -> Vec<String> {
    pgprox_core::sql::statement_words(statement, true)
        .into_iter()
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_replayable_list_is_enumerable_and_agrees_with_itself() {
        // `M14.21`. `names` could return an empty iterator and nothing noticed.
        // The one caller, in `params.rs`, walks it to reset every replayable
        // parameter a previous session left behind before handing the
        // connection on. An empty iterator there is a connection returned to
        // the pool still carrying the last session's settings, which is the
        // exact failure the replay mechanism exists to prevent, and it is
        // silent.
        let listed: Vec<&str> = Replayable::DEFAULT.names().collect();

        assert!(!listed.is_empty(), "the shipped list enumerated as empty");
        assert_eq!(
            listed.len(),
            REPLAYABLE_NAMES.len(),
            "names() and the list it is built from disagree about how many there are"
        );

        // Enumeration and membership have to agree, or one of them is lying.
        for name in &listed {
            assert!(
                Replayable::DEFAULT.contains(name),
                "{name} was enumerated but is not contained"
            );
        }

        // An empty list enumerates as empty, so the assertion above is about
        // the contents rather than about the method always returning something.
        assert_eq!(Replayable::NONE.names().count(), 0);

        // And a custom list round-trips in the order it was written.
        let custom = Replayable::from_names(&["b", "a", "c"]);
        assert_eq!(custom.names().collect::<Vec<_>>(), vec!["b", "a", "c"]);
    }

    fn reason(sql: &str) -> Option<PinReason> {
        pin_reason(sql, Replayable::DEFAULT)
    }

    #[test]
    fn a_set_whose_parameter_name_is_quoted_pins() {
        // `M24.2`. `SET "search_path" = 'tenant1'` is valid Postgres. Both
        // tokens are quoted, `statement_words` drops quoted text on purpose,
        // and the statement arrived here as the single word `set`, so
        // `set_pin_reason` returned before it looked at a name.
        //
        // `params.rs` did not record it either, because the quotes survive its
        // own reading and the name misses the allowlist. Neither replayed nor
        // pinned is the one outcome the two are supposed to make impossible.
        for sql in [
            r#"SET "search_path" = 'tenant1'"#,
            r#"SET "work_mem" = '1GB'"#,
            r#"SET SESSION "search_path" = 'tenant1'"#,
            r#"SELECT 1; SET "search_path" = 'tenant1'"#,
            // A quoted name with a bare value pinned already, and for the wrong
            // reason: the value was read as the name and missed the allowlist.
            // It has to keep pinning for the right one.
            r#"SET "search_path" = tenant1"#,
        ] {
            assert_eq!(reason(sql), Some(PinReason::UnreplayableSet), "{sql}");
        }
    }

    #[test]
    fn a_quoted_name_does_not_make_set_local_pin() {
        // `SET LOCAL` is gone at commit whatever it names, so quoting the
        // parameter must not turn a transaction-scoped statement into a pin.
        for sql in [
            r#"SET LOCAL "search_path" = 'tenant1'"#,
            r#"SET local "work_mem" = '1GB'"#,
        ] {
            assert_eq!(reason(sql), None, "{sql}");
        }
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
            // `M15.11`. Absent from the list until then, so a session that set
            // it was pinned for its lifetime. It is one of the five pgbouncer
            // tracks by default and is reproduced by re-issuing the `SET` like
            // any other here.
            "SET standard_conforming_strings = off",
        ] {
            assert_eq!(reason(sql), None, "{sql}");
        }
    }

    #[test]
    fn a_parameter_that_replays_is_also_recorded_for_replay() {
        // The two halves the `Replayable` type exists to keep in step, checked
        // on the parameter this milestone added. A name on the pin side and off
        // the record side is a session reported as movable whose setting is
        // never replayed, which is silent: the client's parameter quietly
        // reverts between statements and nothing errors.
        let mut params = crate::params::SessionParams::new();
        let change =
            params.observe_statement("SET standard_conforming_strings = off", Replayable::DEFAULT);

        assert!(
            matches!(change, Some(crate::params::ParamChange::Recorded { .. })),
            "it does not pin, and it is not recorded either: {change:?}"
        );
        assert_eq!(params.len(), 1);
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
    fn the_proxys_own_settings_never_pin_except_the_one_that_asks_to() {
        // They never reach the server and change no server-side state, so a
        // session that used one is still perfectly movable.
        assert_eq!(reason("SET pgprox.route = 'replica'"), None);
        assert_eq!(reason("SET pgprox.anything = 1"), None);
    }

    #[test]
    fn an_e_string_does_not_hide_a_later_statement() {
        // The divergence that put the lexer in pgprox-core. This crate's own
        // scanner ended `E'\''` at the escaped quote, read the rest as data,
        // and left the session unpinned. A missed pin hands one client another
        // client's state.
        assert_eq!(
            reason(r"SELECT E'\'' ; LISTEN c"),
            Some(PinReason::Listen),
            "an E-string swallowed the statement after it"
        );
        assert_eq!(
            reason(r"SELECT E'\' ; LISTEN c'"),
            None,
            "an E-string's contents were read as SQL"
        );
    }

    #[test]
    fn a_session_can_pin_itself() {
        // The escape hatch for a tenant using something this list has not
        // learned yet. Without it they discover the gap as another session's
        // state appearing in theirs.
        for sql in [
            "SET pgprox.pin = on",
            "SET pgprox.pin = 'on'",
            "SET pgprox.pin TO true",
            "set PGPROX.PIN to YES",
            "SET pgprox.pin = 1;",
            "SELECT 1; SET pgprox.pin = on",
        ] {
            assert_eq!(reason(sql), Some(PinReason::Requested), "{sql:?}");
        }
    }

    #[test]
    fn setting_the_pin_parameter_to_anything_pins() {
        // Including `off`, which looks wrong and is not. Unpinning is not
        // offered, so honouring it would be a promise this module cannot keep,
        // and reading the value would mean reading quoted text that the scanner
        // drops on purpose so a row's contents cannot pin a session.
        for sql in [
            "SET pgprox.pin = off",
            "SET pgprox.pin = false",
            "SET pgprox.pin = maybe",
        ] {
            assert_eq!(reason(sql), Some(PinReason::Requested), "{sql:?}");
        }
    }

    #[test]
    fn the_pin_parameter_only_pins_when_it_is_set() {
        // A statement that merely mentions it, or resets it, is not a request.
        for sql in [
            "SELECT * FROM t WHERE a = 'pgprox.pin'",
            "RESET pgprox.pin",
            "SET LOCAL pgprox.pin = on",
        ] {
            assert_eq!(reason(sql), None, "{sql:?}");
        }
    }

    #[test]
    fn a_requested_pin_is_as_permanent_as_any_other() {
        let mut state = PinState::new();
        assert_eq!(
            state.observe_statement("SET pgprox.pin = on", Replayable::DEFAULT),
            Some(PinReason::Requested)
        );
        state.observe_statement("SET pgprox.pin = off", Replayable::DEFAULT);
        assert!(state.is_pinned(), "a session unpinned itself");
        assert_eq!(state.reason(), Some(PinReason::Requested));
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
            state.observe_statement("LISTEN c", Replayable::DEFAULT),
            Some(PinReason::Listen)
        );
        assert_eq!(
            state.observe_statement("CREATE TEMP TABLE t (a int)", Replayable::DEFAULT),
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
        state.observe_statement("LISTEN c", Replayable::DEFAULT);
        state.observe_statement("UNLISTEN *", Replayable::DEFAULT);
        state.observe_statement("SELECT 1", Replayable::DEFAULT);
        assert!(state.is_pinned());
    }

    #[test]
    fn a_copy_stream_is_not_a_pin_reason() {
        // A COPY holds the connection while it runs and releases it when it
        // ends, which is what pgprox_proto::session::HoldReason::Copy models.
        // Nothing here may say the same thing, because a pin never clears and
        // a session that once ran a COPY would keep its connection for life.
        let mut state = PinState::new();
        state.observe_statement("COPY t FROM STDIN", Replayable::DEFAULT);
        assert!(
            !state.is_pinned(),
            "COPY pinned a session permanently; that belongs to HoldReason"
        );
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
            pin_reason("SET search_path = public", Replayable::NONE),
            Some(PinReason::UnreplayableSet)
        );
    }
}
