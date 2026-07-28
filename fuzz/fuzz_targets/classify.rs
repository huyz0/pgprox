#![no_main]
//! Statement classification against arbitrary text.
//!
//! The classifier reads SQL that arrives from the internet, so a panic here is
//! a denial of service and a hang is worse.
//!
//! # The invariant is asserted inside the target
//!
//! Not just "does it panic". The property that matters is that a statement
//! bearing DML never classifies read-only, so this checks it on every input:
//! if the classifier says a statement is safe for a replica, a second and much
//! dumber scan must agree there is no DML keyword in it outside a quoted
//! region. Two implementations that must agree, one of them written to be
//! obviously right rather than fast.
//!
//! Checking only for panics would let the fuzzer explore millions of inputs
//! while ignoring the one thing that would actually hurt.

use libfuzzer_sys::fuzz_target;
use pgprox_core::route::StmtClass;
use pgprox_route::{begins_read_only_transaction, classify, statement_hint};

/// The keywords whose presence means a statement is not a plain read.
const DML: &[&str] = &["insert", "update", "delete", "merge", "truncate"];

/// Whether `sql` contains a DML keyword outside anything quoted or commented.
///
/// Deliberately crude and deliberately over-eager: every quote opens a region
/// it skips to the next matching quote, `--` skips to the end of the line and
/// `/*` to the next `*/`, all with no escape handling and no nesting.
/// Over-eager skipping means it can only ever report *fewer* keywords than are
/// really there, so the two agreeing is weak evidence and the two disagreeing
/// is a real finding.
///
/// The comments were not here on the first run of this target, and the first
/// thing it found was `---kk...update;...` calling `classify` wrong. It was
/// not wrong: `--` had opened a comment and the keyword was inside it. An
/// oracle looser than the thing it checks reports the checker's correctness as
/// a bug, which is worse than no oracle.
fn mentions_dml(sql: &str) -> bool {
    let lower = sql.to_ascii_lowercase();
    let bytes = lower.as_bytes();

    let mut word_start = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];

        if c == b'\'' || c == b'"' {
            // Skip to the closing quote, or to the end.
            word_start = None;
            i += 1;
            while i < bytes.len() && bytes[i] != c {
                i += 1;
            }
            i += 1;
            continue;
        }

        // A line comment, which runs to the newline or to the end.
        if c == b'-' && bytes.get(i + 1) == Some(&b'-') {
            word_start = None;
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // A block comment. Postgres nests these, unlike C, and so does this:
        // a version that stopped at the first `*/` would think the comment had
        // closed and read the rest of it as code. That is the second thing
        // this target found, and it fired on `/* /* merge */ */`, where the
        // classifier was right and the oracle was not.
        //
        // The direction matters and the first attempt had it backwards.
        // Skipping *less* than the real scanner means seeing more keywords
        // than are really there, which reports a correct classifier as broken.
        // The oracle has to skip at least as much as the thing it checks.
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            word_start = None;
            let mut depth = 0_u32;
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
            continue;
        }

        let is_word = c.is_ascii_alphanumeric() || c == b'_';
        match (is_word, word_start) {
            (true, None) => word_start = Some(i),
            (false, Some(start)) => {
                if DML.contains(&&lower[start..i]) {
                    return true;
                }
                word_start = None;
            }
            _ => {}
        }
        i += 1;
    }

    if let Some(start) = word_start {
        if DML.contains(&&lower[start..]) {
            return true;
        }
    }
    false
}

fuzz_target!(|data: &[u8]| {
    let Ok(sql) = std::str::from_utf8(data) else {
        return;
    };

    // None of these may panic on any input.
    let class = classify(sql);
    let _ = statement_hint(sql);
    let _ = begins_read_only_transaction(sql);

    // The property. A read-only verdict is a promise the statement is safe on a
    // replica, and a statement mentioning DML outside a string is not.
    if class == StmtClass::ReadOnly {
        assert!(
            !mentions_dml(sql),
            "classified read-only despite a DML keyword: {sql:?}"
        );
    }
});
