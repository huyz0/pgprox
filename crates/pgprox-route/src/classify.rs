//! Deciding what a statement does, from its text.
//!
//! # The rule
//!
//! A false negative sends a read to the primary and costs a little throughput.
//! A false positive sends a write's read to a replica and returns stale data,
//! which is a correctness bug from the tenant's side. So every ambiguity
//! resolves to [`StmtClass::Unknown`], which is not replica-eligible.
//!
//! # How it decides
//!
//! Three conditions, all required for [`StmtClass::ReadOnly`]:
//!
//! 1. The **first** word is one of a short allowlist: `SELECT`, `WITH`,
//!    `TABLE`, `VALUES`, `EXPLAIN`.
//! 2. **No** word anywhere in the statement is on a denylist of things that
//!    write, lock, or have side effects.
//! 3. **No** word anywhere is the name of a function known to write, which is
//!    the section below.
//!
//! The second condition is what handles the cases a first-token scan gets
//! wrong, and it handles them without special-casing any of them:
//! `WITH x AS (INSERT ...)` contains `insert`, `SELECT ... FOR UPDATE` contains
//! `update`, `FOR SHARE` contains `share`, `EXPLAIN ANALYZE` contains
//! `analyze`, and `SELECT ... INTO t` contains `into`.
//!
//! The asymmetry is deliberate. Adding a word to the denylist can only move
//! statements from `ReadOnly` toward the primary, so an over-broad denylist
//! costs throughput while a missing word costs correctness.
//!
//! That does not make an over-broad list free, and the denylist is shorter than
//! it first looks because of it. A keyword that can only *start* a statement
//! needs no entry: it is not on the allowlist, so it already classifies as
//! `Unknown`, and that holds after a semicolon too. Meanwhile `comment`,
//! `copy`, `call` and `share` are all legal unquoted column names, so an entry
//! for each keeps real queries off replicas. Only words reachable *inside* a
//! statement that leads with an allowlisted word are on the list, and each one
//! names the construct that requires it.
//!
//! # Volatile functions
//!
//! A third condition downgrades to [`StmtClass::Unknown`] rather than to
//! `Write`: a call to a function known to have side effects. `nextval` advances
//! a sequence, `pg_advisory_lock` takes a lock, `txid_current` assigns a
//! transaction ID. All of them write, and all of them fail outright on a
//! replica in recovery.
//!
//! `Unknown` rather than `Write` because the distinction is real. A `Write` is
//! a statement this scan understands to modify data; an `Unknown` is one it
//! does not vouch for. They route identically today, and if that ever changes,
//! a volatile call should follow the cautious branch rather than the confident
//! one.
//!
//! The list covers functions that write. It does not try to cover every
//! function marked `VOLATILE` in the catalogue, because `random()` and
//! `clock_timestamp()` are volatile and harmless to route, and because a proxy
//! cannot know what a tenant's own functions do. That last one is the honest
//! limit of a lexical scan, recorded in ADR 0009: a tenant calling a
//! write-performing function of their own from a `SELECT` gets it routed as a
//! read. `SET pgprox.route = 'primary'` is the escape hatch.
//!
//! # Not a parser
//!
//! This is a scan, per ADR 0009. It does not build a tree, does not resolve
//! names, and does not know which functions exist. It has to be right about
//! *lexical structure* though, because that is what decides whether a word is
//! SQL or the contents of a string literal, and mistaking one for the other in
//! the wrong direction is how a write gets called a read.
//!
//! That is not hypothetical. `SELECT $1 INSERT $$` was classified read-only
//! because `$1 INSERT $` was accepted as a dollar-quote tag and swallowed the
//! rest of the statement. See the dollar-quote tag validation below.

use pgprox_core::route::StmtClass;
use pgprox_core::sql::{Lexer, Token};

/// Statements that may be reads, by their first word.
///
/// Deliberately short. Anything not here is [`StmtClass::Unknown`] and goes to
/// the primary, which is the correct answer for transaction control, `SET`,
/// `SHOW`, DDL, and every construct this list has not learned yet.
const READ_FIRST_WORDS: &[&str] = &["select", "with", "table", "values", "explain"];

/// Words that disqualify a statement from being a read, wherever they appear.
///
/// Deliberately shorter than the list of things that write, because a word only
/// needs to be here if it can appear *inside* a statement whose first word is
/// on the allowlist. A keyword that can only start a statement is already
/// handled: it is not on the allowlist, so it classifies as
/// [`StmtClass::Unknown`] and goes to the primary. That holds after a semicolon
/// too, since each statement gets its own first word and the results combine to
/// the most restrictive.
///
/// So `DROP`, `GRANT`, `LISTEN`, `COPY`, `COMMENT` and the rest are absent on
/// purpose. Keeping them would cost real reads: `comment`, `share`, `call` and
/// `copy` are all legal unquoted column names, and `SELECT comment FROM posts`
/// is a query somebody actually writes.
///
/// Each entry below names the construct that requires it. Removing one means
/// showing that construct cannot occur.
const WRITE_WORDS: &[&str] = &[
    // Data-modifying CTEs: `WITH x AS (INSERT ... RETURNING *) SELECT ...`.
    // These are also what `EXPLAIN INSERT`, `EXPLAIN UPDATE` and so on reach.
    "insert", "update", "delete", "merge",
    // Not reachable inside a read: `TRUNCATE` cannot appear in a CTE and
    // `EXPLAIN TRUNCATE` is not valid, so the allowlist already routes it
    // correctly. Kept because it costs nothing, nobody names a column
    // `truncate`, and it makes the reported class accurate rather than merely
    // safe. That distinction matters for `pgprox_query_duration_seconds{route}`
    // and for anyone reading a log line.
    "truncate",
    // `EXPLAIN CREATE TABLE x AS SELECT ...` and `EXPLAIN CREATE MATERIALIZED
    // VIEW ...` are both valid and both write. This is the entry most easily
    // mistaken for redundant.
    "create", // `EXPLAIN REFRESH MATERIALIZED VIEW ...`.
    "refresh",
    // `EXPLAIN EXECUTE stmt`, which runs a prepared statement that may be
    // anything at all.
    "execute", // `EXPLAIN DECLARE ... CURSOR`.
    "declare",
    // The locking clause: `FOR UPDATE` is caught by `update` above, and these
    // cover `FOR SHARE`, `FOR KEY SHARE` and `FOR NO KEY UPDATE`.
    "share", // `SELECT ... INTO t` creates a table.
    "into",
    // `EXPLAIN ANALYZE` executes the plan for real, side effects and all. Plain
    // `EXPLAIN` does not, which is why `explain` is an allowed first word.
    "analyze", "analyse",
];

/// Functions that write, and so cannot run on a replica.
///
/// Matched on the bare name, so `nextval` and `pg_catalog.nextval` both hit:
/// the scan yields `pg_catalog`, `nextval` and the parenthesis separately.
///
/// Not a list of every `VOLATILE` function. `random()` is volatile and
/// perfectly safe to route; these are the ones with side effects.
const WRITING_FUNCTIONS: &[&str] = &[
    // Sequences.
    "nextval",
    "setval",
    // Session and transaction-scoped locks. The `_xact_` variants write too,
    // even though they do not pin the session.
    "pg_advisory_lock",
    "pg_advisory_lock_shared",
    "pg_advisory_unlock",
    "pg_advisory_unlock_all",
    "pg_advisory_unlock_shared",
    "pg_advisory_xact_lock",
    "pg_advisory_xact_lock_shared",
    "pg_try_advisory_lock",
    "pg_try_advisory_lock_shared",
    "pg_try_advisory_xact_lock",
    "pg_try_advisory_xact_lock_shared",
    // Assigns a real transaction ID, which a replica cannot do.
    "txid_current",
    "pg_current_xact_id",
    // Writes WAL.
    "pg_logical_emit_message",
    "pg_create_restore_point",
    // Replication and backup control.
    "pg_switch_wal",
    "pg_create_physical_replication_slot",
    "pg_create_logical_replication_slot",
    "pg_drop_replication_slot",
    "pg_replication_slot_advance",
    // Large objects live in a table.
    "lo_create",
    "lo_creat",
    "lo_import",
    "lo_unlink",
    "lo_from_bytea",
    "lo_put",
];

/// Classifies a statement.
///
/// Handles multi-statement strings, which the simple query protocol permits:
/// the result is the most restrictive of the parts, so
/// `SELECT 1; DELETE FROM t` is a write rather than a read.
///
/// ```
/// use pgprox_core::route::StmtClass;
/// use pgprox_route::classify;
///
/// assert_eq!(classify("SELECT * FROM orders"), StmtClass::ReadOnly);
/// assert_eq!(classify("SELECT 1; DELETE FROM t"), StmtClass::Write);
/// assert_eq!(classify("WITH x AS (INSERT INTO t VALUES (1)) SELECT * FROM x"), StmtClass::Write);
/// ```
#[must_use]
pub fn classify(sql: &str) -> StmtClass {
    let mut scanner = Lexer::new(sql);
    let mut worst = None;

    loop {
        let (class, more) = classify_one(&mut scanner);
        worst = Some(match worst {
            None => class,
            Some(previous) => combine(previous, class),
        });
        if !more {
            break;
        }
    }

    worst.unwrap_or(StmtClass::Unknown)
}

/// The more restrictive of two classes.
///
/// `ReadOnly` survives only if both are, since a string is replica-eligible
/// only when every statement in it is.
const fn combine(a: StmtClass, b: StmtClass) -> StmtClass {
    match (a, b) {
        (StmtClass::Write, _) | (_, StmtClass::Write) => StmtClass::Write,
        (StmtClass::ReadOnly, StmtClass::ReadOnly) => StmtClass::ReadOnly,
        // Everything else is unknown, and `StmtClass` is `#[non_exhaustive]`,
        // so a variant added later lands here too. A new class is therefore
        // safe by default rather than silently replica-eligible.
        _ => StmtClass::Unknown,
    }
}

/// Classifies up to the next statement separator.
///
/// Returns the class and whether another statement follows.
fn classify_one(scanner: &mut Lexer<'_>) -> (StmtClass, bool) {
    let mut first = true;
    let mut class = StmtClass::Unknown;

    while let Some(token) = scanner.next() {
        match token {
            Token::Semicolon => {
                // An empty statement, as in a trailing semicolon, contributes
                // nothing rather than counting as an unclassifiable one.
                let class = if first { StmtClass::ReadOnly } else { class };
                return (class, scanner.has_more());
            }
            Token::Quoted => {
                // A quoted identifier or a string. It is never a keyword, but
                // it does mean this statement has started.
                first = false;
            }
            // Punctuation says nothing about what a statement does, and must
            // not count as its start: `(SELECT 1)` leads with a parenthesis.
            Token::Punct(_) => {}
            Token::Word(word) => {
                if first {
                    first = false;
                    class = if matches_any(word, READ_FIRST_WORDS) {
                        StmtClass::ReadOnly
                    } else {
                        StmtClass::Unknown
                    };
                }
                if matches_any(word, WRITE_WORDS) {
                    // Not an early return: the rest of the statement still has
                    // to be consumed so the scanner is positioned for the next
                    // one, and a `;` inside a string must not be mistaken for a
                    // separator.
                    class = StmtClass::Write;
                } else if class == StmtClass::ReadOnly && matches_any(word, WRITING_FUNCTIONS) {
                    // Downgrades a read, never upgrades an already-known write.
                    class = StmtClass::Unknown;
                }
            }
        }
    }

    // Nothing at all is unclassifiable rather than readable, so an empty
    // string does not become a replica-eligible read.
    let class = if first { StmtClass::Unknown } else { class };
    (class, false)
}

/// ASCII case-insensitive membership, without allocating.
fn matches_any(word: &str, set: &[&str]) -> bool {
    set.iter()
        .any(|candidate| word.eq_ignore_ascii_case(candidate))
}

/// Whether a statement opens a transaction the server will refuse writes in.
///
/// `BEGIN READ ONLY`, `START TRANSACTION READ ONLY`, and the `SET TRANSACTION`
/// form. A session that has said this has told the server to reject writes for
/// the whole transaction, which is a stronger promise than the classifier can
/// make about any individual statement, so the transaction as a whole becomes
/// replica-eligible.
///
/// `READ WRITE` returns `false`, as does a bare `BEGIN`: the default is read
/// write, and reading the absence of a mode as a promise would be exactly
/// backwards.
///
/// ```
/// use pgprox_route::classify::begins_read_only_transaction;
///
/// assert!(begins_read_only_transaction("BEGIN READ ONLY"));
/// assert!(begins_read_only_transaction("START TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY"));
/// assert!(!begins_read_only_transaction("BEGIN"));
/// assert!(!begins_read_only_transaction("BEGIN READ WRITE"));
/// ```
#[must_use]
pub fn begins_read_only_transaction(sql: &str) -> bool {
    // One pass, no allocation: this runs on the route decision's hot path, on
    // every statement outside a transaction.
    let mut first: Option<&str> = None;
    let mut second: Option<&str> = None;
    let mut previous: Option<&str> = None;
    let mut read_only = false;

    for token in Lexer::new(sql) {
        match token {
            // Only the first statement can open the transaction.
            Token::Semicolon => break,
            // A transaction-opening statement has no quoted text in it, so
            // anything quoted means this is not one.
            Token::Quoted => return false,
            Token::Punct(_) => {}
            Token::Word(word) => {
                if first.is_none() {
                    first = Some(word);
                } else if second.is_none() {
                    second = Some(word);
                }
                // `READ ONLY` as adjacent words. `READ WRITE` says the
                // opposite, which is why this looks at the pair rather than
                // for `read` alone.
                if previous.is_some_and(|p| p.eq_ignore_ascii_case("read"))
                    && word.eq_ignore_ascii_case("only")
                {
                    read_only = true;
                }
                previous = Some(word);
            }
        }
    }

    let opens = first.is_some_and(|w| {
        w.eq_ignore_ascii_case("begin")
            || w.eq_ignore_ascii_case("start")
            || (w.eq_ignore_ascii_case("set")
                && second.is_some_and(|s| s.eq_ignore_ascii_case("transaction")))
    });

    opens && read_only
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_select_is_read_only() {
        assert_eq!(classify("SELECT * FROM orders"), StmtClass::ReadOnly);
        assert_eq!(classify("select 1"), StmtClass::ReadOnly);
        assert_eq!(classify("  SELECT\n\t1  "), StmtClass::ReadOnly);
    }

    #[test]
    fn plain_dml_is_a_write() {
        for sql in [
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "MERGE INTO t USING s ON true",
            "TRUNCATE t",
        ] {
            assert_eq!(classify(sql), StmtClass::Write, "{sql}");
        }
    }

    #[test]
    fn a_with_cte_containing_dml_is_a_write() {
        // The case a first-token scan gets wrong, and the reason the denylist
        // applies to every word rather than only the first.
        for sql in [
            "WITH x AS (INSERT INTO t VALUES (1) RETURNING *) SELECT * FROM x",
            "WITH x AS (UPDATE t SET a = 1 RETURNING *) SELECT * FROM x",
            "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x",
            "WITH RECURSIVE x AS (MERGE INTO t USING s ON true) SELECT * FROM x",
        ] {
            assert_eq!(classify(sql), StmtClass::Write, "{sql}");
        }
    }

    #[test]
    fn a_read_only_with_cte_is_still_a_read() {
        // The denylist must not be so broad that ordinary CTEs stop reaching
        // replicas, or the feature is worthless.
        let sql = "WITH x AS (SELECT id FROM orders) SELECT count(*) FROM x";
        assert_eq!(classify(sql), StmtClass::ReadOnly);
    }

    #[test]
    fn a_select_for_update_is_a_write() {
        // It takes row locks, so it must run where the locks mean something.
        for sql in [
            "SELECT * FROM t FOR UPDATE",
            "SELECT * FROM t FOR NO KEY UPDATE",
            "select * from t for update nowait",
        ] {
            assert_eq!(classify(sql), StmtClass::Write, "{sql}");
        }
    }

    #[test]
    fn a_select_for_share_is_a_write() {
        for sql in [
            "SELECT * FROM t FOR SHARE",
            "SELECT * FROM t FOR KEY SHARE",
            "select * from t for share skip locked",
        ] {
            assert_eq!(classify(sql), StmtClass::Write, "{sql}");
        }
    }

    #[test]
    fn explain_analyze_is_a_write_but_plain_explain_is_not() {
        // EXPLAIN builds a plan. EXPLAIN ANALYZE runs it, side effects and all.
        assert_eq!(classify("EXPLAIN SELECT * FROM t"), StmtClass::ReadOnly);
        assert_eq!(
            classify("EXPLAIN ANALYZE SELECT * FROM t"),
            StmtClass::Write
        );
        assert_eq!(
            classify("EXPLAIN (ANALYZE, BUFFERS) DELETE FROM t"),
            StmtClass::Write
        );
        assert_eq!(classify("EXPLAIN ANALYSE SELECT 1"), StmtClass::Write);
    }

    #[test]
    fn select_into_is_a_write() {
        assert_eq!(
            classify("SELECT * INTO new_table FROM orders"),
            StmtClass::Write
        );
    }

    #[test]
    fn an_unrecognised_first_word_is_unknown_rather_than_readable() {
        // The default that makes a construct the classifier has not learned yet
        // safe: it goes to the primary instead of being guessed at.
        for sql in [
            "SHOW work_mem",
            "FETCH ALL FROM c",
            "SOMETHINGNEW foo",
            "\u{1f600}",
        ] {
            assert_eq!(classify(sql), StmtClass::Unknown, "{sql}");
        }
    }

    #[test]
    fn an_empty_statement_is_unknown() {
        for sql in ["", "   ", "-- just a comment", "/* nothing */"] {
            assert_eq!(classify(sql), StmtClass::Unknown, "{sql:?}");
        }
    }

    #[test]
    fn a_second_statement_can_make_the_whole_string_a_write() {
        // The simple query protocol allows several statements in one message.
        // Classifying only the first would send a DELETE to a replica.
        assert_eq!(classify("SELECT 1; DELETE FROM t"), StmtClass::Write);
        assert_eq!(classify("SELECT 1; SELECT 2"), StmtClass::ReadOnly);
        assert_eq!(classify("SELECT 1;"), StmtClass::ReadOnly);
        assert_eq!(classify("SELECT 1; ; SELECT 2"), StmtClass::ReadOnly);
        assert_eq!(classify("DELETE FROM t; SELECT 1"), StmtClass::Write);
        assert_eq!(classify("SELECT 1; SHOW work_mem"), StmtClass::Unknown);
    }

    #[test]
    fn a_keyword_inside_a_string_literal_is_not_a_keyword() {
        // Otherwise every read whose data mentions a verb goes to the primary,
        // which is a throughput bug rather than a correctness one, but a real
        // one at this scale.
        assert_eq!(
            classify("SELECT * FROM t WHERE action = 'delete'"),
            StmtClass::ReadOnly
        );
        assert_eq!(
            classify("SELECT * FROM t WHERE note = 'we should insert this'"),
            StmtClass::ReadOnly
        );
    }

    #[test]
    fn a_keyword_inside_an_identifier_is_not_a_keyword() {
        assert_eq!(classify("SELECT insert_count FROM t"), StmtClass::ReadOnly);
        assert_eq!(classify("SELECT * FROM update_log"), StmtClass::ReadOnly);
        assert_eq!(classify(r#"SELECT "delete" FROM t"#), StmtClass::ReadOnly);
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_split_the_statement() {
        // The dangerous direction. If the scanner ended the string early it
        // would read the rest as fresh SQL, and if it ran past the end it would
        // swallow a real statement.
        assert_eq!(
            classify("SELECT * FROM t WHERE a = 'x; DELETE FROM t'"),
            StmtClass::ReadOnly
        );
        assert_eq!(
            classify("SELECT 'x; y'; DELETE FROM t"),
            StmtClass::Write,
            "a real second statement was swallowed by a string"
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string_early() {
        // `''` is the standard escape and always applies.
        assert_eq!(
            classify("SELECT * FROM t WHERE a = 'it''s fine'"),
            StmtClass::ReadOnly
        );
        assert_eq!(
            classify("SELECT 'it''s fine'; DELETE FROM t"),
            StmtClass::Write
        );
    }

    #[test]
    fn a_backslash_escape_is_honoured_only_in_an_e_string() {
        // The case the module docs name. In `E'\''` the quote is escaped, so
        // the string continues; treating it as terminated would expose the rest
        // as SQL, and the DELETE below must still be found either way.
        assert_eq!(
            classify(r"SELECT E'\'' ; DELETE FROM t"),
            StmtClass::Write,
            "the statement after an E-string was lost"
        );
        assert_eq!(
            classify(r"SELECT E'\' ; DELETE FROM t'"),
            StmtClass::ReadOnly,
            "an E-string's contents were read as SQL"
        );
    }

    #[test]
    fn a_dollar_quoted_string_is_skipped_whole() {
        assert_eq!(
            classify("SELECT $$ DELETE FROM t $$"),
            StmtClass::ReadOnly,
            "a dollar-quoted body was read as SQL"
        );
        assert_eq!(
            classify("SELECT $body$ DELETE FROM t $body$"),
            StmtClass::ReadOnly
        );
        assert_eq!(
            classify("SELECT $$ x $$; DELETE FROM t"),
            StmtClass::Write,
            "a statement after a dollar-quoted string was lost"
        );
    }

    #[test]
    fn a_tagged_dollar_string_is_not_ended_by_a_different_tag() {
        // `$$` inside `$body$ ... $body$` is data, not a delimiter.
        assert_eq!(classify("SELECT $body$ a $$ b $body$"), StmtClass::ReadOnly);
        assert_eq!(
            classify("SELECT $body$ a $$ b $body$; DELETE FROM t"),
            StmtClass::Write
        );
    }

    #[test]
    fn an_invalid_dollar_tag_does_not_swallow_the_statement() {
        // The regression. `$1 INSERT $` was accepted as a dollar-quote tag, so
        // the rest of the statement became a string body and the INSERT
        // vanished: `SELECT $1 INSERT $$` classified read-only. A tag follows
        // the rules for an unquoted identifier, and this one starts with a
        // digit and contains spaces. Found by the differential property test.
        assert_eq!(classify("SELECT $1 INSERT $$"), StmtClass::Write);
        assert_eq!(classify("SELECT $2 DELETE $$"), StmtClass::Write);
        assert_eq!(
            classify("SELECT $ INSERT $"),
            StmtClass::Write,
            "a tag with only a space swallowed the statement"
        );
    }

    #[test]
    fn a_valid_dollar_tag_is_still_dollar_quoting() {
        // The fix must not go the other way and stop recognising real tags,
        // which would expose their contents as SQL.
        for sql in [
            "SELECT $$ DELETE FROM t $$",
            "SELECT $body$ DELETE FROM t $body$",
            "SELECT $_x9$ DELETE FROM t $_x9$",
            "SELECT $tag2$ DELETE FROM t $tag2$",
        ] {
            assert_eq!(classify(sql), StmtClass::ReadOnly, "{sql}");
        }
    }

    #[test]
    fn a_parameter_placeholder_is_not_dollar_quoting() {
        assert_eq!(
            classify("SELECT * FROM t WHERE id = $1"),
            StmtClass::ReadOnly
        );
        assert_eq!(
            classify("SELECT * FROM t WHERE id = $1; DELETE FROM t"),
            StmtClass::Write,
            "a placeholder swallowed the rest of the string"
        );
    }

    #[test]
    fn comments_are_skipped_including_nested_block_comments() {
        // Postgres nests block comments, unlike C.
        assert_eq!(
            classify("-- DELETE FROM t\nSELECT 1"),
            StmtClass::ReadOnly,
            "a line comment was read as SQL"
        );
        assert_eq!(
            classify("/* DELETE FROM t */ SELECT 1"),
            StmtClass::ReadOnly
        );
        assert_eq!(
            classify("/* outer /* inner */ DELETE FROM t */ SELECT 1"),
            StmtClass::ReadOnly,
            "a nested comment ended early and exposed its contents"
        );
        assert_eq!(classify("SELECT 1 -- DELETE FROM t"), StmtClass::ReadOnly);
    }

    #[test]
    fn an_unterminated_construct_terminates_rather_than_hanging() {
        // These arrive from the internet. The server rejects them as syntax
        // errors, so where they route does not matter; that the classifier
        // reaches an answer at all does.
        for sql in [
            "SELECT 'unterminated",
            "SELECT \"unterminated",
            "SELECT $$unterminated",
            "SELECT $tag$unterminated",
            "/* unterminated",
            "SELECT E'\\",
            "$",
            "$$",
            ";",
            "';',",
        ] {
            let _ = classify(sql);
        }
    }

    #[test]
    fn an_unterminated_construct_cannot_hide_a_write() {
        // The direction that would matter. Whatever the scanner does at the end
        // of a malformed string, a write keyword it has already passed must
        // still count.
        for sql in [
            "DELETE FROM t WHERE a = 'unterminated",
            "SELECT 1; DELETE FROM t WHERE a = $$unterminated",
            "WITH x AS (INSERT INTO t VALUES (1)) SELECT 'unterminated",
        ] {
            assert_eq!(classify(sql), StmtClass::Write, "{sql:?}");
        }
    }

    #[test]
    fn explain_reaches_writing_statements_and_each_is_caught() {
        // EXPLAIN accepts more than SELECT, and every one of these writes.
        // This is the test that justifies `create`, `refresh`, `execute` and
        // `declare` being on the denylist at all: without EXPLAIN they could
        // only start a statement, where the allowlist would handle them.
        for sql in [
            "EXPLAIN CREATE TABLE x AS SELECT 1",
            "EXPLAIN CREATE MATERIALIZED VIEW v AS SELECT 1",
            "EXPLAIN REFRESH MATERIALIZED VIEW v",
            "EXPLAIN EXECUTE stmt",
            "EXPLAIN DECLARE c CURSOR FOR SELECT 1",
            "EXPLAIN INSERT INTO t VALUES (1)",
            "EXPLAIN UPDATE t SET a = 1",
            "EXPLAIN DELETE FROM t",
        ] {
            assert_ne!(
                classify(sql),
                StmtClass::ReadOnly,
                "{sql} was sent to a replica"
            );
        }
    }

    #[test]
    fn a_statement_leading_keyword_is_caught_by_the_allowlist_not_the_denylist() {
        // Why the denylist can stay short. These write, none of them is on the
        // denylist, and all of them still stay off replicas because their first
        // word is not on the allowlist. It holds after a semicolon too, since
        // each statement gets its own first word.
        for sql in [
            "DROP TABLE t",
            "GRANT SELECT ON t TO r",
            "COMMENT ON TABLE t IS 'x'",
            "LOCK TABLE t",
            "COPY t FROM STDIN",
            "CALL p()",
            "VACUUM t",
        ] {
            assert_ne!(classify(sql), StmtClass::ReadOnly, "{sql}");
            let chained = format!("SELECT 1; {sql}");
            assert_ne!(
                classify(&chained),
                StmtClass::ReadOnly,
                "{chained} was sent to a replica"
            );
        }
    }

    #[test]
    fn a_column_named_after_a_non_reserved_keyword_still_reaches_replicas() {
        // The cost of an over-broad denylist, made concrete. All of these are
        // legal unquoted column names and all are queries somebody writes.
        for sql in [
            "SELECT comment FROM posts",
            "SELECT copy FROM documents",
            "SELECT call FROM logs",
            "SELECT security FROM policies",
            "SELECT import FROM batches",
            "SELECT lock FROM resources",
        ] {
            assert_eq!(
                classify(sql),
                StmtClass::ReadOnly,
                "{sql} was kept off replicas for no reason"
            );
        }
    }

    #[test]
    fn a_select_calling_a_writing_function_is_unknown_rather_than_read_only() {
        // These write, and all of them fail outright against a replica in
        // recovery. Unknown rather than Write: the scan does not understand the
        // statement to modify data, it declines to vouch for it.
        for sql in [
            "SELECT nextval('s')",
            "SELECT setval('s', 1)",
            "SELECT pg_advisory_lock(1)",
            "SELECT pg_try_advisory_xact_lock(1)",
            "SELECT txid_current()",
            "SELECT pg_logical_emit_message(true, 'a', 'b')",
            "SELECT lo_create(0)",
        ] {
            assert_eq!(classify(sql), StmtClass::Unknown, "{sql}");
        }
    }

    #[test]
    fn a_schema_qualified_writing_function_is_still_caught() {
        // The scan yields `pg_catalog`, `nextval` and `(` separately, so
        // matching the bare name covers the qualified form for free.
        assert_eq!(
            classify("SELECT pg_catalog.nextval('s')"),
            StmtClass::Unknown
        );
    }

    #[test]
    fn a_writing_function_does_not_downgrade_a_known_write() {
        // It must not turn a Write back into an Unknown. Both route to the
        // primary today, but the two mean different things and the confident
        // one should survive.
        assert_eq!(
            classify("INSERT INTO t VALUES (nextval('s'))"),
            StmtClass::Write
        );
    }

    #[test]
    fn a_harmlessly_volatile_function_stays_read_only() {
        // `random()` and `clock_timestamp()` are volatile in the catalogue and
        // perfectly safe on a replica. Treating volatility itself as the signal
        // would keep ordinary reads off replicas for no benefit.
        for sql in [
            "SELECT random()",
            "SELECT clock_timestamp()",
            "SELECT now()",
            "SELECT * FROM t ORDER BY random() LIMIT 1",
        ] {
            assert_eq!(classify(sql), StmtClass::ReadOnly, "{sql}");
        }
    }

    #[test]
    fn a_column_named_like_a_writing_function_is_not_one() {
        assert_eq!(classify("SELECT nextval_cache FROM t"), StmtClass::ReadOnly);
        assert_eq!(
            classify("SELECT * FROM t WHERE a = 'nextval'"),
            StmtClass::ReadOnly
        );
    }

    #[test]
    fn a_read_only_transaction_is_recognised_in_its_several_spellings() {
        for sql in [
            "BEGIN READ ONLY",
            "begin read only",
            "BEGIN TRANSACTION READ ONLY",
            "START TRANSACTION READ ONLY",
            "START TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY",
            "SET TRANSACTION READ ONLY",
            "BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY",
        ] {
            assert!(begins_read_only_transaction(sql), "{sql}");
        }
    }

    #[test]
    fn the_absence_of_a_mode_is_not_a_promise_of_one() {
        // The default is read write. Reading a bare BEGIN as read only would
        // send a whole transaction's writes to a replica.
        for sql in [
            "BEGIN",
            "BEGIN READ WRITE",
            "START TRANSACTION",
            "START TRANSACTION READ WRITE",
            "BEGIN ISOLATION LEVEL SERIALIZABLE",
            "SELECT 1",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            "",
            "'BEGIN READ ONLY'",
        ] {
            assert!(!begins_read_only_transaction(sql), "{sql:?}");
        }
    }

    #[test]
    fn only_the_first_statement_can_open_the_transaction() {
        // A READ ONLY appearing after a semicolon belongs to a later statement
        // and says nothing about the transaction this one opens.
        assert!(!begins_read_only_transaction(
            "BEGIN; SET TRANSACTION READ ONLY"
        ));
        assert!(begins_read_only_transaction("BEGIN READ ONLY; SELECT 1"));
    }

    #[test]
    fn case_and_whitespace_do_not_change_the_answer() {
        for sql in [
            "delete from t",
            "DELETE FROM t",
            "DeLeTe\tFROM\n\nt",
            "  \r\n delete  from  t ",
        ] {
            assert_eq!(classify(sql), StmtClass::Write, "{sql:?}");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    /// The statements that modify data, in the positions they really appear.
    fn dml_clause() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("INSERT INTO t VALUES (1)".to_owned()),
            Just("UPDATE t SET a = 1".to_owned()),
            Just("DELETE FROM t".to_owned()),
            Just("MERGE INTO t USING s ON true WHEN MATCHED THEN DELETE".to_owned()),
            Just("TRUNCATE t".to_owned()),
        ]
    }

    /// Whitespace and comments, which must never change an answer.
    fn trivia() -> impl Strategy<Value = String> {
        prop_oneof![
            Just(" ".to_owned()),
            Just("\n".to_owned()),
            Just("\t  \r\n".to_owned()),
            Just(" /* a comment */ ".to_owned()),
            Just(" /* outer /* nested */ still outer */ ".to_owned()),
            Just(" -- a line comment\n".to_owned()),
        ]
    }

    /// Randomly recases a keyword, since SQL keywords are case-insensitive and
    /// a classifier that only matched lowercase would pass every hand-written
    /// test above.
    fn recase(sql: &str, mask: u64) -> String {
        sql.chars()
            .enumerate()
            .map(|(i, c)| {
                if mask >> (i % 64) & 1 == 1 {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect()
    }

    /// Wraps a DML clause in something that still bears it.
    fn bearing_dml() -> impl Strategy<Value = String> {
        (dml_clause(), trivia(), trivia(), 0..6_u8, any::<u64>()).prop_map(
            |(dml, before, after, shape, mask)| {
                let dml = recase(&dml, mask);
                match shape {
                    // On its own.
                    0 => format!("{before}{dml}{after}"),
                    // Inside a data-modifying CTE.
                    1 => format!("{before}WITH x AS ({dml} RETURNING *){after}SELECT * FROM x"),
                    2 => format!(
                        "{before}WITH RECURSIVE a AS (SELECT 1), b AS ({dml} RETURNING *) \
                         SELECT * FROM a, b{after}"
                    ),
                    // After a harmless read, as the simple query protocol
                    // permits in one message.
                    3 => format!("SELECT 1;{before}{dml}{after}"),
                    // Before one.
                    4 => format!("{before}{dml}{after}; SELECT 1"),
                    // Buried among several.
                    _ => format!("SELECT 1; SELECT 2;{before}{dml}{after}; SELECT 3"),
                }
            },
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2_000))]

        /// The property this milestone exists to hold.
        ///
        /// A false negative sends a read to the primary and costs throughput.
        /// A false positive sends a write's read to a replica and returns stale
        /// data, which the tenant experiences as their write having vanished.
        #[test]
        fn no_dml_bearing_statement_is_ever_classified_read_only(sql in bearing_dml()) {
            prop_assert_ne!(
                classify(&sql),
                StmtClass::ReadOnly,
                "a statement that modifies data was sent to a replica: {:?}",
                sql
            );
        }

        /// Arbitrary bytes must not panic. This parses input from the internet,
        /// so a panic here is a denial of service.
        #[test]
        fn any_input_classifies_without_panicking(sql in ".{0,200}") {
            let _ = classify(&sql);
        }

        /// Locking clauses are writes however they are spelled.
        #[test]
        fn a_locking_select_is_never_read_only(
            strength in prop_oneof![
                Just("UPDATE"), Just("NO KEY UPDATE"), Just("SHARE"), Just("KEY SHARE"),
            ],
            wait in prop_oneof![Just(""), Just(" NOWAIT"), Just(" SKIP LOCKED")],
            mask in any::<u64>(),
        ) {
            let sql = recase(&format!("SELECT * FROM t FOR {strength}{wait}"), mask);
            prop_assert_ne!(classify(&sql), StmtClass::ReadOnly, "{:?}", sql);
        }

        /// The other half: ordinary reads must actually reach replicas, or the
        /// classifier could satisfy the property above by calling everything a
        /// write and the feature would be worthless.
        #[test]
        fn an_ordinary_read_stays_replica_eligible(
            // Prefixed, because an unprefixed generator produces identifiers
            // like `do` and `lock`, which are reserved words that real SQL has
            // to quote. The classifier is right to treat those as keywords, so
            // generating them tests the generator rather than the classifier.
            column in "col_[a-z_]{0,12}",
            table in "tbl_[a-z_]{0,12}",
            lead in trivia(),
            mask in any::<u64>(),
        ) {
            let sql = format!("{lead}{}", recase(&format!("SELECT {column} FROM {table}"), mask));
            prop_assert_eq!(
                classify(&sql),
                StmtClass::ReadOnly,
                "an ordinary read was kept off replicas: {:?}",
                sql
            );
        }
    }
}
