//! Splitting SQL text into the parts that are SQL and the parts that are not.
//!
//! # Why this is here rather than in the crates that use it
//!
//! `pgprox-route` decides whether a statement may reach a replica, and
//! `pgprox-pool` decides whether a session may be moved between connections.
//! Both answer their question by looking for keywords, and both are wrong in a
//! way that matters if they disagree about where a string literal ends.
//!
//! They did disagree. Each grew its own scanner, and `pgprox-pool`'s did not
//! honour backslash escapes inside `E'...'`, so `SELECT E'\'' ; LISTEN c` ended
//! the string early, read the rest as data, and left the session unpinned. A
//! missed pin hands one client another client's state.
//!
//! Deciding which text is SQL is one rule, so it has one implementation. Same
//! argument as [`crate::route::decide`], and the same reason: two
//! implementations of a rule are two chances to get it wrong, and the second
//! one is always the one nobody remembers to fix.
//!
//! # Both directions are dangerous
//!
//! Ending a quoted region early exposes its contents as SQL, so a row
//! containing the word `insert` looks like a write. Running past the end
//! swallows the statement after it, so a real `DELETE` disappears. The first
//! costs throughput, the second costs correctness, and the tests here name
//! which direction each case guards.
//!
//! # Not a parser
//!
//! No tree, no names, no types. It knows where comments and quoted text begin
//! and end, and nothing else about SQL.

/// One lexical unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Token<'a> {
    /// A bare word, which may be a keyword or an identifier.
    Word(&'a str),
    /// A quoted string or identifier. Never a keyword.
    ///
    /// The contents are deliberately not exposed. Every caller so far wants to
    /// know only that something was quoted, and handing out the text invites a
    /// caller to search it, which is how a tenant's own data starts changing
    /// how their queries are treated.
    Quoted,
    /// A statement separator.
    Semicolon,
    /// Anything else: operators, parentheses, commas.
    Punct(char),
}

/// Splits SQL into tokens, skipping comments and the insides of quoted text.
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    rest: &'a str,
}

impl<'a> Lexer<'a> {
    /// A lexer over `sql`.
    #[must_use]
    pub const fn new(sql: &'a str) -> Self {
        Self { rest: sql }
    }

    /// Whether any input remains.
    #[must_use]
    pub const fn has_more(&self) -> bool {
        !self.rest.is_empty()
    }

    /// The unconsumed input, for a caller that needs the raw text.
    #[must_use]
    pub const fn rest(&self) -> &'a str {
        self.rest
    }

    /// Skips whitespace and both comment forms.
    ///
    /// Public because a caller reading a leading hint comment needs to walk
    /// them one at a time rather than have them silently dropped.
    pub fn skip_trivia(&mut self) {
        loop {
            let trimmed = trim_leading_space(self.rest);
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
                let end = block_comment_end(self.rest);
                self.advance(end);
                continue;
            }
            return;
        }
    }

    fn advance(&mut self, bytes: usize) {
        self.rest = &self.rest[bytes.min(self.rest.len())..];
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        self.skip_trivia();
        let first = self.rest.chars().next()?;

        match first {
            ';' => {
                self.advance(1);
                Some(Token::Semicolon)
            }
            '\'' => {
                self.advance(single_quoted_end(self.rest, false));
                Some(Token::Quoted)
            }
            '"' => {
                self.advance(double_quoted_end(self.rest));
                Some(Token::Quoted)
            }
            '$' => {
                if let Some(end) = dollar_quoted_end(self.rest) {
                    self.advance(end);
                    return Some(Token::Quoted);
                }
                // A parameter placeholder like `$1`, or a stray `$`.
                self.advance(first.len_utf8());
                Some(Token::Punct(first))
            }
            c if is_word_char(c) => {
                let end = word_end(self.rest);
                // The lexer must consume something on every step, and this arm
                // is the one place where that depends on two functions agreeing.
                // `word_end` restates the rule `is_word_char` holds, inline and
                // over bytes, because it is the innermost loop of the route
                // decision. If the two ever disagree about one character, this
                // guard accepts it, `word_end` returns zero, and `next` spins
                // forever on a live connection.
                //
                // `M22.5` found that by mutation rather than by reading:
                // replacing `is_word_char` with `true` did not fail a test, it
                // hung the suite, and a timeout is what the tool had to report
                // because nothing could get far enough to disagree with it.
                debug_assert!(
                    end > 0,
                    "the lexer accepted a character word_end will not consume: {c:?}"
                );
                let word = &self.rest[..end];
                self.advance(end);

                // `E'...'`, `B'...'`, `X'...'` and `U&'...'` are strings whose
                // introducer looks like a word. Consuming the string here is
                // what honours its escapes; leaving the quote for the next
                // round would treat `E'\''` as terminated and expose the rest
                // of the statement as SQL.
                if is_string_introducer(word) {
                    // `U&'...'` puts an ampersand between the introducer and
                    // the quote, which is why this is not a simple prefix
                    // check.
                    let ampersand = word.eq_ignore_ascii_case("u") && self.rest.starts_with("&'");
                    if ampersand {
                        self.advance(1);
                    }
                    if self.rest.starts_with('\'') {
                        let backslashes = word.eq_ignore_ascii_case("e");
                        self.advance(single_quoted_end(self.rest, backslashes));
                        return Some(Token::Quoted);
                    }
                }
                Some(Token::Word(word))
            }
            other => {
                self.advance(other.len_utf8());
                Some(Token::Punct(other))
            }
        }
    }
}

/// Each statement in `sql`, as the text between the separators.
///
/// [`statement_words`] is the shape a caller matching keywords wants, and it
/// drops quoted text on purpose. A caller that needs the statement's own text,
/// values and all, wants this instead: the split is the part that has to be
/// right about lexical structure, and the part nobody should write twice.
///
/// The separators are not included, and a run of them contributes nothing, so
/// a trailing `;` does not produce an empty statement.
///
/// ```
/// use pgprox_core::sql::statements;
///
/// assert_eq!(statements("SET a = 1; SET b = 2"), ["SET a = 1", " SET b = 2"]);
///
/// // A semicolon inside a string is data, not a separator.
/// assert_eq!(statements("SET a = 'x; y'"), ["SET a = 'x; y'"]);
/// ```
#[must_use]
pub fn statements(sql: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut lexer = Lexer::new(sql);
    let mut start = 0;

    loop {
        // Taken before the token is consumed, so a separator's own offset is
        // knowable: the lexer only ever moves forward through this one string,
        // so what is left says where it is.
        let remaining = lexer.rest().len();
        match lexer.next() {
            Some(Token::Semicolon) => {
                push_statement(&mut found, &sql[start..sql.len() - remaining]);
                start = sql.len() - lexer.rest().len();
            }
            Some(_) => {}
            None => break,
        }
    }

    push_statement(&mut found, &sql[start..]);
    found
}

/// Adds a statement unless it holds nothing a parser could read.
///
/// Trivia only, which is what a trailing semicolon and the gap between two of
/// them leave behind. Checked with the lexer rather than by trimming, so a
/// statement that is nothing but a comment is dropped for the same reason.
fn push_statement<'a>(found: &mut Vec<&'a str>, text: &'a str) {
    let mut lexer = Lexer::new(text);
    lexer.skip_trivia();
    if lexer.has_more() {
        found.push(text);
    }
}

/// The words of each statement, lowercased, with quoted text left out.
///
/// The shape most callers want: keyword matching that a string literal cannot
/// influence, split on the statement separators the simple query protocol
/// allows.
///
/// `keep_dots` decides whether `pg_catalog.nextval` is one word or two. A
/// caller comparing against qualified parameter names wants one; a caller
/// matching bare keywords wants two, so that a column named `t.insert` does not
/// read as the keyword.
///
/// ```
/// use pgprox_core::sql::statement_words;
///
/// let statements = statement_words("SELECT 1; LISTEN c", false);
/// assert_eq!(statements.len(), 2);
/// assert_eq!(statements[1], vec!["listen", "c"]);
///
/// // A semicolon inside a string does not split anything.
/// assert_eq!(statement_words("SELECT 'a; b'", false).len(), 1);
/// ```
#[must_use]
pub fn statement_words(sql: &str, keep_dots: bool) -> Vec<Vec<String>> {
    let mut statements = Vec::new();
    let mut words: Vec<String> = Vec::new();

    // Set by a `.` when joining is on, so the next word attaches to the
    // previous one rather than standing alone.
    let mut joining = false;

    for token in Lexer::new(sql) {
        match token {
            Token::Semicolon => {
                joining = false;
                if !words.is_empty() {
                    statements.push(std::mem::take(&mut words));
                }
            }
            Token::Word(word) => {
                let word = word.to_ascii_lowercase();
                match words.last_mut() {
                    Some(last) if joining => last.push_str(&word),
                    _ => words.push(word),
                }
                joining = false;
            }
            Token::Punct('.') if keep_dots => {
                if let Some(last) = words.last_mut() {
                    last.push('.');
                    joining = true;
                }
            }
            Token::Quoted | Token::Punct(_) => joining = false,
        }
    }

    if !words.is_empty() {
        statements.push(words);
    }
    statements
}

/// Drops leading whitespace, walking bytes while the input stays ASCII.
///
/// `str::trim_start` asks Unicode about every character. SQL is overwhelmingly
/// ASCII and this runs between every pair of tokens, so the byte loop answers
/// the common case and the general one is still handled: the moment a
/// non-ASCII character appears, this hands over to `trim_start`, which trims
/// exactly what it always did.
fn trim_leading_space(input: &str) -> &str {
    let bytes = input.as_bytes();
    let mut at = 0;
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }

    let rest = input.get(at..).unwrap_or("");
    match rest.as_bytes().first() {
        // Still ASCII, so the byte loop already went as far as it can.
        Some(byte) if byte.is_ascii() => rest,
        // A non-ASCII character, which may or may not be whitespace. Unicode's
        // answer is the one that has always been given here.
        _ => rest.trim_start(),
    }
}

/// Whether a character can appear in a bare word.
///
/// Non-ASCII is included because Postgres identifiers may be, and treating a
/// multi-byte character as punctuation would split one word into two, which
/// could turn a harmless identifier into a keyword match.
/// ASCII is tested first and answered without touching Unicode's
/// general-category tables. The rule is unchanged: for an ASCII character
/// `is_alphanumeric` and `is_ascii_alphanumeric` agree, and a non-ASCII one is
/// a word character whatever its category. It is written this way because this
/// is the innermost loop of the route decision, which runs on every statement:
/// the semantic coverage report counted 3.6 million calls in a twenty-five
/// second replay.
#[must_use]
pub fn is_word_char(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        true
    }
}

/// Whether a word introduces a string literal rather than being one.
///
/// Every introducer is one character, so the length answers most words before
/// any comparison happens. This runs once per word of every statement.
fn is_string_introducer(word: &str) -> bool {
    word.len() == 1
        && ["e", "b", "x", "u"]
            .iter()
            .any(|introducer| word.eq_ignore_ascii_case(introducer))
}

/// How far the word at the start of `input` runs.
///
/// A byte loop while the input is ASCII, handing over to the character scan at
/// the first non-ASCII byte. `str::find` with a character predicate decodes
/// UTF-8 for every character, and SQL is overwhelmingly ASCII. The answer is
/// the same either way: every non-ASCII character is a word character, so the
/// scan only has to be exact about where ASCII punctuation appears.
fn word_end(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if !byte.is_ascii() {
            // Non-ASCII is a word character, and the remainder may hold more
            // of either kind, so the general scan finishes the job.
            return input
                .get(at..)
                .and_then(|rest| rest.find(|c: char| !is_word_char(c)))
                .map_or(input.len(), |offset| at + offset);
        }
        if !(byte.is_ascii_alphanumeric() || byte == b'_') {
            return at;
        }
        at += 1;
    }
    input.len()
}

/// Where a block comment ends, counting from its `/*`.
///
/// Postgres nests them, unlike C. An unterminated comment consumes the rest,
/// which is what the server does with it too.
fn block_comment_end(input: &str) -> usize {
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
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// Where a single-quoted string ends, counting from its opening quote.
///
/// `''` is always an escaped quote. A backslash escapes the next character only
/// in an `E'...'` string, which is why the caller says which it is: treating
/// `'\''` as terminated in a plain string, or as continuing in an E-string,
/// both misplace the end.
fn single_quoted_end(input: &str, backslash_escapes: bool) -> usize {
    let bytes = input.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if backslash_escapes => i += 2,
            b'\'' if bytes.get(i + 1) == Some(&b'\'') => i += 2,
            b'\'' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Where a quoted identifier ends, in which `""` is an escaped quote.
fn double_quoted_end(input: &str) -> usize {
    let bytes = input.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if bytes.get(i + 1) == Some(&b'"') => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Where a dollar-quoted string ends, or [`None`] if this is not one.
///
/// The tag matters twice. It decides where the string ends, since `$$` inside
/// `$body$ ... $body$` is data rather than a delimiter. And it decides whether
/// this is dollar quoting at all: a tag follows the rules for an unquoted
/// identifier, so `$1` is a parameter placeholder and `$1 INSERT $` is not a
/// tag. Accepting that one swallowed the rest of the statement and classified
/// `SELECT $1 INSERT $$` as a replica-eligible read.
fn dollar_quoted_end(input: &str) -> Option<usize> {
    let after = &input[1..];
    let offset = after.find('$')?;
    if !is_dollar_tag(&after[..offset]) {
        return None;
    }

    let tag = &input[..offset + 2];
    let body_at = tag.len();
    Some(
        input[body_at..]
            .find(tag)
            .map_or(input.len(), |i| body_at + i + tag.len()),
    )
}

/// Whether the text between two dollar signs is a valid tag.
///
/// Postgres: a tag follows the rules for an unquoted identifier, except that it
/// cannot contain a dollar sign. Empty is valid, which is `$$`.
fn is_dollar_tag(inner: &str) -> bool {
    let mut chars = inner.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    // A leading digit is what makes `$1` a placeholder rather than a tag.
    (first.is_alphabetic() || first == '_' || !first.is_ascii())
        && chars.all(|c| c.is_alphanumeric() || c == '_' || !c.is_ascii())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    /// `M14.31`. Eighteen mutants survived in this file, the largest group in
    /// `M14`, and it is the crate's most load-bearing pure function: the
    /// statement classifier and the pin detector are both lexical scans over
    /// it. A quote boundary in the wrong place is a write classified as a read,
    /// or a `LISTEN` inside a string literal pinning a session that never asked
    /// for one.
    #[test]
    fn a_statement_is_split_on_a_separator_and_not_on_data() {
        // The split `statement_words` already does internally, exposed for a
        // caller that needs the text rather than the keywords. `M24.1`: the
        // caller that needed it wrote its own and read only the first
        // statement.
        assert_eq!(statements("SET a = 1"), ["SET a = 1"]);
        assert_eq!(
            statements("SET a = 1; SET b = 2"),
            ["SET a = 1", " SET b = 2"]
        );

        // The whole reason this belongs here. A semicolon inside quoted text of
        // any form is data, and a scanner that split on the byte would cut a
        // statement in half.
        assert_eq!(statements("SET a = 'x; y'"), ["SET a = 'x; y'"]);
        assert_eq!(statements(r"SET a = E'x\'; y'"), [r"SET a = E'x\'; y'"]);
        assert_eq!(statements("SET \"a;b\" = 1"), ["SET \"a;b\" = 1"]);
        assert_eq!(
            statements("SELECT $tag$a; b$tag$"),
            ["SELECT $tag$a; b$tag$"]
        );
        assert_eq!(
            statements("SET a = 1 -- ; not a statement"),
            ["SET a = 1 -- ; not a statement"]
        );
    }

    #[test]
    fn a_separator_with_nothing_behind_it_is_not_a_statement() {
        // A trailing semicolon is what every client sends, and an empty
        // statement between two of them would reach a parser as a statement
        // that says nothing.
        assert_eq!(statements("SET a = 1;"), ["SET a = 1"]);
        assert_eq!(statements("SET a = 1;;"), ["SET a = 1"]);
        assert_eq!(
            statements("SET a = 1; ; SET b = 2"),
            ["SET a = 1", " SET b = 2"]
        );
        assert_eq!(statements(";"), Vec::<&str>::new());
        assert_eq!(statements(""), Vec::<&str>::new());
        assert_eq!(statements("   \n\t "), Vec::<&str>::new());

        // Trivia only, decided by the lexer rather than by trimming, so a
        // statement that is nothing but a comment goes the same way.
        assert_eq!(statements("/* nothing */"), Vec::<&str>::new());
        assert_eq!(statements("SET a = 1; -- done"), ["SET a = 1"]);
    }

    #[test]
    fn splitting_never_loses_or_invents_text() {
        // The property that makes the offsets trustworthy: every statement is a
        // slice of the input, in order, and the only bytes dropped are
        // separators and trivia between them.
        for sql in [
            "SET a = 1; SET b = 2; RESET c",
            "SELECT 'a; b'; LISTEN x;",
            "  ;; SET a = 1 ;; ",
        ] {
            let parts = statements(sql);
            let mut at = 0;
            for part in &parts {
                let found = sql[at..].find(part);
                assert!(
                    found.is_some(),
                    "{part:?} is not a slice of {sql:?} at or after {at}"
                );
                at += found.unwrap_or(0) + part.len();
            }
        }
    }

    #[test]
    fn a_word_ends_where_punctuation_starts_and_not_before() {
        // `word_end` has a `!byte.is_ascii()` whose `!` could be deleted, which
        // inverts which half of the input goes down the byte path.
        assert_eq!(word_end("select"), 6);
        assert_eq!(word_end("select 1"), 6);
        assert_eq!(word_end("a_b9,c"), 4);
        assert_eq!(word_end("(x)"), 0, "punctuation is not a word");

        // Non-ASCII is a word character, so an accented identifier is one word
        // and a multi-byte character is not punctuation.
        assert_eq!(word_end("naïve"), "naïve".len());
        assert_eq!(word_end("naïve+1"), "naïve".len());
        assert_eq!(word_end("日本語 x"), "日本語".len());
    }

    #[test]
    fn leading_space_is_trimmed_whether_it_is_ascii_or_not() {
        // `trim_leading_space` matches on `byte.is_ascii()` and both `true` and
        // `false` survived as guards, because no test used a non-ASCII space.
        assert_eq!(trim_leading_space("   select"), "select");
        assert_eq!(trim_leading_space("\t\n select"), "select");
        assert_eq!(trim_leading_space("select"), "select");
        assert_eq!(trim_leading_space(""), "");

        // A non-breaking space is whitespace to Unicode and not to `is_ascii`,
        // so this is the case that separates the two arms.
        assert_eq!(trim_leading_space("\u{a0}select"), "select");
        assert_eq!(trim_leading_space("\u{3000}select"), "select");

        // A non-ASCII character that is *not* whitespace must survive.
        assert_eq!(trim_leading_space(" naïve"), "naïve");
    }

    #[test]
    fn only_the_four_single_letter_introducers_introduce_a_string() {
        // `is_string_introducer` could return `true` for everything, and its
        // `&&` could become `||`, which would make every one-character word a
        // string introducer or every `e`-like word one regardless of length.
        for yes in ["e", "E", "b", "B", "x", "X", "u", "U"] {
            assert!(is_string_introducer(yes), "{yes} should introduce a string");
        }
        for no in ["", "a", "z", "1", "ee", "ex", "select", "exists", "union"] {
            assert!(
                !is_string_introducer(no),
                "{no} should not introduce a string"
            );
        }
    }

    #[test]
    fn a_block_comment_nests_and_ends_where_it_closes() {
        // Postgres nests these, unlike C. `<` becoming `<=` runs the scan one
        // byte past the end.
        assert_eq!(block_comment_end("/* x */rest"), 7);
        assert_eq!(block_comment_end("/* /* x */ y */rest"), 15, "nesting");
        assert_eq!(
            block_comment_end("/* unterminated"),
            15,
            "an unterminated comment consumes the rest, as the server does"
        );
        assert_eq!(block_comment_end("/**/"), 4);
    }

    #[test]
    fn a_single_quoted_string_ends_at_its_closing_quote() {
        // Three mutants here: the doubled-quote guard forced to `false`, and
        // the two `+=` steps. Getting this wrong exposes the rest of the
        // statement as SQL, which is the whole reason the lexer consumes
        // strings rather than skipping to the next quote.
        assert_eq!(single_quoted_end("'abc'rest", false), 5);
        assert_eq!(single_quoted_end("''rest", false), 2, "the empty string");

        // A doubled quote is an escaped quote, not a terminator.
        assert_eq!(single_quoted_end("'a''b'rest", false), 6);

        // With backslash escapes on, a backslash-quote is not a terminator.
        assert_eq!(single_quoted_end(r"'a\'b'rest", true), 6);
        // And with them off, it is.
        assert_eq!(single_quoted_end(r"'a\'b'rest", false), 4);

        assert_eq!(
            single_quoted_end("'unterminated", false),
            13,
            "an unterminated string consumes the rest"
        );
    }

    #[test]
    fn a_quoted_identifier_ends_at_its_closing_quote() {
        // The same three shapes as the single-quoted case, plus a `+` that
        // could become `-` in the doubled-quote lookahead.
        assert_eq!(double_quoted_end(r#""abc"rest"#), 5);
        assert_eq!(double_quoted_end(r#"""rest"#), 2);
        assert_eq!(
            double_quoted_end(r#""a""b"rest"#),
            6,
            "a doubled quote escapes"
        );
        assert_eq!(double_quoted_end(r#""unterminated"#), 13);
    }

    #[test]
    fn a_dollar_tag_follows_the_identifier_rules_and_a_placeholder_does_not() {
        // Four mutants sat here, on the two `!` and the `&&`/`||` between the
        // first-character rule and the rest-of-the-tag rule. `$1` being read as
        // a tag rather than a placeholder would swallow the rest of a statement
        // as a string.
        assert!(is_dollar_tag(""), "$$ is a valid empty tag");
        assert!(is_dollar_tag("tag"));
        assert!(is_dollar_tag("_tag"));
        assert!(is_dollar_tag("t4g"));
        assert!(is_dollar_tag("naïve"), "non-ASCII is allowed in a tag");
        assert!(is_dollar_tag("_"));

        // A leading digit is a placeholder, which is the case the `!` protects.
        assert!(!is_dollar_tag("1"), "$1 is a placeholder");
        assert!(!is_dollar_tag("12"));
        assert!(!is_dollar_tag("1tag"));

        // Punctuation anywhere disqualifies it.
        assert!(!is_dollar_tag("a-b"));
        assert!(!is_dollar_tag("a b"));
        assert!(!is_dollar_tag("a.b"));
    }

    #[test]
    fn a_doubled_quote_advances_by_two_from_wherever_it_is() {
        // `i += 2` could become `i *= 2`, which agrees with addition only when
        // `i` happens to be 2. The first version of these tests put the doubled
        // quote at exactly that offset, so both survived. The escape has to sit
        // somewhere else for the two to differ.
        assert_eq!(single_quoted_end("'ab''c'rest", false), 7);
        assert_eq!(single_quoted_end("'abcd''e'rest", false), 9);
        assert_eq!(double_quoted_end(r#""ab""c"rest"#), 7);
        assert_eq!(double_quoted_end(r#""abcd""e"rest"#), 9);
    }

    #[test]
    fn an_underscore_is_allowed_after_the_first_character_of_a_tag() {
        // `is_dollar_tag`'s trailing rule is
        //   c.is_alphanumeric() || c == '_' || !c.is_ascii()
        // and the second `||` becoming `&&` makes the underscore clause
        // unsatisfiable, since no character is both `_` and non-ASCII. The
        // earlier test only ever put an underscore first, where a different
        // clause handles it.
        assert!(is_dollar_tag("a_b"));
        assert!(is_dollar_tag("tag_1"));
        assert!(is_dollar_tag("a__b"));
    }

    #[test]
    fn a_u_introducer_needs_its_ampersand_and_a_string_without_one_still_closes() {
        // `word.eq_ignore_ascii_case("u") && self.rest.starts_with("&'")`
        // becoming `||` makes any `u` word skip a byte, so `u'abc'` loses its
        // opening quote and the string is never consumed: the rest of the
        // statement is then read as SQL, which is exactly what consuming
        // strings in the lexer exists to prevent.
        let quoted: Vec<Token<'_>> = Lexer::new("u'abc' , 1").collect();
        assert_eq!(quoted.first(), Some(&Token::Quoted));

        // The real `U&'...'` form still works.
        let escaped: Vec<Token<'_>> = Lexer::new("u&'abc' , 1").collect();
        assert_eq!(escaped.first(), Some(&Token::Quoted));

        // And a word that is not `u` followed by `&'` is not an introducer.
        let other: Vec<Token<'_>> = Lexer::new("e&'abc'").collect();
        assert_eq!(other.first(), Some(&Token::Word("e")));
    }

    #[test]
    fn trivia_is_skipped_in_any_order_and_any_amount() {
        // `skip_trivia`'s `+` could become `*` in the line-comment end, and the
        // lexer's own `&&` could become `||`. Both need trivia that repeats and
        // mixes kinds to tell apart.
        let mut lexer = Lexer::new("  -- one\n /* two */ \n--three\n  select 1");
        assert_eq!(lexer.next(), Some(Token::Word("select")));

        // A line comment with no trailing newline ends the input.
        let mut only_comment = Lexer::new("-- nothing after this");
        assert_eq!(only_comment.next(), None);

        // A block comment at the very end, likewise.
        let mut trailing = Lexer::new("select /* end");
        assert_eq!(trailing.next(), Some(Token::Word("select")));
        assert_eq!(trailing.next(), None);
    }

    fn words(sql: &str) -> Vec<String> {
        Lexer::new(sql)
            .filter_map(|token| match token {
                Token::Word(word) => Some(word.to_ascii_lowercase()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn bare_words_come_out_as_words() {
        assert_eq!(words("SELECT a FROM t"), ["select", "a", "from", "t"]);
        assert_eq!(words("  \n select\t1 "), ["select", "1"]);
    }

    #[test]
    fn quoted_text_is_never_a_word() {
        // Otherwise a tenant's own data changes how their queries are treated.
        assert_eq!(words("SELECT 'insert'"), ["select"]);
        assert_eq!(words(r#"SELECT "insert""#), ["select"]);
        assert_eq!(words("SELECT $$ insert $$"), ["select"]);
    }

    #[test]
    fn comments_are_skipped_and_block_comments_nest() {
        assert_eq!(words("-- insert\nSELECT 1"), ["select", "1"]);
        assert_eq!(words("/* insert */ SELECT 1"), ["select", "1"]);
        assert_eq!(
            words("/* outer /* inner */ insert */ SELECT 1"),
            ["select", "1"],
            "a nested comment ended early and exposed its contents"
        );
        assert_eq!(words("SELECT 1 -- insert"), ["select", "1"]);
    }

    #[test]
    fn a_doubled_quote_does_not_end_a_string() {
        assert_eq!(words("SELECT 'it''s' , x"), ["select", "x"]);
        assert_eq!(words(r#"SELECT "a""b" , x"#), ["select", "x"]);
    }

    #[test]
    fn a_backslash_escape_is_honoured_only_in_an_e_string() {
        // The divergence this module exists to end. pgprox-pool's own scanner
        // ended `E'\''` at the escaped quote, read the rest as data, and left a
        // LISTEN unseen. A missed pin hands one client another client's state.
        assert_eq!(
            words(r"SELECT E'\'' ; LISTEN c"),
            ["select", "listen", "c"],
            "the statement after an E-string was lost"
        );
        assert_eq!(
            words(r"SELECT E'\' ; LISTEN c'"),
            ["select"],
            "an E-string's contents were read as SQL"
        );
        // Without the E, a backslash is an ordinary character.
        assert_eq!(words(r"SELECT '\' , x"), ["select", "x"]);
    }

    #[test]
    fn the_other_string_introducers_are_recognised() {
        for sql in ["SELECT B'101'", "SELECT X'1f'", "SELECT U&'a'"] {
            assert_eq!(words(sql), ["select"], "{sql}");
        }
    }

    #[test]
    fn an_invalid_dollar_tag_is_not_dollar_quoting() {
        // Accepting `$1 insert $` as a tag swallowed the rest of the statement
        // and classified SELECT $1 INSERT $$ as a replica-eligible read.
        assert!(words("SELECT $1 INSERT $$").contains(&"insert".to_owned()));
        assert_eq!(words("SELECT $1"), ["select", "1"]);
    }

    #[test]
    fn a_valid_dollar_tag_is_dollar_quoting() {
        for sql in [
            "SELECT $$ insert $$",
            "SELECT $body$ insert $body$",
            "SELECT $_x9$ insert $_x9$",
        ] {
            assert_eq!(words(sql), ["select"], "{sql}");
        }
    }

    #[test]
    fn a_tagged_string_is_not_ended_by_a_different_tag() {
        assert_eq!(
            words("SELECT $body$ a $$ insert $body$ , x"),
            ["select", "x"]
        );
    }

    #[test]
    fn statements_split_on_semicolons_outside_strings() {
        assert_eq!(
            statement_words("SELECT 1; LISTEN c", false),
            vec![vec!["select", "1"], vec!["listen", "c"]]
        );
        assert_eq!(
            statement_words("SELECT 'a; b'", false),
            vec![vec!["select"]],
            "a semicolon inside a string split a statement"
        );
        assert_eq!(
            statement_words("SELECT 'a; b'; LISTEN c", false).len(),
            2,
            "a real second statement was swallowed by a string"
        );
    }

    #[test]
    fn empty_statements_are_dropped_rather_than_reported() {
        assert_eq!(statement_words("SELECT 1;", false).len(), 1);
        assert_eq!(statement_words(";;;", false).len(), 0);
        assert_eq!(statement_words("", false).len(), 0);
        assert_eq!(statement_words("   ", false).len(), 0);
    }

    #[test]
    fn qualified_names_join_only_when_asked() {
        // A caller comparing against `pgprox.route` wants one word. A caller
        // matching bare keywords wants two, so a column named `t.insert` does
        // not read as the keyword.
        assert_eq!(
            statement_words("SET pgprox.route = x", true),
            vec![vec!["set", "pgprox.route", "x"]]
        );
        assert_eq!(
            statement_words("SET pgprox.route = x", false),
            vec![vec!["set", "pgprox", "route", "x"]]
        );
    }

    #[test]
    fn a_leading_dot_does_not_panic() {
        // There is no word to attach it to.
        assert_eq!(statement_words(".a", true), vec![vec!["a"]]);
    }

    #[test]
    fn an_unterminated_construct_terminates() {
        // These arrive from the internet. The server rejects them; the lexer
        // must reach the end rather than looping.
        for sql in [
            "SELECT 'unterminated",
            "SELECT \"unterminated",
            "SELECT $$unterminated",
            "SELECT $tag$unterminated",
            "/* unterminated",
            "SELECT E'\\",
            "$",
            "$$",
            "';',",
            "--",
        ] {
            let count = Lexer::new(sql).count();
            assert!(count < 100, "{sql:?} produced {count} tokens");
        }
    }

    #[test]
    fn punctuation_is_reported_rather_than_dropped() {
        // `(` matters to a caller looking for a function call, and `.` to one
        // rejoining a qualified name.
        let tokens: Vec<Token<'_>> = Lexer::new("f(a)").collect();
        assert_eq!(
            tokens,
            vec![
                Token::Word("f"),
                Token::Punct('('),
                Token::Word("a"),
                Token::Punct(')'),
            ]
        );
    }

    #[test]
    fn a_placeholder_is_punctuation_rather_than_a_quote() {
        let tokens: Vec<Token<'_>> = Lexer::new("$1").collect();
        assert_eq!(tokens, vec![Token::Punct('$'), Token::Word("1")]);
    }

    #[test]
    fn trivia_can_be_walked_one_comment_at_a_time() {
        // What a caller reading a leading hint comment needs: the comments in
        // order, rather than silently dropped.
        let mut lexer = Lexer::new("  /* first */ /* second */ SELECT 1");
        lexer.skip_trivia();
        assert!(lexer.rest().starts_with("SELECT"));
        assert!(lexer.has_more());

        let mut empty = Lexer::new("   ");
        empty.skip_trivia();
        assert!(!empty.has_more());
    }

    #[test]
    fn non_ascii_identifiers_stay_whole() {
        // Splitting one at a multi-byte character could turn a harmless
        // identifier into a keyword match.
        assert_eq!(words("SELECT café FROM t"), ["select", "café", "from", "t"]);
        assert!(is_word_char('é'));
        assert!(!is_word_char('('));
    }
}
