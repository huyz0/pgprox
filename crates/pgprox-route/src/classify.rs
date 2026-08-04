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
const READ_FIRST_WORDS: WordSet = WordSet::new(&["select", "with", "table", "values", "explain"]);

/// A keyword list, and a filter over it computed at compile time.
///
/// # Why the lists are not searched directly
///
/// They were, and it cost 1,935 instructions of the route decision's 6,444: a
/// read-only statement asks about every word twice, once against
/// [`WRITE_WORDS`] and once against [`WRITING_FUNCTIONS`], so the reference
/// point select ran about 290 comparisons to find no match at all. `M30.2`.
///
/// Almost all of those comparisons are against words that share nothing with
/// the one being looked up. `pgbench_accounts` is sixteen bytes and ends in
/// `s`; nothing on either list is both. So three facts about a word are kept
/// as bitmasks, and a word disagreeing with any of them cannot be on the list
/// and is refused before a single comparison happens.
///
/// # Why these three facts
///
/// Length, first byte and last byte, because each is one load and one shift and
/// the three are close to independent for identifiers. They are a filter and
/// not an answer: passing all three means the scan still runs, which is why
/// [`matches_any`] ends where it always did.
///
/// The lists themselves are untouched, and that is deliberate rather than
/// convenient. Every entry carries a comment naming the construct that requires
/// it, and those comments are the reason the lists are correct.
struct WordSet {
    words: &'static [&'static str],
    /// Bit `n` set if some word is `n` bytes long.
    lengths: u64,
    /// Bit `n` set if some word starts with the `n`th letter of the alphabet.
    initials: u32,
    /// The same for the last letter.
    finals: u32,
}

impl WordSet {
    /// Builds the filter by reading the list.
    ///
    /// Derived rather than written down, so a word added to a list cannot be
    /// added without its filter bits. That failure would be a statement
    /// silently classified as a read, which is the one direction this crate
    /// must not be wrong in.
    const fn new(words: &'static [&'static str]) -> Self {
        let mut lengths = 0_u64;
        let mut initials = 0_u32;
        let mut finals = 0_u32;

        let mut at = 0;
        while at < words.len() {
            let bytes = words[at].as_bytes();
            assert!(
                !bytes.is_empty() && bytes.len() < 64,
                "a keyword this long cannot be filtered by a 64-bit length mask"
            );
            assert!(
                bytes[0].is_ascii_lowercase() && bytes[bytes.len() - 1].is_ascii_lowercase(),
                "the lists are lowercase and start and end in a letter"
            );

            lengths |= 1 << bytes.len();
            initials |= 1 << (bytes[0] - b'a');
            finals |= 1 << (bytes[bytes.len() - 1] - b'a');
            at += 1;
        }

        Self {
            words,
            lengths,
            initials,
            finals,
        }
    }

    /// Whether the filter can rule `word` out without comparing it to anything.
    ///
    /// Answers `true` for a word that might be on the list and `false` only for
    /// one that certainly is not, which is the direction that has to be exact.
    fn might_hold(&self, word: &str) -> bool {
        let bytes = word.as_bytes();

        // Length first, and it also covers the empty word: no entry is empty,
        // so bit zero is clear and the indexing below is unreachable for one.
        if bytes.len() >= 64 || self.lengths & (1 << bytes.len()) == 0 {
            return false;
        }

        // `| 0x20` lowercases an ASCII letter and moves everything else
        // somewhere that is not one, which `is_ascii_lowercase` then refuses.
        // A word starting with a digit, an underscore or a multi-byte character
        // is on neither list, and Postgres identifiers may be any of the three.
        let first = bytes[0] | 0x20;
        let last = bytes[bytes.len() - 1] | 0x20;
        if !first.is_ascii_lowercase() || !last.is_ascii_lowercase() {
            return false;
        }

        self.initials & (1 << (first - b'a')) != 0 && self.finals & (1 << (last - b'a')) != 0
    }
}

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
const WRITE_WORDS: WordSet = WordSet::new(&[
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
]);

/// Functions that write, and so cannot run on a replica.
///
/// Matched on the bare name, so `nextval` and `pg_catalog.nextval` both hit:
/// the scan yields `pg_catalog`, `nextval` and the parenthesis separately.
///
/// Not a list of every `VOLATILE` function. `random()` is volatile and
/// perfectly safe to route; these are the ones with side effects.
const WRITING_FUNCTIONS: WordSet = WordSet::new(&[
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
]);

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
                    class = if matches_any(word, &READ_FIRST_WORDS) {
                        StmtClass::ReadOnly
                    } else {
                        StmtClass::Unknown
                    };
                }
                if matches_any(word, &WRITE_WORDS) {
                    // Not an early return: the rest of the statement still has
                    // to be consumed so the scanner is positioned for the next
                    // one, and a `;` inside a string must not be mistaken for a
                    // separator.
                    class = StmtClass::Write;
                } else if class == StmtClass::ReadOnly && matches_any(word, &WRITING_FUNCTIONS) {
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
///
/// The filter first, which answers most words. See [`WordSet`] for why: the
/// scan below is the same one it always was, and it now runs on the words that
/// could plausibly be on the list rather than on every word of every statement.
fn matches_any(word: &str, set: &WordSet) -> bool {
    set.might_hold(word)
        && set
            .words
            .iter()
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
    // One pass, no allocation, and it stops as soon as the answer is fixed.
    //
    // The stopping is the part that matters. This runs on the route decision's
    // hot path for every statement outside a transaction, next to `classify`,
    // which lexes the same text; reading to the end here made that two full
    // passes over every statement the router sees. Three words can open a
    // transaction and nothing later in the statement can change that, so a
    // statement beginning with any other word is answered after one token.
    // `M30.1` measured the difference on the reference point select.
    let mut lexer = Lexer::new(sql);

    let opener = loop {
        match lexer.next() {
            Some(Token::Word(word)) => break word,
            // Punctuation says nothing: `(BEGIN` is not valid SQL, but leading
            // punctuation must not be read as the statement's first word.
            Some(Token::Punct(_)) => {}
            // Quoted text, a leading separator, or nothing at all. A
            // transaction-opening statement is none of those.
            _ => return false,
        }
    };

    // `SET` opens one only as `SET TRANSACTION`. Every other `SET` assigns a
    // parameter, including `SET SESSION CHARACTERISTICS AS TRANSACTION READ
    // ONLY`, which says what later transactions will do rather than opening
    // one, so the second word has to be checked rather than searched for.
    let sets = opener.eq_ignore_ascii_case("set");
    if !sets && !opener.eq_ignore_ascii_case("begin") && !opener.eq_ignore_ascii_case("start") {
        return false;
    }

    let mut previous: Option<&str> = None;
    let mut at_second_word = true;
    let mut read_only = false;

    for token in lexer {
        match token {
            // Only the first statement can open the transaction.
            Token::Semicolon => break,
            // A transaction-opening statement has no quoted text in it, so
            // anything quoted means this is not one.
            Token::Quoted => return false,
            Token::Punct(_) => {}
            Token::Word(word) => {
                if at_second_word {
                    at_second_word = false;
                    if sets && !word.eq_ignore_ascii_case("transaction") {
                        return false;
                    }
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

    read_only
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
            // Sets the default for transactions that have not started yet, and
            // opens none. `M10.6` found that replacing the `&&` in the `SET`
            // arm with an `||` survived every case here, because none of them
            // was a `SET` that is read only and is not a transaction.
            "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY",
            "",
            "'BEGIN READ ONLY'",
        ] {
            assert!(!begins_read_only_transaction(sql), "{sql:?}");
        }
    }

    #[test]
    fn a_statement_that_cannot_open_a_transaction_is_answered_by_its_first_words() {
        // `M30.1` made this stop as soon as the answer is fixed, because the
        // route decision was lexing every statement twice: once here and once
        // in `classify`. The stopping is only sound if no later word can change
        // the answer, so these are the cases where a later word tries to.
        //
        // Every one of them says READ ONLY somewhere and none of them opens a
        // read-only transaction. Reading further would find those two words and
        // has to not matter.
        for sql in [
            // The first word is not one of the three that can open one.
            // `read` and `only` are legal unquoted column names, which is what
            // makes the first of these a query somebody writes rather than a
            // shape invented to fail.
            "SELECT read, only FROM t",
            "SELECT 1 WHERE mode = 'READ ONLY'",
            "UPDATE t SET note = 'read only'",
            "COMMIT AND CHAIN /* READ ONLY */",
            // The first word is `SET`, so the second decides. Neither of these
            // is `SET TRANSACTION`, and both continue into words that would
            // otherwise say yes.
            "SET search_path = read, only",
            "SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY",
        ] {
            assert!(!begins_read_only_transaction(sql), "{sql:?}");
        }

        // And the one that does have to read past its second word, so the
        // early exit cannot be a blanket one.
        assert!(begins_read_only_transaction(
            "START TRANSACTION ISOLATION LEVEL SERIALIZABLE, READ ONLY"
        ));
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

    /// The three lists, so a check written about one is written about all.
    const EVERY_SET: &[(&str, &WordSet)] = &[
        ("READ_FIRST_WORDS", &READ_FIRST_WORDS),
        ("WRITE_WORDS", &WRITE_WORDS),
        ("WRITING_FUNCTIONS", &WRITING_FUNCTIONS),
    ];

    /// The scan `matches_any` did before `M30.2` put a filter in front of it.
    ///
    /// The comparison the filter has to survive, kept here rather than deleted,
    /// because "the fast one agrees with the slow one" is a claim that needs
    /// both of them.
    /// A word taken from one of the lists, cased at random and optionally
    /// edited by one character.
    ///
    /// The generator that makes the agreement property mean something. An
    /// entry with one byte changed, one appended, or one removed is exactly a
    /// word the filter has to decide about and the scan has to reject, and it
    /// is what a random string generator will never produce.
    fn near_a_keyword() -> impl Strategy<Value = String> {
        let every: Vec<&'static str> = EVERY_SET
            .iter()
            .flat_map(|(_, set)| set.words.iter().copied())
            .collect();

        (
            proptest::sample::select(every),
            any::<u64>(),
            0_u8..4,
            any::<char>(),
        )
            .prop_map(|(word, case_mask, edit, extra)| {
                let mut cased: String = word
                    .chars()
                    .enumerate()
                    .map(|(at, c)| {
                        if case_mask >> (at % 64) & 1 == 1 {
                            c.to_ascii_uppercase()
                        } else {
                            c
                        }
                    })
                    .collect();

                match edit {
                    // Unedited, so the entries themselves are generated too.
                    0 => {}
                    // One longer, one shorter, and one character different:
                    // the three ways a word can sit next to a keyword.
                    1 => cased.push(extra),
                    2 => {
                        cased.pop();
                    }
                    _ => {
                        cased.pop();
                        cased.push(extra);
                    }
                }
                cased
            })
    }

    fn scan_only(word: &str, set: &WordSet) -> bool {
        set.words
            .iter()
            .any(|candidate| word.eq_ignore_ascii_case(candidate))
    }

    #[test]
    fn the_filter_lets_every_word_on_every_list_through() {
        // The direction that matters. A filter rejecting a word that is on the
        // list turns a write into a read, which is the one mistake this crate
        // is not allowed to make, and it would do it silently: every test above
        // asserts about statements, and a word quietly stopped being found is
        // a statement quietly changing class.
        for (name, set) in EVERY_SET {
            for word in set.words {
                assert!(set.might_hold(word), "{name}: {word} was filtered out");
                assert!(matches_any(word, set), "{name}: {word} is not found");

                // The scan is case insensitive, so the filter has to be too.
                assert!(
                    matches_any(&word.to_uppercase(), set),
                    "{name}: {word} is not found in upper case"
                );
            }
        }
    }

    #[test]
    fn the_filter_is_a_filter_and_not_an_answer() {
        // A word agreeing with all three facts and being on no list. Without
        // this, a `might_hold` that returned `true` for everything would pass
        // the test above and every other test in this file, because the scan
        // behind it would still be doing the work.
        //
        // `shade` is five bytes, starts with `s` and ends with `e`, all of
        // which `share` also is. It is not on the list.
        assert!(WRITE_WORDS.might_hold("shade"));
        assert!(!matches_any("shade", &WRITE_WORDS));

        // And one the filter does reject, so it is doing something at all.
        assert!(!WRITE_WORDS.might_hold("pgbench_accounts"));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2_000))]

        /// The filter never changes an answer, for any input at all.
        ///
        /// The unit tests above cover the entries and two hand-picked words.
        /// This covers the rest of the space, which is where a filter goes
        /// wrong: a length, a case, or a byte nobody thought to write down.
        ///
        /// Most of the generator is [`near_a_keyword`] rather than arbitrary
        /// text, and that is not decoration. Four mutations were run against
        /// this milestone's filter, and with a generator of arbitrary strings
        /// this property caught none of them: two thousand random words never
        /// land on a thirty-word list, so it was asserting that two functions
        /// agree about text neither of them was ever going to match.
        #[test]
        fn the_filter_and_the_scan_agree_on_everything(
            word in prop_oneof![
                // Words one edit away from a real entry, which is where a
                // filter is wrong if it is wrong at all.
                4 => near_a_keyword(),
                // Arbitrary text, including non-ASCII and the empty string,
                // because Postgres identifiers may be either.
                1 => ".{0,40}",
                1 => "[a-zA-Z_]{0,20}",
            ],
        ) {
            for (name, set) in EVERY_SET {
                prop_assert_eq!(
                    matches_any(&word, set),
                    scan_only(&word, set),
                    "{}: the filter changed the answer for {:?}",
                    name,
                    word
                );
            }
        }

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
