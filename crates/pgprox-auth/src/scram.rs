//! SCRAM-SHA-256, per RFC 5802 and RFC 7677.
//!
//! ADR 0002 chose SCRAM for clients that cannot carry a JWT: admin tooling,
//! migrations, monitoring. ADR 0014 declines `SCRAM-SHA-256-PLUS`, because
//! binding to a terminating proxy's own TLS channel would report a property the
//! client does not actually have.
//!
//! Sans-I/O: every function here maps bytes to bytes. The framing is
//! `pgprox-proto`'s job and the socket is nobody's job in this crate.
//!
//! # Crypto provider
//!
//! HMAC, PBKDF2 and SHA-256 come from `aws-lc-rs`, the same provider
//! `pgprox-tls` uses, so a FIPS build has one validated module rather than two
//! crypto stacks to reason about.

use std::num::NonZeroU32;

use aws_lc_rs::{digest, hmac, pbkdf2};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use subtle::ConstantTimeEq;

/// Length of a SHA-256 digest, and therefore of every SCRAM key.
pub const KEY_LEN: usize = 32;

/// Bytes of randomness in a generated nonce, before base64.
///
/// RFC 5802 requires only that a nonce be unpredictable and unique per
/// exchange. 24 bytes gives 192 bits, comfortably past the point where birthday
/// collisions matter across a fleet.
const NONCE_BYTES: usize = 24;

/// Why a SCRAM exchange failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScramError {
    /// A message did not have the shape RFC 5802 defines.
    #[error("malformed SCRAM message: {reason}")]
    Malformed {
        /// What was wrong.
        reason: String,
    },
    /// A base64 field did not decode.
    #[error("SCRAM field {field} is not valid base64")]
    NotBase64 {
        /// Which field.
        field: &'static str,
    },
    /// The server's nonce did not extend ours.
    ///
    /// This is a replay defence, not a formatting check: a server that does not
    /// echo our nonce is not answering our exchange.
    #[error("server nonce does not extend the client nonce")]
    NonceMismatch,
    /// The iteration count was absent, zero, or absurd.
    #[error("SCRAM iteration count is not usable: {count}")]
    BadIterationCount {
        /// What the peer sent.
        count: u32,
    },
    /// The proof or signature did not verify.
    #[error("SCRAM verification failed")]
    VerificationFailed,
}

/// Computes `HMAC-SHA-256(key, message)`.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; KEY_LEN] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let tag = hmac::sign(&key, message);
    let mut out = [0_u8; KEY_LEN];
    out.copy_from_slice(tag.as_ref());
    out
}

/// Computes `SHA-256(input)`.
fn sha256(input: &[u8]) -> [u8; KEY_LEN] {
    let digest = digest::digest(&digest::SHA256, input);
    let mut out = [0_u8; KEY_LEN];
    out.copy_from_slice(digest.as_ref());
    out
}

/// `Hi(password, salt, iterations)` from RFC 5802: PBKDF2-HMAC-SHA-256.
///
/// # Errors
///
/// Fails when `iterations` is zero, which PBKDF2 rejects and which a hostile
/// server could otherwise use to make derivation free.
pub fn salted_password(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
) -> Result<[u8; KEY_LEN], ScramError> {
    let rounds =
        NonZeroU32::new(iterations).ok_or(ScramError::BadIterationCount { count: iterations })?;

    let mut out = [0_u8; KEY_LEN];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, password, &mut out);
    Ok(out)
}

/// The five derived values RFC 5802 builds an exchange from.
#[derive(Clone, PartialEq, Eq)]
pub struct ScramKeys {
    /// `Hi(password, salt, i)`.
    pub salted_password: [u8; KEY_LEN],
    /// `HMAC(SaltedPassword, "Client Key")`.
    pub client_key: [u8; KEY_LEN],
    /// `H(ClientKey)`. This is what a server stores.
    pub stored_key: [u8; KEY_LEN],
    /// `HMAC(SaltedPassword, "Server Key")`.
    pub server_key: [u8; KEY_LEN],
}

impl std::fmt::Debug for ScramKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // These are password-equivalent: ClientKey alone lets an attacker
        // authenticate. Printing them would be as bad as printing the password.
        f.write_str("ScramKeys([redacted])")
    }
}

impl ScramKeys {
    /// Derives every key from a password.
    ///
    /// # Errors
    ///
    /// Fails when `iterations` is zero.
    pub fn derive(password: &[u8], salt: &[u8], iterations: u32) -> Result<Self, ScramError> {
        let salted = salted_password(password, salt, iterations)?;
        let client_key = hmac_sha256(&salted, b"Client Key");
        Ok(Self {
            salted_password: salted,
            client_key,
            stored_key: sha256(&client_key),
            server_key: hmac_sha256(&salted, b"Server Key"),
        })
    }
}

/// The `client-first-message-bare`: `n=username,r=nonce`.
///
/// The username is a parameter rather than a constant because this exact string
/// goes into the [`auth_message`] that both sides sign. Hardcoding it would
/// make the proof depend on an assumption the peer does not share, and would
/// make the RFC's own test vectors unreproducible.
///
/// Against Postgres, pass the empty string: it takes the user from the startup
/// packet and ignores this field, which is what libpq sends.
#[must_use]
pub fn client_first_bare(username: &str, nonce: &str) -> String {
    format!("n={username},r={nonce}")
}

/// The full `client-first-message`, with the GS2 header.
///
/// `n,,` means the client does not support channel binding. Per ADR 0014 this
/// is the only header we send, and it must match the `c=` field later or the
/// server rejects the exchange as a downgrade attempt.
#[must_use]
pub fn client_first(username: &str, nonce: &str) -> String {
    format!("n,,{}", client_first_bare(username, nonce))
}

/// A parsed `server-first-message`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ServerFirst {
    /// The combined nonce, which must begin with the client's.
    pub nonce: String,
    /// The salt to derive with.
    pub salt: Vec<u8>,
    /// The iteration count.
    pub iterations: u32,
}

/// Parses `r=nonce,s=salt,i=iterations`.
///
/// # Errors
///
/// Fails when a field is missing, the salt is not base64, the iteration count
/// is unusable, or the nonce does not extend `client_nonce`.
pub fn parse_server_first(message: &str, client_nonce: &str) -> Result<ServerFirst, ScramError> {
    let mut nonce = None;
    let mut salt = None;
    let mut iterations = None;

    for field in message.split(',') {
        match field.split_once('=') {
            Some(("r", value)) => nonce = Some(value),
            Some(("s", value)) => salt = Some(value),
            Some(("i", value)) => iterations = Some(value),
            // RFC 5802 allows extensions; unknown fields are skipped rather
            // than rejected so a newer server stays usable.
            _ => {}
        }
    }

    let nonce = nonce.ok_or_else(|| ScramError::Malformed {
        reason: "no nonce (r=) in server-first".into(),
    })?;

    // The replay defence: a server that does not echo our nonce is not
    // answering our exchange. Checked before anything is derived.
    if !nonce.starts_with(client_nonce) || nonce.len() == client_nonce.len() {
        return Err(ScramError::NonceMismatch);
    }

    let salt = BASE64
        .decode(salt.ok_or_else(|| ScramError::Malformed {
            reason: "no salt (s=) in server-first".into(),
        })?)
        .map_err(|_| ScramError::NotBase64 { field: "salt" })?;

    let iterations: u32 = iterations
        .ok_or_else(|| ScramError::Malformed {
            reason: "no iteration count (i=) in server-first".into(),
        })?
        .parse()
        .map_err(|_| ScramError::BadIterationCount { count: 0 })?;

    if iterations == 0 {
        return Err(ScramError::BadIterationCount { count: iterations });
    }

    Ok(ServerFirst {
        nonce: nonce.to_owned(),
        salt,
        iterations,
    })
}

/// The `client-final-message-without-proof`: `c=biws,r=nonce`.
///
/// `biws` is base64 of `n,,`, the GS2 header we sent. It must match, or the
/// server treats the exchange as a downgrade attempt.
#[must_use]
pub fn client_final_without_proof(nonce: &str) -> String {
    format!("c={},r={nonce}", BASE64.encode("n,,"))
}

/// The `AuthMessage`, which both sides sign.
#[must_use]
pub fn auth_message(
    client_first_bare: &str,
    server_first: &str,
    client_final_bare: &str,
) -> String {
    format!("{client_first_bare},{server_first},{client_final_bare}")
}

/// Computes `ClientProof = ClientKey XOR HMAC(StoredKey, AuthMessage)`.
#[must_use]
pub fn client_proof(keys: &ScramKeys, auth_message: &str) -> [u8; KEY_LEN] {
    let signature = hmac_sha256(&keys.stored_key, auth_message.as_bytes());
    let mut proof = keys.client_key;
    for (p, s) in proof.iter_mut().zip(signature.iter()) {
        *p ^= *s;
    }
    proof
}

/// Computes `ServerSignature = HMAC(ServerKey, AuthMessage)`.
#[must_use]
pub fn server_signature(keys: &ScramKeys, auth_message: &str) -> [u8; KEY_LEN] {
    hmac_sha256(&keys.server_key, auth_message.as_bytes())
}

/// Checks the server's final message proves it knew the password too.
///
/// Skipping this is the mistake that turns SCRAM into a one-way check: without
/// it, anything that can complete a TCP handshake can claim success, and mutual
/// authentication was the reason to use SCRAM rather than a password.
///
/// # Errors
///
/// Fails when the message is malformed, the signature is not base64, or it does
/// not match.
pub fn verify_server_final(
    message: &str,
    keys: &ScramKeys,
    auth_message: &str,
) -> Result<(), ScramError> {
    // `e=` is the server reporting an error rather than a signature.
    if let Some(reason) = message.strip_prefix("e=") {
        return Err(ScramError::Malformed {
            reason: format!("server rejected the exchange: {reason}"),
        });
    }

    let encoded = message
        .split(',')
        .find_map(|f| f.strip_prefix("v="))
        .ok_or_else(|| ScramError::Malformed {
            reason: "no verifier (v=) in server-final".into(),
        })?;

    let received = BASE64
        .decode(encoded)
        .map_err(|_| ScramError::NotBase64 { field: "verifier" })?;

    let expected = server_signature(keys, auth_message);

    // Constant time: a byte-at-a-time comparison leaks how much of a forged
    // signature was right, which is enough to build one.
    if received.ct_eq(&expected).into() {
        Ok(())
    } else {
        Err(ScramError::VerificationFailed)
    }
}

/// Verifies a client's proof against a stored key.
///
/// The server side: `ClientKey = ClientProof XOR HMAC(StoredKey, AuthMessage)`,
/// and `H(ClientKey)` must equal the stored key.
///
/// # Errors
///
/// Fails when the proof does not verify.
pub fn verify_client_proof(
    proof: &[u8],
    stored_key: &[u8; KEY_LEN],
    auth_message: &str,
) -> Result<(), ScramError> {
    if proof.len() != KEY_LEN {
        return Err(ScramError::Malformed {
            reason: format!("client proof is {} bytes, expected {KEY_LEN}", proof.len()),
        });
    }

    let signature = hmac_sha256(stored_key, auth_message.as_bytes());
    let mut client_key = [0_u8; KEY_LEN];
    for (i, out) in client_key.iter_mut().enumerate() {
        *out = proof[i] ^ signature[i];
    }

    if sha256(&client_key).ct_eq(stored_key).into() {
        Ok(())
    } else {
        Err(ScramError::VerificationFailed)
    }
}

/// Generates a nonce.
///
/// Base64 of random bytes, which is always in the printable range RFC 5802
/// requires and never contains a comma, the field separator.
#[must_use]
pub fn generate_nonce() -> String {
    use aws_lc_rs::rand::{SecureRandom as _, SystemRandom};
    let mut bytes = [0_u8; NONCE_BYTES];
    let rng = SystemRandom::new();
    // A failure here means the system entropy source is broken, in which case
    // there is nothing safe to fall back to. Panicking is wrong on a connection
    // path, so this returns a nonce that cannot collide with a real one and
    // lets the exchange fail on verification instead.
    if rng.fill(&mut bytes).is_err() {
        return String::new();
    }
    BASE64.encode(bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The worked example from RFC 7677 section 3, which is the SHA-256
    /// profile of RFC 5802. Using the published vectors rather than our own
    /// output is the only way to know the derivation is right rather than
    /// merely self-consistent.
    const RFC7677_USER: &[u8] = b"pencil";
    const RFC7677_SALT_B64: &str = "W22ZaJ0SNY7soEsUEjb6gQ==";
    const RFC7677_ITERATIONS: u32 = 4096;
    const RFC7677_CLIENT_NONCE: &str = "rOprNGfwEbeRWgbNEkqO";
    const RFC7677_SERVER_NONCE: &str = "rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";

    fn rfc_keys() -> ScramKeys {
        let salt = BASE64.decode(RFC7677_SALT_B64).unwrap();
        ScramKeys::derive(RFC7677_USER, &salt, RFC7677_ITERATIONS).unwrap()
    }

    fn rfc_auth_message() -> String {
        auth_message(
            &client_first_bare("user", RFC7677_CLIENT_NONCE),
            &format!("r={RFC7677_SERVER_NONCE},s={RFC7677_SALT_B64},i={RFC7677_ITERATIONS}"),
            &client_final_without_proof(RFC7677_SERVER_NONCE),
        )
    }

    #[test]
    fn the_client_proof_matches_rfc_7677() {
        // The published vector. If this fails, the derivation is wrong however
        // internally consistent it looks.
        let proof = client_proof(&rfc_keys(), &rfc_auth_message());
        assert_eq!(
            BASE64.encode(proof),
            "dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=",
            "client proof does not match RFC 7677"
        );
    }

    #[test]
    fn the_server_signature_matches_rfc_7677() {
        let signature = server_signature(&rfc_keys(), &rfc_auth_message());
        assert_eq!(
            BASE64.encode(signature),
            "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=",
            "server signature does not match RFC 7677"
        );
    }

    #[test]
    fn the_client_first_message_carries_the_no_channel_binding_header() {
        // `n,,` says the client does not support channel binding, and it must
        // match the c= field later or the server sees a downgrade attempt.
        // Postgres form: no username, since it comes from the startup packet.
        assert_eq!(client_first("", "abc"), "n,,n=,r=abc");
        // RFC form, which is what makes the published vectors reproducible.
        assert_eq!(client_first("user", "abc"), "n,,n=user,r=abc");
        assert_eq!(client_final_without_proof("abcdef"), "c=biws,r=abcdef");
        assert_eq!(BASE64.decode("biws").unwrap(), b"n,,");
    }

    #[test]
    fn a_server_first_message_parses() {
        let parsed = parse_server_first(
            &format!("r={RFC7677_SERVER_NONCE},s={RFC7677_SALT_B64},i=4096"),
            RFC7677_CLIENT_NONCE,
        )
        .unwrap();
        assert_eq!(parsed.nonce, RFC7677_SERVER_NONCE);
        assert_eq!(parsed.iterations, 4096);
        assert_eq!(parsed.salt.len(), 16);
    }

    #[test]
    fn a_server_nonce_that_does_not_extend_ours_is_refused() {
        // The replay defence. A server not echoing our nonce is not answering
        // our exchange, and this is checked before anything is derived.
        for bad in [
            "r=totally-different,s=c2FsdA==,i=4096",
            "r=,s=c2FsdA==,i=4096",
        ] {
            assert_eq!(
                parse_server_first(bad, RFC7677_CLIENT_NONCE).unwrap_err(),
                ScramError::NonceMismatch,
                "{bad} was accepted"
            );
        }
    }

    #[test]
    fn a_server_nonce_equal_to_ours_is_refused() {
        // It must *extend* ours, not merely echo it: an exact echo means the
        // server contributed no randomness.
        let message = format!("r={RFC7677_CLIENT_NONCE},s=c2FsdA==,i=4096");
        assert_eq!(
            parse_server_first(&message, RFC7677_CLIENT_NONCE).unwrap_err(),
            ScramError::NonceMismatch
        );
    }

    #[test]
    fn a_zero_iteration_count_is_refused() {
        // A hostile server could otherwise make derivation free and then brute
        // force the password offline.
        let message = format!("r={RFC7677_SERVER_NONCE},s=c2FsdA==,i=0");
        assert_eq!(
            parse_server_first(&message, RFC7677_CLIENT_NONCE).unwrap_err(),
            ScramError::BadIterationCount { count: 0 }
        );
        assert!(salted_password(b"p", b"s", 0).is_err());
    }

    #[test]
    fn missing_fields_are_reported_by_name() {
        let cases = [
            ("s=c2FsdA==,i=4096".to_owned(), "nonce"),
            (format!("r={RFC7677_SERVER_NONCE},i=4096"), "salt"),
            (format!("r={RFC7677_SERVER_NONCE},s=c2FsdA=="), "iteration"),
        ];
        for (message, expected) in cases {
            let err = parse_server_first(&message, RFC7677_CLIENT_NONCE).unwrap_err();
            assert!(
                err.to_string().contains(expected),
                "{message} gave {err}, expected mention of {expected}"
            );
        }
    }

    #[test]
    fn unknown_fields_are_skipped_rather_than_rejected() {
        // RFC 5802 allows extensions. Rejecting them would break against a
        // newer server for no benefit.
        let message =
            format!("r={RFC7677_SERVER_NONCE},s={RFC7677_SALT_B64},i=4096,x=future-extension");
        assert!(parse_server_first(&message, RFC7677_CLIENT_NONCE).is_ok());
    }

    #[test]
    fn a_non_base64_salt_is_reported_as_such() {
        let message = format!("r={RFC7677_SERVER_NONCE},s=!!!not-base64!!!,i=4096");
        assert_eq!(
            parse_server_first(&message, RFC7677_CLIENT_NONCE).unwrap_err(),
            ScramError::NotBase64 { field: "salt" }
        );
    }

    #[test]
    fn the_server_final_message_verifies() {
        let keys = rfc_keys();
        let auth = rfc_auth_message();
        let signature = BASE64.encode(server_signature(&keys, &auth));
        assert!(verify_server_final(&format!("v={signature}"), &keys, &auth).is_ok());
    }

    #[test]
    fn a_wrong_server_signature_is_refused() {
        // Skipping this check turns SCRAM into a one-way password check, and
        // mutual authentication was the reason to use it.
        let keys = rfc_keys();
        let auth = rfc_auth_message();
        let wrong = BASE64.encode([0_u8; KEY_LEN]);
        assert_eq!(
            verify_server_final(&format!("v={wrong}"), &keys, &auth).unwrap_err(),
            ScramError::VerificationFailed
        );
    }

    #[test]
    fn a_server_error_message_is_reported_rather_than_treated_as_a_signature() {
        let keys = rfc_keys();
        let auth = rfc_auth_message();
        let err = verify_server_final("e=invalid-proof", &keys, &auth).unwrap_err();
        assert!(err.to_string().contains("invalid-proof"), "{err}");
    }

    #[test]
    fn a_client_proof_verifies_against_the_stored_key() {
        // The server side of the same exchange, which is what the proxy does
        // for a client that cannot carry a JWT.
        let keys = rfc_keys();
        let auth = rfc_auth_message();
        let proof = client_proof(&keys, &auth);
        assert!(verify_client_proof(&proof, &keys.stored_key, &auth).is_ok());
    }

    #[test]
    fn a_forged_client_proof_is_refused() {
        let keys = rfc_keys();
        let auth = rfc_auth_message();
        assert_eq!(
            verify_client_proof(&[0_u8; KEY_LEN], &keys.stored_key, &auth).unwrap_err(),
            ScramError::VerificationFailed
        );
    }

    #[test]
    fn a_proof_of_the_wrong_length_is_malformed_rather_than_wrong() {
        // A different error, because a short proof is a protocol mistake while
        // a wrong one is an authentication failure, and conflating them makes
        // debugging harder.
        let keys = rfc_keys();
        let err = verify_client_proof(&[0_u8; 8], &keys.stored_key, "msg").unwrap_err();
        assert!(matches!(err, ScramError::Malformed { .. }), "{err:?}");
    }

    #[test]
    fn a_proof_for_a_different_auth_message_is_refused() {
        // The binding that stops a captured proof being replayed into another
        // exchange.
        let keys = rfc_keys();
        let proof = client_proof(&keys, &rfc_auth_message());
        assert_eq!(
            verify_client_proof(&proof, &keys.stored_key, "a different auth message").unwrap_err(),
            ScramError::VerificationFailed
        );
    }

    #[test]
    fn keys_never_print_themselves() {
        // ClientKey alone lets an attacker authenticate, so these are
        // password-equivalent.
        let keys = rfc_keys();
        let rendered = format!("{keys:?}");
        assert_eq!(rendered, "ScramKeys([redacted])");
        assert!(!rendered.contains(&BASE64.encode(keys.client_key)));
    }

    #[test]
    fn nonces_are_unique_printable_and_comma_free() {
        // A comma would split a field, and a non-printable byte violates
        // RFC 5802's grammar.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let nonce = generate_nonce();
            assert!(!nonce.is_empty(), "entropy source failed");
            assert!(!nonce.contains(','), "nonce contains the field separator");
            assert!(
                nonce.chars().all(|c| c.is_ascii_graphic()),
                "nonce is not printable: {nonce}"
            );
            assert!(seen.insert(nonce), "a nonce repeated");
        }
    }

    #[test]
    fn derivation_is_deterministic_for_the_same_inputs() {
        let a = ScramKeys::derive(b"pw", b"salt", 100).unwrap();
        let b = ScramKeys::derive(b"pw", b"salt", 100).unwrap();
        assert!(a == b);

        let different = ScramKeys::derive(b"pw", b"other-salt", 100).unwrap();
        assert!(a != different, "the salt did not affect derivation");
    }

    #[test]
    fn parsing_never_panics_on_arbitrary_input() {
        let mut seed = 0xC0FF_EE00_1234_5678_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 48).unwrap();
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from(0x20 + ((seed >> (i % 8 * 8)) & 0x3F)).unwrap())
                .collect();
            let text = String::from_utf8_lossy(&bytes);

            let _ = parse_server_first(&text, "nonce");
            let keys = ScramKeys::derive(b"p", b"s", 1).unwrap();
            let _ = verify_server_final(&text, &keys, &text);
            let _ = verify_client_proof(bytes.as_slice(), &keys.stored_key, &text);
        }
    }
}
