//! Changing the statement name in a frame, and nothing else.
//!
//! # Why rewriting rather than re-encoding
//!
//! A `Bind` carries the client's parameter values, which may be megabytes of
//! them and which this proxy has no reason to parse. Decoding one to encode it
//! again would mean understanding every parameter format to reproduce bytes
//! that were already correct. The name is a prefix, so the rest is copied.
//!
//! # Why this is here and not in `pgprox-pool`
//!
//! ADR 0011, as amended by M5.1: the pool owns the *mapping*, which is a data
//! structure over strings and hashes, and this crate owns the *rewriting*,
//! which is protocol. `pgprox-session` joins them, which is what a composer is
//! for.
//!
//! # What a failure means
//!
//! `None`, and the caller refuses the client. A frame whose name field cannot
//! be found is one this proxy did not understand, and forwarding it unchanged
//! would send the client's private statement name to a server that has never
//! seen it.

/// Replaces the statement name at the start of a `Parse` body.
///
/// The body is `statement\0 query\0 ...`, so the name is the first field.
#[must_use]
pub fn parse_statement(body: &[u8], name: &str) -> Option<Vec<u8>> {
    replace_cstring(body, 0, name)
}

/// Replaces the statement name in a `Bind` body.
///
/// The body is `portal\0 statement\0 ...`, so the name is the second field and
/// the portal is the client's to keep.
#[must_use]
pub fn bind_statement(body: &[u8], name: &str) -> Option<Vec<u8>> {
    let portal_end = memchr::memchr(0, body)? + 1;
    replace_cstring(body, portal_end, name)
}

/// Replaces the name in a `Describe` or `Close` body.
///
/// The body is a kind byte and then the name. Returns `None` for kind `P`: a
/// portal is the client's own name for a result set and this proxy does not
/// rename it, so a caller that rewrote one would be renaming the wrong thing.
#[must_use]
pub fn described_statement(body: &[u8], name: &str) -> Option<Vec<u8>> {
    if body.first()? != &b'S' {
        return None;
    }
    replace_cstring(body, 1, name)
}

/// Whether a `Describe` or `Close` names a statement rather than a portal.
#[must_use]
pub fn describes_statement(body: &[u8]) -> bool {
    body.first() == Some(&b'S')
}

/// Replaces the null-terminated string starting at `at`, keeping the rest.
fn replace_cstring(body: &[u8], at: usize, name: &str) -> Option<Vec<u8>> {
    // A name with a null in it cannot be encoded, and one that arrived that
    // way is a client doing something the protocol does not allow.
    if memchr::memchr(0, name.as_bytes()).is_some() {
        return None;
    }

    let rest = body.get(at..)?;
    let end = at + memchr::memchr(0, rest)? + 1;

    let mut out = Vec::with_capacity(body.len() + name.len());
    out.extend_from_slice(&body[..at]);
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    out.extend_from_slice(body.get(end..)?);
    Some(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn body(fields: &[&str], trailing: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for field in fields {
            out.extend_from_slice(field.as_bytes());
            out.push(0);
        }
        out.extend_from_slice(trailing);
        out
    }

    #[test]
    fn a_parse_keeps_its_query_and_everything_after_it() {
        // The query and the parameter types are the client's, and this
        // function's whole job is to leave them alone.
        let original = body(&["s1", "SELECT $1"], &[0, 1, 0, 0, 0, 23]);
        let rewritten = parse_statement(&original, "pgprox_1234").unwrap();

        assert_eq!(
            rewritten,
            body(&["pgprox_1234", "SELECT $1"], &[0, 1, 0, 0, 0, 23])
        );
    }

    #[test]
    fn a_bind_keeps_its_portal_and_its_parameters() {
        // The portal is the client's own name for a result set. Renaming it
        // would break the `Execute` that follows.
        let original = body(&["my_portal", "s1"], &[0, 0, 0, 1, 255, 255, 255, 255]);
        let rewritten = bind_statement(&original, "pgprox_1234").unwrap();

        assert_eq!(
            rewritten,
            body(
                &["my_portal", "pgprox_1234"],
                &[0, 0, 0, 1, 255, 255, 255, 255]
            )
        );
    }

    #[test]
    fn an_unnamed_statement_is_rewritten_like_any_other() {
        // The empty name is the one every driver uses for a one-shot, and it
        // is a name like any other to this function.
        let original = body(&["", "SELECT 1"], &[0, 0]);
        let rewritten = parse_statement(&original, "pgprox_9").unwrap();

        assert_eq!(rewritten, body(&["pgprox_9", "SELECT 1"], &[0, 0]));
    }

    #[test]
    fn a_describe_of_a_portal_is_refused() {
        // A portal is not a statement, and renaming one would rename the wrong
        // thing rather than fail visibly.
        let statement = body(&["s1"], &[]);
        let mut portal = vec![b'P'];
        portal.extend_from_slice(&statement);
        let mut described = vec![b'S'];
        described.extend_from_slice(&statement);

        assert!(described_statement(&portal, "pgprox_1").is_none());
        assert!(described_statement(&described, "pgprox_1").is_some());
        assert!(describes_statement(&described));
        assert!(!describes_statement(&portal));
    }

    #[test]
    fn a_body_with_no_terminator_is_refused_rather_than_panicking() {
        // These bytes come from the network like any others.
        assert!(parse_statement(b"no terminator here", "x").is_none());
        assert!(bind_statement(b"portal\0no terminator", "x").is_none());
        assert!(described_statement(b"S", "x").is_none());
        assert!(described_statement(&[], "x").is_none());
    }

    #[test]
    fn a_name_carrying_a_null_is_refused() {
        // It could not be encoded, and a name that arrived with one is a
        // client doing something the protocol does not allow.
        let original = body(&["s1", "SELECT 1"], &[]);
        assert!(parse_statement(&original, "bad\0name").is_none());
    }

    #[test]
    fn rewriting_is_reversible_in_the_sense_that_matters() {
        // The bytes after the name are byte-identical, which is the property
        // that lets a Bind carry megabytes of parameters through untouched.
        let parameters: Vec<u8> = (0..=255).collect();
        let original = body(&["p", "s1"], &parameters);
        let rewritten = bind_statement(&original, "pgprox_abc").unwrap();

        assert!(rewritten.ends_with(&parameters));
    }
}
