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
//! Two conditions, both required for [`StmtClass::ReadOnly`]:
//!
//! 1. The **first** word is one of a short allowlist: `SELECT`, `WITH`,
//!    `TABLE`, `VALUES`, `EXPLAIN`.
//! 2. **No** word anywhere in the statement is on a denylist of things that
//!    write, lock, or have side effects.
//!
//! The second condition is what handles the cases a first-token scan gets
//! wrong, and it handles them without special-casing any of them:
//! `WITH x AS (INSERT ...)` contains `insert`, `SELECT ... FOR UPDATE` contains
//! `update`, `FOR SHARE` contains `share`, `EXPLAIN ANALYZE` contains
//! `analyze`, and `SELECT ... INTO t` contains `into`.
//!
//! The asymmetry is deliberate. Adding a word to the denylist can only move
//! statements from `ReadOnly` toward the primary, so an over-broad denylist
//! costs throughput. Missing one costs correctness.
//!
//! # Not a parser
//!
//! This is a scan, per ADR 0009. It does not build a tree, does not resolve
//! names, and does not know which functions exist. It has to be right about
//! *lexical structure* though, because that is what decides whether a word is
//! SQL or the contents of a string literal, and mistaking one for the other in
//! the wrong direction is how a write gets called a read.

use pgprox_core::route::StmtClass;

/// Statements that may be reads, by their first word.
///
/// Deliberately short. Anything not here is [`StmtClass::Unknown`] and goes to
/// the primary, which is the correct answer for transaction control, `SET`,
/// `SHOW`, DDL, and every construct this list has not learned yet.
const READ_FIRST_WORDS: &[&str] = &["select", "with", "table", "values", "explain"];

/// Words that disqualify a statement from being a read, wherever they appear.
///
/// Read this as "if any of these is a bare word outside a string, assume the
/// worst". A word here that turns out to be harmless costs a replica read. A
/// word missing from here that turns out to write costs a stale answer.
const WRITE_WORDS: &[&str] = &[
    // Data modification, including inside a CTE.
    "insert",
    "update",
    "delete",
    "merge",
    "truncate",
    "upsert",
    // Row locking. `FOR UPDATE` is caught by `update`; these cover the rest of
    // the locking clause forms.
    "share",
    "lock",
    // `SELECT ... INTO t` creates a table.
    "into",
    // `EXPLAIN ANALYZE` executes the plan for real. Plain `EXPLAIN` does not,
    // which is why `explain` is an allowed first word and `analyze` is not.
    "analyze",
    "analyse",
    // Schema changes.
    "create",
    "drop",
    "alter",
    "grant",
    "revoke",
    "comment",
    "refresh",
    "reindex",
    "vacuum",
    "reassign",
    "import",
    "security",
    // Session and connection state.
    "prepare",
    "deallocate",
    "declare",
    "discard",
    "listen",
    "unlisten",
    "notify",
    "copy",
    // Anything that runs arbitrary code.
    "call",
    "do",
    // Transaction control has no business being appended to a read, and if it
    // is, the transaction's target was fixed by its first statement anyway.
    "begin",
    "commit",
    "rollback",
    "savepoint",
    "checkpoint",
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
    let mut scanner = Scanner::new(sql);
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
fn classify_one(scanner: &mut Scanner<'_>) -> (StmtClass, bool) {
    let mut first = true;
    let mut class = StmtClass::Unknown;

    while let Some(piece) = scanner.next_piece() {
        match piece {
            Piece::Semicolon => {
                // An empty statement, as in a trailing semicolon, contributes
                // nothing rather than counting as an unclassifiable one.
                let class = if first { StmtClass::ReadOnly } else { class };
                return (class, scanner.has_more());
            }
            Piece::Opaque => {
                // A quoted identifier or a string. It is never a keyword, but
                // it does mean this statement has started.
                first = false;
            }
            Piece::Word(word) => {
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

/// One lexical unit the classifier cares about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Piece<'a> {
    /// A bare word, which may be a keyword.
    Word(&'a str),
    /// A string literal or quoted identifier. Never a keyword.
    Opaque,
    /// A statement separator.
    Semicolon,
}

/// Splits SQL into the pieces the classifier reasons about.
///
/// Everything it skips, comments and quoted text, it skips because that text is
/// not SQL. Getting that wrong in the direction of skipping too much is the
/// dangerous one: consuming past the end of a string would swallow the
/// statement after it, and `E'\''; DELETE FROM t` would look like one harmless
/// read. That case has its own test.
struct Scanner<'a> {
    rest: &'a str,
}

impl<'a> Scanner<'a> {
    const fn new(sql: &'a str) -> Self {
        Self { rest: sql }
    }

    /// Whether any input remains.
    fn has_more(&self) -> bool {
        !self.rest.is_empty()
    }

    /// The next piece, skipping whitespace, comments and punctuation.
    fn next_piece(&mut self) -> Option<Piece<'a>> {
        loop {
            self.skip_trivia();
            let mut chars = self.rest.char_indices();
            let (_, first) = chars.next()?;

            match first {
                ';' => {
                    self.advance(1);
                    return Some(Piece::Semicolon);
                }
                '\'' => {
                    self.skip_single_quoted(false);
                    return Some(Piece::Opaque);
                }
                '"' => {
                    self.skip_double_quoted();
                    return Some(Piece::Opaque);
                }
                '$' => {
                    if self.skip_dollar_quoted() {
                        return Some(Piece::Opaque);
                    }
                    // A parameter placeholder like `$1`, or a stray `$`.
                    self.advance(first.len_utf8());
                }
                c if is_word_char(c) => {
                    let end = self
                        .rest
                        .find(|c: char| !is_word_char(c))
                        .unwrap_or(self.rest.len());
                    let word = &self.rest[..end];
                    self.advance(end);

                    // `E'...'` and `U&'...'` are strings whose introducer looks
                    // like a word. Consume the string with the introducer, so
                    // its backslash escapes are honoured rather than leaving a
                    // dangling quote for the next round.
                    if self.rest.starts_with('\'') && is_string_introducer(word) {
                        self.skip_single_quoted(word.eq_ignore_ascii_case("e"));
                        return Some(Piece::Opaque);
                    }
                    return Some(Piece::Word(word));
                }
                other => {
                    // Operators, parentheses, commas. None of them changes the
                    // classification, and skipping them is what lets
                    // `(INSERT ...)` still yield `insert` as a word.
                    self.advance(other.len_utf8());
                }
            }
        }
    }

    fn advance(&mut self, bytes: usize) {
        self.rest = &self.rest[bytes.min(self.rest.len())..];
    }

    /// Skips whitespace and both comment forms.
    fn skip_trivia(&mut self) {
        loop {
            let trimmed = self.rest.trim_start();
            if trimmed.len() != self.rest.len() {
                self.rest = trimmed;
                continue;
            }
            if self.rest.starts_with("--") {
                let end = self.rest.find('\n').map_or(self.rest.len(), |i| i + 1);
                self.advance(end);
                continue;
            }
            if self.rest.starts_with("/*") {
                self.skip_block_comment();
                continue;
            }
            return;
        }
    }

    /// Skips a block comment, which nests in Postgres.
    fn skip_block_comment(&mut self) {
        let bytes = self.rest.as_bytes();
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
        // An unterminated comment consumes the rest, which is what the server
        // does with it too.
        self.advance(i);
    }

    /// Skips a single-quoted string.
    ///
    /// `''` is always an escaped quote. A backslash escapes the next character
    /// only in an `E'...'` string, which is why the caller says which it is:
    /// treating `'\''` as terminated in a plain string, or unterminated in an
    /// E-string, both misplace the end.
    fn skip_single_quoted(&mut self, backslash_escapes: bool) {
        let bytes = self.rest.as_bytes();
        let mut i = 1; // past the opening quote
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if backslash_escapes => i += 2,
                b'\'' if bytes.get(i + 1) == Some(&b'\'') => i += 2,
                b'\'' => {
                    i += 1;
                    break;
                }
                _ => i += 1,
            }
        }
        self.advance(i);
    }

    /// Skips a quoted identifier, in which `""` is an escaped quote.
    fn skip_double_quoted(&mut self) {
        let bytes = self.rest.as_bytes();
        let mut i = 1;
        while i < bytes.len() {
            match bytes[i] {
                b'"' if bytes.get(i + 1) == Some(&b'"') => i += 2,
                b'"' => {
                    i += 1;
                    break;
                }
                _ => i += 1,
            }
        }
        self.advance(i);
    }

    /// Skips a dollar-quoted string, returning whether one was there.
    ///
    /// The tag matters: `$$ ... $$` and `$body$ ... $body$` are different
    /// delimiters, and a `$$` inside a `$body$` string does not end it.
    fn skip_dollar_quoted(&mut self) -> bool {
        let bytes = self.rest.as_bytes();
        let Some(tag_end) = bytes[1..].iter().position(|b| *b == b'$') else {
            return false;
        };
        let tag = &self.rest[..=tag_end + 1];
        // A tag must be an identifier, so `$1$` is not dollar quoting.
        if tag[1..tag.len() - 1]
            .chars()
            .any(|c| c.is_ascii_digit() && tag.len() == 3)
        {
            return false;
        }

        let body = &self.rest[tag.len()..];
        let end = body.find(tag).map_or(self.rest.len(), |i| {
            // Past the closing tag.
            tag.len() + i + tag.len()
        });
        self.advance(end);
        true
    }
}

/// Whether a character can appear in a bare word.
///
/// Non-ASCII is included because Postgres identifiers may be, and treating a
/// multi-byte character as punctuation would split one word into two, which
/// could turn a harmless identifier into a keyword match.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || !c.is_ascii()
}

/// Whether a word introduces a string literal rather than being one.
fn is_string_introducer(word: &str) -> bool {
    word.eq_ignore_ascii_case("e")
        || word.eq_ignore_ascii_case("b")
        || word.eq_ignore_ascii_case("x")
        || word.eq_ignore_ascii_case("u")
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
            column in "[a-z][a-z_]{0,12}",
            table in "[a-z][a-z_]{0,12}",
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
