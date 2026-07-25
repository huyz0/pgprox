//! Credential wrapper.
//!
//! The proxy holds credentials for every tenant database on the fleet, so one
//! leaked log line is a fleet-wide incident. [`SecretString`] makes the safe
//! path the default one: it cannot be printed, and reaching the real value
//! takes an explicit [`SecretString::expose`] call that is greppable and is a
//! review item at every call site.

use std::fmt;

use zeroize::Zeroizing;

/// What redacted secrets render as, in both `Debug` and `Display`.
const REDACTED: &str = "[redacted]";

/// A string that will not be printed and is zeroed when dropped.
///
/// Deliberately missing: `PartialEq`, `Deref`, `AsRef<str>`, and any `From`
/// conversion back to `String`. Each would provide a route to the value that
/// does not show up when grepping for `expose`, which is the whole mechanism.
///
/// ```
/// use pgprox_core::SecretString;
/// let s = SecretString::new("hunter2");
/// assert_eq!(format!("{s:?}"), "[redacted]");
/// assert_eq!(s.expose(), "hunter2");
/// ```
///
/// Printing it never reveals anything, whichever formatter is used:
///
/// ```
/// use pgprox_core::SecretString;
/// let s = SecretString::new("hunter2");
/// assert!(!format!("{s} {s:?} {s:#?}").contains("hunter2"));
/// ```
#[derive(Clone)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Wraps a credential.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Borrows the real value.
    ///
    /// Every call site is a review item. Do not use the result to build a
    /// string that outlives it, and never pass it to a formatter that reaches
    /// a log, a span attribute, a metric label, or an error variant.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Length of the secret, which is safe to expose and useful in tests.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the secret is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "correct-horse-battery-staple";

    #[test]
    fn debug_reveals_nothing() {
        let s = SecretString::new(SECRET);
        assert_eq!(format!("{s:?}"), REDACTED);
        assert!(!format!("{s:?}").contains(SECRET));
    }

    #[test]
    fn display_reveals_nothing() {
        let s = SecretString::new(SECRET);
        assert_eq!(format!("{s}"), REDACTED);
    }

    #[test]
    fn alternate_and_padded_formatting_reveal_nothing() {
        // A formatter flag must not open a route around the redaction. `{:#?}`
        // in particular is what a derived Debug on an enclosing struct emits.
        let s = SecretString::new(SECRET);
        for rendered in [
            format!("{s:#?}"),
            format!("{s:>40}"),
            format!("{s:.3}"),
            format!("{s:^50?}"),
        ] {
            assert!(!rendered.contains(SECRET), "leaked in {rendered:?}");
        }
    }

    #[test]
    fn nested_in_a_derived_debug_reveals_nothing() {
        // The realistic leak: a struct derives Debug and someone logs it.
        #[derive(Debug)]
        struct Holder {
            user: &'static str,
            password: SecretString,
        }
        let h = Holder {
            user: "app",
            password: SecretString::new(SECRET),
        };
        let rendered = format!("{h:#?}");
        assert!(!rendered.contains(SECRET), "leaked in {rendered}");
        assert!(rendered.contains("app"), "non-secret fields still print");
        assert_eq!(h.user, "app");
        assert_eq!(h.password.len(), SECRET.len());
    }

    #[test]
    fn expose_returns_the_value() {
        assert_eq!(SecretString::new(SECRET).expose(), SECRET);
    }

    #[test]
    fn length_is_available_without_exposing() {
        let s = SecretString::new(SECRET);
        assert_eq!(s.len(), SECRET.len());
        assert!(!s.is_empty());
        assert!(SecretString::new("").is_empty());
    }

    #[test]
    fn conversions_wrap_rather_than_reveal() {
        let from_str: SecretString = SECRET.into();
        let from_string: SecretString = String::from(SECRET).into();
        assert_eq!(from_str.expose(), SECRET);
        assert_eq!(from_string.expose(), SECRET);
        assert_eq!(format!("{from_str:?}"), REDACTED);
    }

    #[test]
    fn clone_is_independent_and_still_redacted() {
        let a = SecretString::new(SECRET);
        let b = a.clone();
        assert_eq!(b.expose(), SECRET);
        assert_eq!(format!("{b:?}"), REDACTED);
        drop(a);
        // Dropping the original must not disturb the clone's buffer.
        assert_eq!(b.expose(), SECRET);
    }
}
