//! JWT header inspection.
//!
//! # This does not verify anything
//!
//! The sidecar owns signature and claim validation. Two validators that
//! disagree about whether a token is valid is a vulnerability, not redundancy,
//! so nothing here checks a signature and nothing here decides a token is good.
//!
//! What it does is refuse tokens whose header names an algorithm outside the
//! allowlist, before the sidecar is called. That is defence in depth against a
//! sidecar that would otherwise accept `alg: none`, and it costs one base64
//! decode of a header that is a few dozen bytes.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pgprox_core::error::AuthRejection;

/// Algorithms the proxy will pass to the sidecar.
///
/// Asymmetric signatures only. The `HS*` family is symmetric, so accepting it
/// would mean the verification key is also a signing key, and a leaked
/// verification key would let anyone mint tokens.
///
/// `EdDSA` is absent deliberately: it is not exposed by the FIPS validated module
/// this proxy can be built against, and an algorithm that works in one build
/// and not the other is worse than one that never works.
pub const ALLOWED_ALGORITHMS: &[&str] = &["RS256", "RS384", "RS512", "PS256", "ES256", "ES384"];

/// Checks a token's header names an allowed algorithm.
///
/// # Errors
///
/// Returns [`AuthRejection::Malformed`] when the token is not three
/// dot-separated base64url segments or the header is not JSON with an `alg`
/// string, and [`AuthRejection::AlgorithmNotAllowed`] when the algorithm is not
/// in [`ALLOWED_ALGORITHMS`].
pub fn check_algorithm(token: &str) -> Result<(), AuthRejection> {
    let alg = header_algorithm(token)?;
    if ALLOWED_ALGORITHMS.contains(&alg.as_str()) {
        Ok(())
    } else {
        Err(AuthRejection::AlgorithmNotAllowed)
    }
}

/// Reads the `alg` field from a token's header.
///
/// # Errors
///
/// Returns [`AuthRejection::Malformed`] if the token is not shaped like a JWS
/// or the header does not decode to JSON with a string `alg`.
pub fn header_algorithm(token: &str) -> Result<String, AuthRejection> {
    let mut parts = token.split('.');
    let (Some(header), Some(_payload), Some(_signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(AuthRejection::Malformed);
    };

    if header.is_empty() {
        return Err(AuthRejection::Malformed);
    }

    let decoded = URL_SAFE_NO_PAD
        .decode(header)
        .map_err(|_| AuthRejection::Malformed)?;

    let json: serde_json::Value =
        serde_json::from_slice(&decoded).map_err(|_| AuthRejection::Malformed)?;

    json.get("alg")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or(AuthRejection::Malformed)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Builds a token with the given header JSON. The payload and signature are
    /// never inspected here, so they can be anything.
    fn token_with_header(header_json: &str) -> String {
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(header_json),
            URL_SAFE_NO_PAD.encode(r#"{"sub":"user"}"#),
            URL_SAFE_NO_PAD.encode("not-a-real-signature")
        )
    }

    #[test]
    fn every_allowed_algorithm_passes() {
        for alg in ALLOWED_ALGORITHMS {
            let token = token_with_header(&format!(r#"{{"alg":"{alg}","typ":"JWT"}}"#));
            assert!(check_algorithm(&token).is_ok(), "{alg} was rejected");
        }
    }

    #[test]
    fn alg_none_is_rejected() {
        // The classic JWT attack: strip the signature and claim the token needs
        // none. Rejected here before the sidecar is asked.
        let token = token_with_header(r#"{"alg":"none"}"#);
        assert_eq!(
            check_algorithm(&token).unwrap_err(),
            AuthRejection::AlgorithmNotAllowed
        );
        // Case variations too, since JSON values are compared exactly and an
        // attacker will try them.
        for variant in ["None", "NONE", "nOnE"] {
            let token = token_with_header(&format!(r#"{{"alg":"{variant}"}}"#));
            assert_eq!(
                check_algorithm(&token).unwrap_err(),
                AuthRejection::AlgorithmNotAllowed,
                "{variant} was accepted"
            );
        }
    }

    #[test]
    fn the_symmetric_family_is_rejected() {
        // HS* verification keys are also signing keys, so accepting them means a
        // leaked verification key lets anyone mint tokens.
        for alg in ["HS256", "HS384", "HS512"] {
            let token = token_with_header(&format!(r#"{{"alg":"{alg}"}}"#));
            assert_eq!(
                check_algorithm(&token).unwrap_err(),
                AuthRejection::AlgorithmNotAllowed,
                "{alg} was accepted"
            );
        }
    }

    #[test]
    fn eddsa_is_rejected_because_the_fips_build_cannot_use_it() {
        // Not a security judgement. An algorithm that works in one build and
        // not the other is worse than one that never works.
        let token = token_with_header(r#"{"alg":"EdDSA"}"#);
        assert_eq!(
            check_algorithm(&token).unwrap_err(),
            AuthRejection::AlgorithmNotAllowed
        );
    }

    #[test]
    fn a_token_with_the_wrong_number_of_segments_is_malformed() {
        for bad in [
            "",
            "onlyone",
            "two.parts",
            "four.parts.here.now",
            "..",
            "a.b.c.d",
        ] {
            assert_eq!(
                check_algorithm(bad).unwrap_err(),
                AuthRejection::Malformed,
                "{bad:?} was not rejected"
            );
        }
    }

    #[test]
    fn a_header_that_is_not_base64_is_malformed() {
        assert_eq!(
            check_algorithm("!!!not-base64!!!.payload.sig").unwrap_err(),
            AuthRejection::Malformed
        );
    }

    #[test]
    fn a_header_that_is_not_json_is_malformed() {
        let token = format!("{}.payload.sig", URL_SAFE_NO_PAD.encode("this is not json"));
        assert_eq!(
            check_algorithm(&token).unwrap_err(),
            AuthRejection::Malformed
        );
    }

    #[test]
    fn a_header_without_an_alg_field_is_malformed() {
        let token = token_with_header(r#"{"typ":"JWT"}"#);
        assert_eq!(
            check_algorithm(&token).unwrap_err(),
            AuthRejection::Malformed
        );
    }

    #[test]
    fn a_non_string_alg_is_malformed() {
        // A number or object where a string belongs must not be coerced.
        for header in [r#"{"alg":256}"#, r#"{"alg":null}"#, r#"{"alg":["RS256"]}"#] {
            let token = token_with_header(header);
            assert_eq!(
                check_algorithm(&token).unwrap_err(),
                AuthRejection::Malformed,
                "{header} was accepted"
            );
        }
    }

    #[test]
    fn the_algorithm_is_readable_without_being_judged() {
        // header_algorithm reports what the token claims; check_algorithm
        // decides. Keeping them separate means logs can record a rejected
        // algorithm.
        let token = token_with_header(r#"{"alg":"HS256"}"#);
        assert_eq!(header_algorithm(&token).unwrap(), "HS256");
        assert!(check_algorithm(&token).is_err());
    }

    #[test]
    fn checking_never_panics_on_arbitrary_input() {
        // Tokens arrive from unauthenticated clients.
        let mut seed = 0x5DEE_CE66_D1B4_7A5F_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 48).unwrap();
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0x7F).unwrap())
                .collect();
            let text = String::from_utf8_lossy(&bytes);
            let _ = check_algorithm(&text);
            let _ = header_algorithm(&text);
        }
    }
}
