//! TLS for the load client, and only for the load client.
//!
//! # Why this verifies nothing
//!
//! The stack this measures makes a self-signed certificate per node at start,
//! because a test stack with a checked-in private key would be a repository
//! with a private key in it. There is no authority to verify against, and the
//! question being asked is what termination costs, not who the peer is.
//!
//! This is therefore a deliberately insecure client. It lives in the load
//! generator, it is reached only through `--tls-insecure`, and nothing in the
//! proxy links against it. `pgprox-tls` is what the product uses and it
//! verifies properly; if this file is ever copied from, that is the mistake.

use std::sync::Arc;

use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::crypto::{
    CryptoProvider, verify_tls12_signature, verify_tls13_signature,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{ClientConfig, DigitallySignedStruct, Error, SignatureScheme};

/// A verifier that accepts any certificate.
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    // The signature checks stay real. They cost what they cost, and this is a
    // tool for measuring what things cost.
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// A client configuration that trusts whatever it is given.
///
/// # Errors
///
/// Fails when the process has no crypto provider and one cannot be installed,
/// which is a build problem rather than a runtime one.
pub fn insecure_config() -> Result<Arc<ClientConfig>, String> {
    let provider = CryptoProvider::get_default().cloned().unwrap_or_else(|| {
        let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
        // Ignored on purpose: a second caller losing the race means a provider
        // is installed, which is the thing being asked for.
        let _ = CryptoProvider::install_default((*provider).clone());
        provider
    });

    let config = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("tls: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(provider)))
        .with_no_client_auth();

    Ok(Arc::new(config))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_config_is_built_and_reused() {
        // Twice, because the crypto provider is process-global and the second
        // call is the one that would fail if installing it were not tolerant
        // of already being installed.
        assert!(insecure_config().is_ok());
        assert!(insecure_config().is_ok());
    }

    #[test]
    fn the_supported_schemes_come_from_the_provider() {
        // The signature checks stay real, and they are the provider's. This
        // asserts the wiring: a verifier that offered no schemes would fail
        // every handshake rather than accepting everything.
        let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
        let schemes = AcceptAny(Arc::clone(&provider)).supported_verify_schemes();
        assert_eq!(
            schemes,
            provider
                .signature_verification_algorithms
                .supported_schemes()
        );
    }

    #[test]
    fn every_certificate_is_accepted() {
        // The point of the file, asserted so that a future change which starts
        // verifying is a failing test rather than a load run that cannot
        // connect to the stack it is meant to measure.
        let provider = Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
        let verifier = AcceptAny(provider);
        let outcome = verifier.verify_server_cert(
            &CertificateDer::from(vec![0_u8; 4]),
            &[],
            &ServerName::try_from("pgprox-1").unwrap(),
            &[],
            UnixTime::now(),
        );
        assert!(outcome.is_ok());
        assert!(!verifier.supported_verify_schemes().is_empty());
    }
}
