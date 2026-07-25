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

/// Whether `sql` contains a DML keyword outside anything quoted.
///
/// Deliberately crude and deliberately over-eager: it treats every quote as
/// opening a region it skips to the next matching quote, with no escape
/// handling at all. Over-eager skipping means it can only ever report *fewer*
/// keywords than are really there, so when it says "no DML" while `classify`
/// says "read only", the two agreeing is weak evidence and the two disagreeing
/// is a real finding.
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
