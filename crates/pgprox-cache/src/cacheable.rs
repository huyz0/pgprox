//! Whether a statement's answer may be stored and served again.
//!
//! # A different question from replica routing
//!
//! `pgprox-route` asks whether a statement *writes*, because that is what
//! decides whether a replica may run it. This asks whether the answer is a
//! function of the cache key, because that is what decides whether the answer
//! may be handed to somebody else later.
//!
//! The two disagree, and `random()` is the example the classifier itself
//! names: it is volatile, it is perfectly safe to route to a replica, and
//! caching it turns a random number into a constant for the length of the TTL.
//! So the class is necessary and not sufficient, and this module is the rest.
//!
//! # Refuses by default
//!
//! Every path returns an error unless the statement passes every check. A
//! cache that stored what it was handed would be wrong in a way the TTL does
//! not bound: bounded staleness means data of a knowable age, not the wrong
//! row.
//!
//! # The honest limit
//!
//! The function list is a denylist of built-in names, so a tenant's own
//! `VOLATILE` function called from a `SELECT` is not caught. That is the same
//! limit ADR 0009 records for the classifier and it has the same shape: a
//! lexical scan cannot know what a tenant's functions do.
//!
//! What makes it acceptable here is the same thing that makes the whole
//! feature acceptable: the cache is off unless a tenant turned it on, and
//! turning it on is a statement about their own workload. It is not acceptable
//! on the strength of the denylist being complete, because it is not.

use pgprox_core::route::StmtClass;
use pgprox_core::sql::statement_words;

/// Why a statement's answer may not be cached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotCacheable {
    /// The classifier did not call it read-only.
    ///
    /// Covers writes, locks, and anything it could not place, since
    /// `StmtClass::Unknown` exists so a construct nobody has taught it yet is
    /// treated as a write.
    NotReadOnly,
    /// The session has written in this transaction.
    ///
    /// Two reasons, either of which is enough. Its reads can see rows no other
    /// session can, so storing one would publish uncommitted data. And ADR
    /// 0021 says that where the cache and read routing disagree, routing wins:
    /// a session with a watermark is one that has written.
    SessionWrote,
    /// The session has a transaction open.
    ///
    /// Both directions are wrong, and the reasons are different. An answer
    /// produced inside a transaction can see rows no other session can, even
    /// when the transaction has not written, because a `SET TRANSACTION
    /// ISOLATION LEVEL` or a read of an uncommitted sibling's work is visible
    /// where the entry would not be. And a stored answer carries the
    /// transaction status the server sent with it: served to a session mid
    /// transaction, an entry recorded while idle says `ReadyForQuery('I')` and
    /// tells the client its transaction ended.
    ///
    /// The second half is why this is refused rather than merely not stored.
    /// See ADR 0022, which rests on it: a sequence is only ever withheld from
    /// a session with nothing open, so the `ReadyForQuery` the relay generates
    /// for a hit is `'I'` by construction.
    InTransaction,
    /// The session is pinned, so it holds state the cache cannot see.
    ///
    /// A temporary table is the case that matters. `SELECT * FROM scratch`
    /// names one table in this session and a different one in the next, and
    /// nothing in the cache key tells them apart.
    Pinned,
    /// More than one statement arrived in a single simple query.
    ///
    /// Refused rather than handled. The response is several result sets whose
    /// boundaries this crate does not track, and the classifier's verdict is
    /// per statement.
    MultipleStatements,
    /// The answer is not a function of the key.
    ///
    /// The named call returns something that depends on when, where or by whom
    /// it was made.
    NotAFunctionOfTheKey {
        /// Which call, so a log line says why rather than that.
        name: &'static str,
    },
}

/// What the session looks like at the moment the statement arrives.
///
/// Passed in rather than reached for, because this crate may depend on
/// `pgprox-core` and nothing else in the workspace: the pin state lives in
/// `pgprox-pool` and the classifier in `pgprox-route`. The caller composing
/// those is `pgprox-session`, the same way the pin allowlist arrives as an
/// argument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SessionFacts {
    /// Whether this session has written inside the current transaction.
    pub wrote_in_transaction: bool,
    /// Whether the session is pinned to one upstream connection.
    pub pinned: bool,
    /// Whether the session has a transaction open, written in or not.
    ///
    /// Read from the transaction status the server last sent rather than from
    /// the SQL, for the reason the release rule is: a `COMMIT` inside a failed
    /// transaction does not commit, and a text scan cannot tell.
    pub in_transaction: bool,
}

impl SessionFacts {
    /// A session that has done nothing to disqualify itself.
    #[must_use]
    pub const fn clean() -> Self {
        Self {
            wrote_in_transaction: false,
            pinned: false,
            in_transaction: false,
        }
    }

    /// What the caller observed about the session.
    ///
    /// A constructor rather than a struct literal because the type is
    /// `#[non_exhaustive]`: a field added here has to be a compile error at
    /// every call site, since a caller that quietly kept the old default would
    /// be asserting something about a session it never looked at.
    #[must_use]
    pub const fn new(wrote_in_transaction: bool, pinned: bool, in_transaction: bool) -> Self {
        Self {
            wrote_in_transaction,
            pinned,
            in_transaction,
        }
    }
}

/// Calls whose result is not a function of the cache key.
///
/// Deliberately *not* the classifier's `WRITING_FUNCTIONS`. That list is about
/// side effects and this one is about determinism, and the interesting entries
/// are on exactly one of them: `nextval` writes and is already refused by the
/// class, `random()` writes nothing and must never be cached.
///
/// Matched on the bare name, so `pg_catalog.now` hits: `statement_words` with
/// dots split yields `pg_catalog` and `now` separately.
///
/// A column named `now` refuses to cache its own query. That is a miss rather
/// than a wrong answer, which is the direction this list errs in throughout.
const NOT_A_FUNCTION_OF_THE_KEY: &[&str] = &[
    // Time. `now` and `transaction_timestamp` are fixed within a transaction
    // and still vary between them, which is exactly long enough to look
    // correct in testing.
    "now",
    "current_timestamp",
    "current_date",
    "current_time",
    "localtime",
    "localtimestamp",
    "clock_timestamp",
    "statement_timestamp",
    "transaction_timestamp",
    "timeofday",
    // Randomness.
    "random",
    "random_normal",
    "gen_random_uuid",
    "uuid_generate_v1",
    "uuid_generate_v1mc",
    "uuid_generate_v4",
    // Who and where. The key holds a tenant, not a role: a tenant that used
    // `SET ROLE` would otherwise see one role's answer served to another.
    "current_user",
    "session_user",
    "current_role",
    "current_catalog",
    "current_database",
    "current_schema",
    "current_schemas",
    "current_query",
    "pg_backend_pid",
    "inet_client_addr",
    "inet_client_port",
    "inet_server_addr",
    "inet_server_port",
    "pg_current_logfile",
    // Session-scoped settings. `current_setting('x')` reads whatever this
    // session last set, which no part of the key records.
    "current_setting",
    // Sequence position, which is per session and moves.
    "currval",
    "lastval",
    // Transaction identity.
    "pg_current_xact_id",
    "pg_current_xact_id_if_assigned",
    "txid_current_if_assigned",
    "pg_current_snapshot",
    "txid_current_snapshot",
    // Waiting. Caching one turns a delay into no delay, which is a change in
    // behaviour even where it is not a change in data.
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
];

/// Whether this statement's answer may be cached.
///
/// The class comes from `pgprox-route`, the facts from the session. Everything
/// else is read out of the SQL.
///
/// # Errors
///
/// Returns why not, so a caller can count the reasons apart. A cache whose
/// miss rate is entirely `Pinned` wants a different conversation from one
/// whose misses are all `NotAFunctionOfTheKey`.
///
/// ```
/// use pgprox_cache::cacheable::{cacheable, NotCacheable, SessionFacts};
/// use pgprox_core::route::StmtClass;
///
/// let clean = SessionFacts::clean();
/// assert!(cacheable("SELECT * FROM orders", StmtClass::ReadOnly, clean).is_ok());
///
/// // Read-only, replica-safe, and never the same answer twice.
/// assert_eq!(
///     cacheable("SELECT random()", StmtClass::ReadOnly, clean),
///     Err(NotCacheable::NotAFunctionOfTheKey { name: "random" })
/// );
/// ```
pub fn cacheable(sql: &str, class: StmtClass, session: SessionFacts) -> Result<(), NotCacheable> {
    if class != StmtClass::ReadOnly {
        return Err(NotCacheable::NotReadOnly);
    }
    if session.wrote_in_transaction {
        return Err(NotCacheable::SessionWrote);
    }
    if session.in_transaction {
        return Err(NotCacheable::InTransaction);
    }
    if session.pinned {
        return Err(NotCacheable::Pinned);
    }

    // Quoted text is already gone: `statement_words` drops it, so a row
    // containing the word `random` cannot make its own query uncacheable.
    let statements = statement_words(sql, false);
    if statements.len() > 1 {
        return Err(NotCacheable::MultipleStatements);
    }

    for words in &statements {
        for word in words {
            if let Some(name) = NOT_A_FUNCTION_OF_THE_KEY
                .iter()
                .find(|candidate| *candidate == word)
            {
                return Err(NotCacheable::NotAFunctionOfTheKey { name });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn verdict(sql: &str) -> Result<(), NotCacheable> {
        cacheable(sql, StmtClass::ReadOnly, SessionFacts::clean())
    }

    #[test]
    fn an_ordinary_read_is_cacheable() {
        assert!(verdict("SELECT id, name FROM orders WHERE id = $1").is_ok());
        assert!(verdict("select count(*) from t").is_ok());
        assert!(verdict("SELECT * FROM a JOIN b ON a.id = b.id").is_ok());
    }

    #[test]
    fn anything_the_classifier_did_not_call_read_only_is_refused() {
        // Including Unknown, which exists so a construct nobody has taught the
        // classifier yet is treated as a write rather than guessed at.
        for class in [StmtClass::Write, StmtClass::Unknown] {
            assert_eq!(
                cacheable("SELECT 1", class, SessionFacts::clean()),
                Err(NotCacheable::NotReadOnly),
                "{class:?}"
            );
        }
    }

    #[test]
    fn a_session_that_has_written_is_refused() {
        // Two reasons and either is enough: its reads see rows no other
        // session can, so caching one publishes uncommitted data, and ADR 0021
        // gives read routing the last word where the two disagree.
        let facts = SessionFacts {
            wrote_in_transaction: true,
            ..SessionFacts::clean()
        };
        assert_eq!(
            cacheable("SELECT * FROM t", StmtClass::ReadOnly, facts),
            Err(NotCacheable::SessionWrote)
        );
    }

    #[test]
    fn a_session_with_a_transaction_open_is_refused() {
        // Refused even though it has not written, and the reason the writing
        // case does not cover it is the transaction status. A stored answer
        // carries the one the server sent with it, so an entry recorded while
        // idle ends in `ReadyForQuery('I')` and tells this session its
        // transaction ended. ADR 0022 rests on this being refused.
        let facts = SessionFacts {
            in_transaction: true,
            ..SessionFacts::clean()
        };
        assert_eq!(
            cacheable("SELECT * FROM t", StmtClass::ReadOnly, facts),
            Err(NotCacheable::InTransaction)
        );
    }

    #[test]
    fn a_pinned_session_is_refused() {
        // The temporary table case. `SELECT * FROM scratch` names one table in
        // this session and another in the next, and nothing in the key tells
        // them apart.
        let facts = SessionFacts {
            pinned: true,
            ..SessionFacts::clean()
        };
        assert_eq!(
            cacheable("SELECT * FROM scratch", StmtClass::ReadOnly, facts),
            Err(NotCacheable::Pinned)
        );
    }

    #[test]
    fn a_volatile_call_the_classifier_allows_is_still_refused() {
        // The whole reason this module exists. Every one of these is
        // replica-safe and the classifier is right to route it.
        for sql in [
            "SELECT random()",
            "SELECT now()",
            "SELECT clock_timestamp()",
            "SELECT gen_random_uuid()",
            "SELECT current_user",
            "SELECT current_setting('x')",
            "SELECT pg_sleep(1)",
            "SELECT currval('s')",
            "SELECT * FROM t WHERE created > now() - interval '1 day'",
        ] {
            assert!(
                matches!(verdict(sql), Err(NotCacheable::NotAFunctionOfTheKey { .. })),
                "{sql:?} was called cacheable"
            );
        }
    }

    #[test]
    fn the_reason_names_the_call() {
        // So a log line says which one, rather than that there was one.
        assert_eq!(
            verdict("SELECT now()"),
            Err(NotCacheable::NotAFunctionOfTheKey { name: "now" })
        );
    }

    #[test]
    fn a_schema_qualified_call_is_caught() {
        // `statement_words` with dots split yields the qualifier and the name
        // separately, which is how the classifier matches its own list.
        assert!(matches!(
            verdict("SELECT pg_catalog.now()"),
            Err(NotCacheable::NotAFunctionOfTheKey { name: "now" })
        ));
    }

    #[test]
    fn the_case_of_the_call_does_not_matter() {
        assert!(matches!(
            verdict("SELECT RANDOM()"),
            Err(NotCacheable::NotAFunctionOfTheKey { .. })
        ));
    }

    #[test]
    fn a_row_containing_the_word_random_does_not_refuse_its_own_query() {
        // Quoted text is not SQL. If it were, a tenant's data would decide how
        // their queries are treated, which is the hazard `pgprox_core::sql`
        // exists to keep out of every caller.
        assert!(verdict("SELECT * FROM t WHERE note = 'random()'").is_ok());
        assert!(verdict("SELECT 'now()' FROM t").is_ok());
    }

    #[test]
    fn several_statements_in_one_query_are_refused() {
        // The response is several result sets whose boundaries this crate does
        // not track, and the class is a verdict on one statement.
        assert_eq!(
            verdict("SELECT 1; SELECT 2"),
            Err(NotCacheable::MultipleStatements)
        );
    }

    #[test]
    fn a_trailing_semicolon_is_not_a_second_statement() {
        assert!(verdict("SELECT 1;").is_ok());
    }

    #[test]
    fn an_empty_statement_is_cacheable_and_harmless() {
        // Nothing to refuse. The caller has no result to store either, so this
        // arm exists to be defined rather than to be used.
        assert!(verdict("").is_ok());
    }

    #[test]
    fn the_checks_are_ordered_cheapest_first() {
        // A session that wrote is refused on the fact rather than on a scan of
        // its SQL, which matters because this runs on every statement.
        let wrote = SessionFacts {
            wrote_in_transaction: true,
            ..SessionFacts::clean()
        };
        assert_eq!(
            cacheable("SELECT random()", StmtClass::ReadOnly, wrote),
            Err(NotCacheable::SessionWrote),
            "the scan ran before the fact that made it unnecessary"
        );
    }

    #[test]
    fn the_deny_list_does_not_repeat_the_classifiers() {
        // `nextval` writes, so the class already refuses it and this list has
        // no entry. Two lists with overlapping entries drift, and the one that
        // drifts is the one nobody remembers to update.
        assert!(
            !NOT_A_FUNCTION_OF_THE_KEY.contains(&"nextval"),
            "a writing function leaked into the determinism list"
        );
        assert_eq!(
            cacheable(
                "SELECT nextval('s')",
                StmtClass::Write,
                SessionFacts::clean()
            ),
            Err(NotCacheable::NotReadOnly)
        );
    }

    #[test]
    fn a_clean_session_is_the_default() {
        assert_eq!(SessionFacts::default(), SessionFacts::clean());
    }
}
