//! Shared scaffolding for integration tests.
//!
//! # Why this crate exists
//!
//! A Postgres container accepts TCP, completes a TLS handshake, and answers a
//! startup packet well before its databases exist. During that window it replies
//! `ErrorResponse` with SQLSTATE `57P03`, the database system is starting up.
//!
//! A readiness probe that accepts *any* reply therefore reports ready too early.
//! That bug shipped once in the M1.11 Postgres probe, where it made the suite
//! pass against Postgres 17 and fail against 18 purely on timing, and then it
//! was written again from scratch in the SCRAM tests. Two independent
//! reproductions is the evidence that it belongs in one place.
//!
//! Sleeping a fixed amount instead is the other version of the same bug: it
//! passes on an idle machine and fails under load.

/// The SQLSTATE a Postgres sends while it is still starting.
///
/// Seeing this means "not ready yet", never "broken".
pub const STARTING_UP: &str = "57P03";

/// What a probe made of a server's reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Readiness {
    /// The server answered as a working server would.
    Ready,
    /// The server answered, but it is still coming up. Retry.
    NotYet,
    /// The server said something a probe should not paper over.
    Failed,
}

/// Classifies the first byte of a reply to a startup packet.
///
/// `R` is an `Authentication` message, which only a server past startup sends.
/// `E` is an `ErrorResponse`: during startup it carries `57P03`, and otherwise it
/// means something genuinely wrong, so the body decides.
///
/// Anything else is treated as not-ready rather than failed, because a partial
/// read during startup is common and retrying costs nothing.
#[must_use]
pub fn classify_startup_reply(tag: u8, body: &[u8]) -> Readiness {
    match tag {
        b'R' => Readiness::Ready,
        b'E' => {
            // The body is a run of type-tagged strings; `C` carries the code.
            if find_error_code(body).is_some_and(|code| code == STARTING_UP) {
                Readiness::NotYet
            } else {
                Readiness::Failed
            }
        }
        _ => Readiness::NotYet,
    }
}

/// Extracts the `C` field from an `ErrorResponse` body.
fn find_error_code(body: &[u8]) -> Option<&str> {
    let mut rest = body;
    while let Some((&kind, tail)) = rest.split_first() {
        if kind == 0 {
            return None;
        }
        let end = tail.iter().position(|b| *b == 0)?;
        if kind == b'C' {
            return std::str::from_utf8(&tail[..end]).ok();
        }
        rest = &tail[end + 1..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an `ErrorResponse` body carrying `code`.
    fn error_body(code: &str) -> Vec<u8> {
        let mut body = vec![b'S'];
        body.extend_from_slice(b"FATAL\0");
        body.push(b'C');
        body.extend_from_slice(code.as_bytes());
        body.push(0);
        body.push(b'M');
        body.extend_from_slice(b"a message\0");
        body.push(0);
        body
    }

    #[test]
    fn an_authentication_message_means_ready() {
        assert_eq!(classify_startup_reply(b'R', &[0; 4]), Readiness::Ready);
    }

    #[test]
    fn the_starting_up_error_means_retry_not_failure() {
        // The exact bug this crate exists to stop recurring. A probe treating
        // this as ready passes on a fast machine and fails under load; one
        // treating it as failure gives up on a container that was about to
        // work.
        assert_eq!(
            classify_startup_reply(b'E', &error_body(STARTING_UP)),
            Readiness::NotYet
        );
    }

    #[test]
    fn a_real_error_is_not_papered_over() {
        // Retrying forever on a genuine failure turns a clear error into a
        // timeout, which is a worse thing to debug.
        for code in ["28000", "3D000", "53300"] {
            assert_eq!(
                classify_startup_reply(b'E', &error_body(code)),
                Readiness::Failed,
                "{code} was treated as transient"
            );
        }
    }

    #[test]
    fn an_error_with_no_code_is_a_failure() {
        let mut body = vec![b'M'];
        body.extend_from_slice(b"no code here\0");
        body.push(0);
        assert_eq!(classify_startup_reply(b'E', &body), Readiness::Failed);
    }

    #[test]
    fn an_unexpected_tag_is_retried_rather_than_failed() {
        // A partial or surprising read during startup is common, and retrying
        // costs nothing.
        for tag in [b'Z', b'S', b'K', 0_u8] {
            assert_eq!(classify_startup_reply(tag, &[]), Readiness::NotYet);
        }
    }

    #[test]
    fn a_truncated_error_body_does_not_panic() {
        // Probe input comes off a socket mid-startup, so it is routinely
        // incomplete.
        for len in 0..12 {
            let body = vec![b'C'; len];
            let _ = classify_startup_reply(b'E', &body);
        }
        assert_eq!(classify_startup_reply(b'E', b"C"), Readiness::Failed);
    }
}
