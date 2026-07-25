//! Properties that run on stable, covering what the fuzz targets cannot here.
//!
//! `fuzz/fuzz_targets/classify.rs` asserts the same invariant under
//! coverage-guided fuzzing, but libFuzzer needs a nightly toolchain that is not
//! installed on this machine. This file runs the identical oracle under
//! proptest so the property is actually executed rather than merely written
//! down. See `fuzz/README.md`.
//!
//! The oracle below is duplicated in the fuzz target on purpose. Sharing it
//! would mean exporting it from the crate as public API, and a testing oracle
//! is not part of what `pgprox-route` offers its callers.

use pgprox_core::route::StmtClass;
use pgprox_route::{begins_read_only_transaction, classify, statement_hint};
use proptest::prelude::*;

/// The keywords whose presence means a statement is not a plain read.
const DML: &[&str] = &["insert", "update", "delete", "merge", "truncate"];

/// Whether `sql` contains a DML keyword outside a region where text is not SQL.
///
/// The oracle. It knows two things: which regions are data or commentary, and
/// which words modify data. It knows nothing about the first-word allowlist,
/// the denylist, statement splitting, writing functions, or read-only
/// transactions, which is what makes disagreement with `classify` informative
/// rather than tautological.
///
/// Skipping regions is unavoidable. An oracle that read a comment's contents as
/// SQL would flag `SELECT -- INSERT`, which is a perfectly ordinary read, and a
/// stream of false findings is how a differential test gets switched off.
fn mentions_dml(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    let mut word_start: Option<usize> = None;
    let mut i = 0;

    // Ends the word in progress, reporting whether it was DML.
    let flush = |word_start: &mut Option<usize>, end: usize| -> bool {
        word_start
            .take()
            .is_some_and(|start| DML.contains(&&lower[start..end]))
    };

    while i < bytes.len() {
        // A region boundary always ends the word before it.
        let region = if bytes[i..].starts_with(b"--") {
            Some(
                lower[i..]
                    .find('\n')
                    .map_or(bytes.len(), |offset| i + offset + 1),
            )
        } else if bytes[i..].starts_with(b"/*") {
            // Nested, as Postgres nests them.
            let mut depth = 0_u32;
            let mut j = i;
            let mut end = bytes.len();
            while j < bytes.len() {
                if bytes[j..].starts_with(b"/*") {
                    depth += 1;
                    j += 2;
                } else if bytes[j..].starts_with(b"*/") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        end = j;
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            Some(end)
        } else if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let mut j = i + 1;
            let mut end = bytes.len();
            while j < bytes.len() {
                if bytes[j] == quote {
                    // A doubled quote is an escaped one and the string goes on.
                    if bytes.get(j + 1) == Some(&quote) {
                        j += 2;
                        continue;
                    }
                    end = j + 1;
                    break;
                }
                j += 1;
            }
            Some(end)
        } else if bytes[i] == b'$' {
            // `$tag$ ... $tag$`, where the tag may be empty but not numeric.
            lower[i + 1..].find('$').and_then(|offset| {
                let tag = &lower[i..=i + 1 + offset];
                let inner = &tag[1..tag.len() - 1];
                if inner.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    // `$1` is a placeholder.
                    return None;
                }
                let body_at = i + tag.len();
                Some(
                    lower[body_at..]
                        .find(tag)
                        .map_or(bytes.len(), |offset| body_at + offset + tag.len()),
                )
            })
        } else {
            None
        };

        if let Some(end) = region {
            if flush(&mut word_start, i) {
                return true;
            }
            i = end.max(i + 1);
            continue;
        }

        let is_word = bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_';
        if is_word {
            if word_start.is_none() {
                word_start = Some(i);
            }
        } else if flush(&mut word_start, i) {
            return true;
        }
        i += 1;
    }

    flush(&mut word_start, bytes.len())
}

/// SQL-shaped text, so the generator spends its time near the interesting
/// inputs rather than in arbitrary Unicode.
fn sql_like() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("SELECT".to_owned()),
        Just("WITH".to_owned()),
        Just("EXPLAIN".to_owned()),
        Just("INSERT".to_owned()),
        Just("UPDATE".to_owned()),
        Just("DELETE".to_owned()),
        Just("MERGE".to_owned()),
        Just("TRUNCATE".to_owned()),
        Just("FROM".to_owned()),
        Just("INTO".to_owned()),
        Just("FOR".to_owned()),
        Just("SHARE".to_owned()),
        Just("AS".to_owned()),
        Just("*".to_owned()),
        Just("(".to_owned()),
        Just(")".to_owned()),
        Just(";".to_owned()),
        Just("'".to_owned()),
        Just("''".to_owned()),
        Just("\"".to_owned()),
        Just("$$".to_owned()),
        Just("$tag$".to_owned()),
        Just("$1".to_owned()),
        Just("--".to_owned()),
        Just("/*".to_owned()),
        Just("*/".to_owned()),
        Just("\n".to_owned()),
        Just("/* pgprox:replica */".to_owned()),
        Just("t".to_owned()),
        Just("x".to_owned()),
    ];
    proptest::collection::vec(fragment, 0..24).prop_map(|parts| parts.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_000))]

    /// The invariant, checked against an independent and much dumber scan.
    ///
    /// A read-only verdict is a promise the statement is safe on a replica, and
    /// a statement mentioning DML outside a string is not.
    #[test]
    fn a_read_only_verdict_never_covers_a_dml_keyword(sql in sql_like()) {
        if classify(&sql) == StmtClass::ReadOnly {
            prop_assert!(
                !mentions_dml(&sql),
                "classified read-only despite a DML keyword: {:?}",
                sql
            );
        }
    }

    /// The same, over arbitrary text rather than SQL-shaped text.
    #[test]
    fn the_invariant_holds_over_arbitrary_text(sql in ".{0,300}") {
        if classify(&sql) == StmtClass::ReadOnly {
            prop_assert!(
                !mentions_dml(&sql),
                "classified read-only despite a DML keyword: {:?}",
                sql
            );
        }
    }

    /// Nothing in the crate's parsing surface may panic on hostile input. All
    /// three read text that arrives from the internet.
    #[test]
    fn no_parser_panics_on_arbitrary_input(sql in ".{0,300}") {
        let _ = classify(&sql);
        let _ = statement_hint(&sql);
        let _ = begins_read_only_transaction(&sql);
    }

    /// Truncating a statement must not turn it into a read. A short read is a
    /// syntax error; a short write that reaches a replica is a bug.
    #[test]
    fn truncating_a_write_never_makes_it_read_only(
        sql in sql_like(),
        cut in 0usize..300,
    ) {
        let truncated: String = sql.chars().take(cut).collect();
        if classify(&truncated) == StmtClass::ReadOnly {
            prop_assert!(
                !mentions_dml(&truncated),
                "a truncated statement became a replica-eligible read: {:?}",
                truncated
            );
        }
    }
}
