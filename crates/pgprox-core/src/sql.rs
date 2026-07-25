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
                let end = self
                    .rest
                    .find(|c: char| !is_word_char(c))
                    .unwrap_or(self.rest.len());
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

/// Whether a character can appear in a bare word.
///
/// Non-ASCII is included because Postgres identifiers may be, and treating a
/// multi-byte character as punctuation would split one word into two, which
/// could turn a harmless identifier into a keyword match.
#[must_use]
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || !c.is_ascii()
}

/// Whether a word introduces a string literal rather than being one.
fn is_string_introducer(word: &str) -> bool {
    ["e", "b", "x", "u"]
        .iter()
        .any(|introducer| word.eq_ignore_ascii_case(introducer))
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
