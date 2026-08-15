//! Turning SQL text into the form the cache key holds.
//!
//! # What this is for
//!
//! Two clients sending the same query rarely send the same bytes. One
//! pretty-prints, one puts the keywords in capitals, one has a comment from an
//! ORM at the front. All three ask the server the same question, and a cache
//! keyed on raw text answers it three times.
//!
//! # The rule, which is Postgres's own
//!
//! Outside quotes, SQL is case-insensitive and whitespace is a separator.
//! Inside them, neither is true: `'a'` and `'A'` are different values, and
//! `"MyTable"` and `"mytable"` are different tables. So a word is lowercased,
//! quoted text is copied byte for byte, and a run of whitespace or comments
//! becomes exactly one space.
//!
//! One space *only where the source had trivia*, rather than between every
//! token. Always spacing would turn `1.5` and `1 . 5` into the same key, and
//! while one of those is a syntax error the property this module claims is
//! that normalising never merges two statements a server would answer
//! differently. An error is an answer.
//!
//! # What it does not do
//!
//! Literals are not replaced by placeholders. `SELECT 1` and `SELECT 2` keep
//! separate keys, which costs entries and is the safe direction: merging them
//! would need the confidence that the literal reaches the server only as a
//! value, and that is a parser's judgement rather than a lexer's. See the
//! `M9.4` note in the backlog.
//!
//! # Why the scanner is borrowed rather than written here
//!
//! `pgprox_core::sql` exists because `pgprox-pool` and `pgprox-route` grew
//! separate scanners that disagreed about where an `E'...'` string ends, and a
//! session went unpinned as a result. A third scanner here would be the same
//! mistake with a longer gap before anyone noticed.
//!
//! It takes some care to use, because `Token::Quoted` deliberately does not
//! carry its contents: handing them out invites a caller to search them, which
//! is how a tenant's own data starts changing how their queries are treated.
//! This module does not search them. It needs the bytes only to copy them, and
//! it gets them by measuring how far the lexer moved rather than by asking the
//! token what it held.

use pgprox_core::sql::{Lexer, Token};

/// The cache key's form of a statement.
///
/// Case-folded outside quotes, verbatim inside them, and trivia collapsed to a
/// single space where there was any.
///
/// ```
/// use pgprox_cache::normalize::normalize;
///
/// // Case and layout do not change the question.
/// assert_eq!(normalize("SELECT   1"), normalize("select 1"));
///
/// // A comment is whitespace.
/// assert_eq!(normalize("SELECT /* hi */ 1"), normalize("SELECT 1"));
///
/// // What is inside quotes is not.
/// assert_ne!(normalize("SELECT 'a'"), normalize("SELECT 'A'"));
/// ```
#[must_use]
pub fn normalize(sql: &str) -> String {
    let mut lexer = Lexer::new(sql);
    let mut out = String::with_capacity(sql.len());
    let mut first = true;

    loop {
        // Trivia is skipped here rather than left to `next`, because whether
        // there *was* any is part of the answer. `next` would swallow it and
        // the gap between two tokens would be lost.
        let before_trivia = lexer.rest();
        lexer.skip_trivia();
        let had_gap = lexer.rest().len() != before_trivia.len();

        // The token's own text, measured by how far the lexer moved. This is
        // the only way to keep a quoted string verbatim without the token type
        // carrying contents no other caller should have.
        let before = lexer.rest();
        let Some(token) = lexer.next() else { break };
        let consumed = before.len() - lexer.rest().len();
        let text = &before[..consumed];

        if had_gap && !first {
            out.push(' ');
        }
        first = false;

        match token {
            // Unquoted words are folded, because the server folds them: a
            // table written `MyTable` and one written `mytable` are one table.
            //
            // ASCII-only, matching `pgprox_core::sql::statement_words`'s own
            // convention, and not `char::to_lowercase`. `M90.11`. Unicode's
            // *unconditional* special-casing table has one lower-casing
            // expansion: `İ` (U+0130) folds to the two codepoints `i` +
            // COMBINING DOT ABOVE (U+0307). `pgprox_core::sql::is_word_char`
            // treats every non-ASCII character, combining marks included, as
            // a word character by design, so `İ` and `i` + U+0307 lex as two
            // different single-token words that folded to the same output
            // here — one table's name and a different, independently
            // typeable spelling, colliding on one cache key. Postgres's own
            // identifier downcasing is a codepoint-for-codepoint transform
            // and never performs this expansion, so this was a deviation
            // from "the rule, which is Postgres's own" this module's own
            // module doc claims, not a matter of taste. Folding only ASCII
            // guarantees the transform can never change a word's codepoint
            // count, so two distinct source identifiers can never be
            // conflated by it — the same safe-direction trade the string
            // introducer's case already makes: one extra cache entry costs
            // memory, one cache entry for two questions serves wrong data.
            Token::Word(_) => out.extend(text.chars().map(|c| {
                if c.is_ascii() {
                    c.to_ascii_lowercase()
                } else {
                    c
                }
            })),
            // Everything else goes through untouched. For `Quoted` that is the
            // point; for punctuation there is no case to fold.
            _ => out.push_str(text),
        }
    }

    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The tokens of a statement, with words folded and quoted text kept.
    ///
    /// The model the properties below compare against: two statements Postgres
    /// answers the same way have the same sequence here.
    fn meaning(sql: &str) -> Vec<String> {
        let mut lexer = Lexer::new(sql);
        let mut out = Vec::new();
        loop {
            lexer.skip_trivia();
            let before = lexer.rest();
            let Some(token) = lexer.next() else { break };
            let text = &before[..before.len() - lexer.rest().len()];
            out.push(match token {
                Token::Word(_) => text.to_lowercase(),
                _ => text.to_owned(),
            });
        }
        out
    }

    #[test]
    fn layout_and_case_do_not_change_the_key() {
        // The reason this module exists. Three clients, one question.
        let canonical = normalize("select * from orders where id = 1");
        for spelling in [
            "SELECT * FROM orders WHERE id = 1",
            "select\n  *\n  from orders\n  where id = 1",
            "SeLeCt   *   FrOm   orders   WhErE   id   =   1",
            "/* orm v3 */ select * from orders where id = 1",
            "select * from orders where id = 1 -- trailing",
        ] {
            assert_eq!(normalize(spelling), canonical, "{spelling:?}");
        }
    }

    #[test]
    fn a_string_literal_keeps_its_case_and_its_spaces() {
        // Inside quotes nothing is a separator and nothing folds. A cache that
        // folded here would serve one tenant's row for another's query.
        assert_ne!(normalize("SELECT 'a'"), normalize("SELECT 'A'"));
        assert_ne!(normalize("SELECT 'a  b'"), normalize("SELECT 'a b'"));
        assert!(normalize("SELECT 'Hello   World'").contains("'Hello   World'"));
    }

    #[test]
    fn a_quoted_identifier_keeps_its_case() {
        // `"MyTable"` and `"mytable"` are two tables in Postgres, and this is
        // the difference between a cache and a data leak.
        assert_ne!(
            normalize(r#"SELECT * FROM "MyTable""#),
            normalize(r#"SELECT * FROM "mytable""#)
        );
    }

    #[test]
    fn an_unquoted_identifier_folds_because_the_server_folds_it() {
        assert_eq!(
            normalize("SELECT * FROM MyTable"),
            normalize("select * from mytable")
        );
    }

    #[test]
    fn folding_never_changes_a_words_codepoint_count() {
        // `M90.11`. Unicode's unconditional special-casing table has exactly
        // one lower-casing expansion: `İ` (U+0130, one codepoint) folds to
        // `i` + COMBINING DOT ABOVE (U+0307, two codepoints). Both `İ` and a
        // source that already spells `i` followed by U+0307 lex as one
        // `Token::Word` each — every non-ASCII character is a word character
        // by `pgprox_core::sql::is_word_char`'s own design — so `char::
        // to_lowercase` folded two different, independently typeable
        // identifiers onto the same cache key. Postgres's own identifier
        // downcasing never expands a codepoint into two, so an ASCII-only
        // fold is what stays faithful to it, and it is what this asserts:
        // two distinct source spellings must stay distinct keys.
        let a = normalize("SELECT * FROM \u{130}");
        let b = normalize("SELECT * FROM i\u{307}");
        assert_ne!(
            a, b,
            "two different identifiers collapsed onto one cache key: {a:?} vs {b:?}"
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        // The bug `pgprox_core::sql` was extracted to prevent, arriving here.
        // A scanner that ended the string at the escaped quote would copy the
        // rest of the statement as if it were a literal, and two statements
        // differing after that point would key the same.
        let a = normalize(r"SELECT E'\'' , 1");
        let b = normalize(r"SELECT E'\'' , 2");
        assert_ne!(a, b, "text after an escaped quote was swallowed");
    }

    #[test]
    fn a_string_introducer_keeps_its_case_which_costs_an_entry() {
        // A known imprecision, named so it is deliberate. `E'x'` and `e'x'`
        // are the same string to Postgres, but the introducer is part of the
        // quoted token's source and this module copies that source verbatim
        // rather than reaching inside it.
        //
        // Separating the introducer from the contents would mean parsing
        // within the span the lexer already decided was quoted, which is the
        // second-scanner mistake this module opens by refusing to make. The
        // cost is one extra entry for a spelling almost nobody uses, and the
        // direction is safe: two keys for one question wastes memory, one key
        // for two questions serves wrong data.
        assert_ne!(normalize(r"SELECT E'x'"), normalize(r"SELECT e'x'"));
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_split_anything() {
        assert_eq!(normalize("SELECT 'a; b'"), "select 'a; b'");
    }

    #[test]
    fn dollar_quoted_text_survives() {
        let body = "$fn$ SELECT 'A'; $fn$";
        assert!(normalize(&format!("SELECT {body}")).contains(body));
    }

    #[test]
    fn tokens_that_touch_stay_touching() {
        // The reason a space goes only where the source had trivia. Spacing
        // every token would turn these two into one key, and one of them is a
        // syntax error, which is an answer the server gives differently.
        assert_ne!(normalize("SELECT 1.5"), normalize("SELECT 1 . 5"));
        assert_eq!(normalize("SELECT $1"), "select $1");
        assert_eq!(normalize("count(*)"), "count(*)");
    }

    #[test]
    fn two_words_do_not_become_one() {
        // The failure that would be silent: `a b` and `ab` are different
        // things, and a normaliser that dropped the separator would merge them.
        assert_ne!(normalize("select a b"), normalize("select ab"));
    }

    #[test]
    fn an_empty_statement_normalises_to_nothing() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   \n  "), "");
        assert_eq!(normalize("-- just a comment"), "");
    }

    #[test]
    fn normalising_is_idempotent() {
        // A key derived twice is the same key. If this failed, a result stored
        // through one path could never be found through another.
        for sql in [
            "SELECT   *  FROM t",
            "SELECT 'a  b' /* c */ , 1",
            r"SELECT E'\'' ",
            r#"UPDATE "T" SET x = 1"#,
            "",
        ] {
            let once = normalize(sql);
            assert_eq!(normalize(&once), once, "{sql:?}");
        }
    }

    #[test]
    fn normalising_preserves_what_the_server_would_read() {
        // The property, stated against a model rather than an example: the
        // token sequence, folded the way the server folds it, is unchanged.
        // Anything normalisation did that a server would notice shows up here.
        for sql in [
            "SELECT * FROM orders WHERE id = $1",
            "/* hint */ SELECT 'a; b', \"Col\" FROM T",
            r"INSERT INTO t VALUES (E'\'', 2)",
            "BEGIN; SET search_path = a; SELECT 1; COMMIT",
            "select count(*) from t where x between 1 and 2",
            "",
        ] {
            assert_eq!(meaning(sql), meaning(&normalize(sql)), "{sql:?}");
        }
    }

    #[test]
    fn statements_a_server_answers_differently_get_different_keys() {
        // The direction that matters. Everything here differs in a way the
        // server acts on, so nothing here may collide.
        let all = [
            "SELECT 1",
            "SELECT 2",
            "SELECT a",
            "SELECT 'a'",
            "SELECT 'A'",
            r#"SELECT "a""#,
            "SELECT a, b",
            "SELECT * FROM t",
            "SELECT * FROM u",
            "DELETE FROM t",
            "SELECT $1",
            "SELECT $2",
        ];
        for (i, left) in all.iter().enumerate() {
            for right in &all[i + 1..] {
                assert_ne!(
                    normalize(left),
                    normalize(right),
                    "{left:?} and {right:?} share a key"
                );
            }
        }
    }
}
