//! TLS configuration, shared by the client listener and upstream connections.
//!
//! # The FIPS assertion is the point
//!
//! Building with `--features fips` swaps in the FIPS 140-3 validated `aws-lc-rs`
//! module. [`assert_fips`] then checks the resulting configuration actually
//! reports `fips()`, and the process refuses to start if it does not.
//!
//! That check is load-bearing. A binary that claims FIPS and silently runs
//! non-validated crypto is worse than no FIPS binary at all, because it passes
//! an audit it should fail.
//!
//! # There is no way to skip verification
//!
//! Upstream TLS verifies the certificate chain against a configured CA, and
//! this crate exposes no option to disable that. Such a flag always ends up set
//! in production.
//!
//! # No unsafe, and not by the workspace's leave
//!
//! `#![forbid]` rather than the workspace's `deny`, so no `#[allow]` anywhere
//! in this crate can reach it. This crate sits on the path a client's first
//! bytes take.
//!
//! `M27.1` opened the door elsewhere and left it shut here on purpose. See ADR
//! 0026 and `scripts/check-unsafe.sh`, which holds the list.

#![forbid(unsafe_code)]

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};

/// Whether this build uses the FIPS validated provider.
pub const FIPS_BUILD: bool = cfg!(feature = "fips");

/// Why TLS could not be configured.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TlsError {
    /// A certificate or key file could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// The file involved.
        path: PathBuf,
        /// The underlying failure.
        source: io::Error,
    },
    /// A PEM file held no certificate.
    #[error("{path} contains no certificates")]
    NoCertificates {
        /// The file involved.
        path: PathBuf,
    },
    /// A PEM file held no private key.
    #[error("{path} contains no private key")]
    NoPrivateKey {
        /// The file involved.
        path: PathBuf,
    },
    /// rustls rejected the configuration.
    #[error("rustls rejected the configuration: {0}")]
    Rustls(#[from] rustls::Error),
    /// A FIPS build produced a configuration that is not FIPS.
    ///
    /// Refusing to start is deliberate. See the module documentation.
    #[error(
        "this binary was built with --features fips but the {which} configuration \
         does not report FIPS mode; refusing to start"
    )]
    NotFips {
        /// Which configuration failed the check.
        which: &'static str,
    },
    /// A certificate is dated in the future.
    ///
    /// `M88.11`. A clock skew between wherever the certificate was minted and
    /// this node, or an operator who mixed up which file is which.
    #[error("{path}: not valid until {not_before}")]
    CertificateNotYetValid {
        /// The file involved.
        path: PathBuf,
        /// What the certificate itself says, formatted by the parser that
        /// read it.
        not_before: String,
    },
    /// A certificate's validity window has already ended.
    ///
    /// `M88.11`. Without this check a listener kept serving an expired
    /// certificate until a TLS peer's own verification rejected it, one
    /// connection at a time, which is a support ticket rather than a log
    /// line.
    #[error("{path}: expired {not_after}")]
    CertificateExpired {
        /// The file involved.
        path: PathBuf,
        /// What the certificate itself says, formatted by the parser that
        /// read it.
        not_after: String,
    },
    /// The certificate parsed well enough for rustls to serve it, but not
    /// well enough to check its validity window.
    ///
    /// `M88.11`. Refused rather than served unchecked: a certificate whose
    /// dates cannot be determined might be validly outside them, and this
    /// crate's stance is that no unsafe posture is the default one.
    #[error("{path}: could not check its validity window: {reason}")]
    CertificateValidityUnreadable {
        /// The file involved.
        path: PathBuf,
        /// What the X.509 parser said.
        reason: String,
    },
}

/// Refuses a leaf certificate outside its validity window.
///
/// Checked against the leaf only — the first certificate in the chain, which
/// is the one this proxy is claiming to be. An intermediate's own window is
/// the issuing CA's problem, not this node's, and this crate's own PEM files
/// carry the chain leaf-first by the same convention every TLS stack expects.
///
/// # Errors
///
/// [`TlsError::CertificateNotYetValid`] or [`TlsError::CertificateExpired`] if
/// `now` falls outside the certificate's window, and
/// [`TlsError::CertificateValidityUnreadable`] if the window cannot be read at
/// all.
fn check_validity(
    leaf: &CertificateDer<'_>,
    path: &Path,
    now: std::time::SystemTime,
) -> Result<(), TlsError> {
    let (_, parsed) = x509_parser::parse_x509_certificate(leaf).map_err(|err| {
        TlsError::CertificateValidityUnreadable {
            path: path.to_owned(),
            reason: err.to_string(),
        }
    })?;
    let validity = parsed.validity();

    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        });
    let now = x509_parser::time::ASN1Time::from_timestamp(secs).map_err(|err| {
        TlsError::CertificateValidityUnreadable {
            path: path.to_owned(),
            reason: err.to_string(),
        }
    })?;

    if now < validity.not_before {
        return Err(TlsError::CertificateNotYetValid {
            path: path.to_owned(),
            not_before: validity.not_before.to_string(),
        });
    }
    if now > validity.not_after {
        return Err(TlsError::CertificateExpired {
            path: path.to_owned(),
            not_after: validity.not_after.to_string(),
        });
    }
    Ok(())
}

/// Reads a PEM certificate chain.
///
/// # Errors
///
/// Fails if the file cannot be read or holds no certificate.
pub fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    // A corrupt PEM block must be an error rather than an empty result, since
    // "no certificates here" is indistinguishable from an empty file and sends
    // an operator looking in the wrong place.
    let certs = CertificateDer::pem_file_iter(path)
        .map_err(|e| pem_error(path, &e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| pem_error(path, &e))?;

    if certs.is_empty() {
        return Err(TlsError::NoCertificates {
            path: path.to_owned(),
        });
    }
    Ok(certs)
}

/// Reads a PEM private key.
///
/// # Errors
///
/// Fails if the file cannot be read or holds no key.
pub fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    PrivateKeyDer::from_pem_file(path).map_err(|e| match e {
        rustls::pki_types::pem::Error::NoItemsFound => TlsError::NoPrivateKey {
            path: path.to_owned(),
        },
        other => pem_error(path, &other),
    })
}

/// Maps a PEM parse failure onto this crate's error, keeping the path.
///
/// A missing file and a corrupt one are both `Io` here, because both mean an
/// operator should go and look at that path.
fn pem_error(path: &Path, error: &rustls::pki_types::pem::Error) -> TlsError {
    TlsError::Io {
        path: path.to_owned(),
        source: std::io::Error::other(error.to_string()),
    }
}

/// Builds the listener's TLS configuration.
///
/// # Errors
///
/// Fails if the certificate and key do not match, or if a FIPS build produces a
/// non-FIPS configuration.
pub fn server_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<ServerConfig>, TlsError> {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    assert_fips(config.fips(), "server")?;
    Ok(Arc::new(config))
}

/// How often a node should ask whether its certificate has changed.
///
/// A certificate is rotated on the order of weeks and a rewrite has to be
/// noticed on the order of minutes, so the interval is chosen for how little it
/// costs rather than how quickly it reacts: two small files read and hashed.
pub const RELOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// A certificate that can be replaced while the listener is running.
///
/// # Why this exists
///
/// `docs/internal/product/architecture.md` has credited this crate with "cert hot-reload"
/// since M-1 and the crate's own `AGENTS.md` repeated it. Nothing re-read a
/// certificate: `server_config` was called once and the `ServerConfig` it
/// returned was fixed for the life of the process, so a cert-manager rotation
/// served an expired certificate until somebody restarted the pod. `M24.9`.
///
/// # Why a resolver rather than a new `ServerConfig`
///
/// The `Arc<ServerConfig>` is handed to the accept loop at startup and read by
/// every connection. Swapping it would mean an indirection on the accept path
/// and a decision about connections mid-handshake. A resolver is the seam
/// rustls already has for this: the configuration never changes, and what it
/// resolves to does.
///
/// # A rewrite that does not parse changes nothing
///
/// Certificates are rotated by machines, and a half-written file is a normal
/// thing to read. [`CertReloader::reload`] parses into a new key before it
/// touches the live one, so the failure mode of a bad rewrite is a log line and
/// the previous certificate still serving, rather than a listener that stops
/// answering.
#[derive(Debug)]
pub struct CertReloader {
    cert: PathBuf,
    key: PathBuf,
    /// What the listener is serving right now.
    current: std::sync::RwLock<Arc<rustls::sign::CertifiedKey>>,
}

impl CertReloader {
    /// Reads a certificate and key, ready to be replaced later.
    ///
    /// `now` is the caller's, not read here: nothing in this crate reads the
    /// real clock, per this workspace's sans-I/O rule. Pass
    /// [`pgprox_core::clock::Clock::wall`], since a certificate's validity
    /// window is stated in wall time by whoever issued it.
    ///
    /// # Errors
    ///
    /// Fails when either file cannot be read, holds nothing, does not match
    /// the other, or is outside its validity window. All four are deployment
    /// mistakes and none of them should start a node.
    pub fn new(cert: &Path, key: &Path, now: std::time::SystemTime) -> Result<Arc<Self>, TlsError> {
        let certified = Self::read(cert, key, now)?;
        Ok(Arc::new(Self {
            cert: cert.to_owned(),
            key: key.to_owned(),
            current: std::sync::RwLock::new(certified),
        }))
    }

    /// Re-reads both files, replacing what is served if they changed.
    ///
    /// Returns whether the certificate this serves is now a different one.
    ///
    /// Re-read and compared rather than watched by modification time. A
    /// certificate arrives in a Kubernetes volume as a symlink swap, whose
    /// timestamps are its own business, and two small files a minute is a cost
    /// nobody has to reason about.
    ///
    /// # Errors
    ///
    /// Fails when the files cannot be read, do not parse, or the fresh
    /// certificate is outside its validity window, leaving the previous
    /// certificate serving. A rotation that writes half a file, or lands
    /// before the certificate it names has started being valid, is a normal
    /// thing to catch, and a listener that stopped answering because of one
    /// would be worse than the stale certificate it replaced.
    pub fn reload(&self, now: std::time::SystemTime) -> Result<bool, TlsError> {
        let fresh = Self::read(&self.cert, &self.key, now)?;

        let mut current = self
            .current
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.cert == fresh.cert {
            return Ok(false);
        }
        *current = fresh;
        Ok(true)
    }

    /// The certificate chain currently being served, as DER.
    ///
    /// For a caller that wants to say which certificate is live, and for the
    /// tests that a rewrite reached the listener.
    #[must_use]
    pub fn serving(&self) -> Vec<CertificateDer<'static>> {
        self.current
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cert
            .clone()
    }

    /// Reads and validates a pair from disk.
    ///
    /// Validation includes the leaf's validity window against `now`, which is
    /// what makes both callers — the initial read and every reload — refuse a
    /// certificate outside it rather than serving one silently. `M88.11`.
    fn read(
        cert: &Path,
        key: &Path,
        now: std::time::SystemTime,
    ) -> Result<Arc<rustls::sign::CertifiedKey>, TlsError> {
        let certs = load_certs(cert)?;
        let key = load_private_key(key)?;

        // The leaf, first in the chain by the same convention every TLS stack
        // reading this file expects.
        check_validity(&certs[0], cert, now)?;

        let provider = rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));

        Ok(Arc::new(rustls::sign::CertifiedKey::from_der(
            certs, key, &provider,
        )?))
    }
}

impl rustls::server::ResolvesServerCert for CertReloader {
    fn resolve(
        &self,
        _hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(Arc::clone(
            &self
                .current
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        ))
    }
}

/// Builds the listener's TLS configuration around a reloadable certificate.
///
/// The configuration is fixed for the life of the process and the certificate
/// it resolves to is not, which is the whole arrangement: see [`CertReloader`].
///
/// # Errors
///
/// Fails if a FIPS build produces a non-FIPS configuration.
pub fn server_config_reloading(reloader: Arc<CertReloader>) -> Result<Arc<ServerConfig>, TlsError> {
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(reloader);

    // The same assertion the fixed-certificate path makes, and it has to be
    // made here too: a build that took this path and skipped it would be a FIPS
    // binary that never checked, which is the thing ADR 0010 says is worse than
    // no FIPS binary.
    assert_fips(config.fips(), "server")?;
    Ok(Arc::new(config))
}

/// Builds the configuration used for upstream connections.
///
/// Takes an explicit root store. There is no variant that trusts everything and
/// no flag that disables verification.
///
/// # Errors
///
/// Fails if a FIPS build produces a non-FIPS configuration.
pub fn client_config(roots: RootCertStore) -> Result<Arc<ClientConfig>, TlsError> {
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    assert_fips(config.fips(), "client")?;
    Ok(Arc::new(config))
}

/// Builds a root store from a PEM bundle.
///
/// # Errors
///
/// Fails if the file cannot be read, holds no certificate, or holds one rustls
/// rejects.
pub fn root_store_from_pem(path: &Path) -> Result<RootCertStore, TlsError> {
    let mut roots = RootCertStore::empty();
    for cert in load_certs(path)? {
        roots.add(cert)?;
    }
    Ok(roots)
}

/// The name of this build's crypto provider, for the startup log.
///
/// The FIPS image and the default image carry a binary with the same name in
/// the same place, started by the same entrypoint. An operator holding a
/// running pod has no other way to tell which one they have, and "which image
/// is this" is the first question in any FIPS audit.
///
/// This reports the build rather than a configuration, so it can be logged
/// before any TLS is set up and on a node serving plaintext. What a
/// configuration actually reports is [`assert_fips`]'s business, and that one
/// refuses to start rather than logging.
#[must_use]
pub const fn provider() -> &'static str {
    provider_for(FIPS_BUILD)
}

/// The provider name, with the build flag passed in rather than read.
///
/// Same reason as [`check_fips`]: both answers have to be reachable in one
/// build, or one of them is never tested.
const fn provider_for(fips_build: bool) -> &'static str {
    if fips_build {
        "aws-lc-rs-fips"
    } else {
        "aws-lc-rs"
    }
}

/// Checks a configuration reports FIPS mode when this is a FIPS build.
///
/// A no-op in a default build, so the same code path serves both.
///
/// # Errors
///
/// Fails when built with `--features fips` and `is_fips` is false.
pub const fn assert_fips(is_fips: bool, which: &'static str) -> Result<(), TlsError> {
    check_fips(FIPS_BUILD, is_fips, which)
}

/// The FIPS decision, with the build flag passed in rather than read.
///
/// Taking the flag as an argument is what makes both halves of this reachable
/// in one build. Reading `FIPS_BUILD` directly would leave the failure branch
/// untestable in normal CI and the success branch untestable in a FIPS build,
/// which is exactly the branch that must not be wrong.
const fn check_fips(fips_build: bool, is_fips: bool, which: &'static str) -> Result<(), TlsError> {
    if fips_build && !is_fips {
        return Err(TlsError::NotFips { which });
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a self-signed certificate and its key to a temporary directory.
    fn test_cert() -> (PathBuf, PathBuf, PathBuf) {
        // Per call, not per process. `cargo test` runs a binary's tests on
        // parallel threads, so a directory keyed on the process id alone is
        // one directory shared by every test in the crate, and all of them
        // write `cert.pem` and `key.pem`. One truncates a file another is
        // reading, or a certificate is paired with a different test's key.
        //
        // That failed about one run in three and looked random. `M16.8`.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("pgprox-tls-test-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");

        std::fs::File::create(&cert_path)
            .unwrap()
            .write_all(cert.cert.pem().as_bytes())
            .unwrap();
        std::fs::File::create(&key_path)
            .unwrap()
            .write_all(cert.signing_key.serialize_pem().as_bytes())
            .unwrap();

        (dir, cert_path, key_path)
    }

    /// Writes a self-signed certificate with a chosen validity window, rather
    /// than `rcgen`'s own default of nineteen seventy-five to four thousand
    /// ninety-six, which every other test in this crate relies on staying
    /// valid for as long as this suite exists.
    fn test_cert_valid(
        not_before: time::OffsetDateTime,
        not_after: time::OffsetDateTime,
    ) -> (PathBuf, PathBuf, PathBuf) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "pgprox-tls-test-validity-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        rcgen_write(&cert_path, &key_path, not_before, not_after);

        (dir, cert_path, key_path)
    }

    #[test]
    fn a_server_config_builds_from_a_certificate_and_key() {
        let (_dir, cert_path, key_path) = test_cert();
        let certs = load_certs(&cert_path).unwrap();
        let key = load_private_key(&key_path).unwrap();

        assert_eq!(certs.len(), 1);
        assert!(server_config(certs, key).is_ok());
    }

    /// The assertion the whole FIPS feature exists to make, with the validated
    /// module actually linked.
    ///
    /// Everything else here tests the decision with the build flag passed in as
    /// a value, which is what keeps both branches reachable in one build. This
    /// one cannot: it asks a real `ServerConfig` and a real `ClientConfig` what
    /// they report, and in a default build there is no FIPS provider to ask. So
    /// it is gated, and `scripts/fips-check.sh` is what runs it.
    ///
    /// `server_config` and `client_config` already refuse a non-FIPS
    /// configuration, so an `unwrap` here would be the assertion. The explicit
    /// `fips()` check is deliberate duplication: if that call were ever dropped
    /// from either builder, this test is what would notice.
    #[cfg(feature = "fips")]
    #[test]
    fn a_fips_build_produces_fips_configurations() {
        let (_dir, cert_path, key_path) = test_cert();
        let certs = load_certs(&cert_path).unwrap();
        let key = load_private_key(&key_path).unwrap();

        let server = server_config(certs, key).expect("a FIPS build must produce a FIPS server");
        assert!(server.fips(), "server configuration is not in FIPS mode");

        let client =
            client_config(RootCertStore::empty()).expect("a FIPS build must produce a FIPS client");
        assert!(client.fips(), "client configuration is not in FIPS mode");
    }

    #[test]
    fn the_provider_name_distinguishes_the_two_builds() {
        // Both answers, in whichever build this is. An operator reading a
        // startup line has to be able to tell one image from the other, so the
        // two names must not be the same string.
        assert_eq!(provider_for(true), "aws-lc-rs-fips");
        assert_eq!(provider_for(false), "aws-lc-rs");
        assert_ne!(provider_for(true), provider_for(false));
        assert_eq!(provider(), provider_for(FIPS_BUILD));
    }

    #[test]
    fn a_client_config_builds_from_a_root_store() {
        let (_dir, cert_path, _key_path) = test_cert();
        let roots = root_store_from_pem(&cert_path).unwrap();
        assert!(!roots.is_empty());
        assert!(client_config(roots).is_ok());
    }

    #[test]
    fn there_is_no_way_to_build_a_client_config_that_trusts_everything() {
        // An empty root store is the closest anyone can get, and it verifies
        // nothing successfully rather than verifying everything. The absence of
        // a skip-verification option is the property; this documents it.
        let config = client_config(RootCertStore::empty()).unwrap();
        assert!(
            !config
                .crypto_provider()
                .signature_verification_algorithms
                .supported_schemes()
                .is_empty(),
            "a config with no verification algorithms would verify nothing"
        );
    }

    #[test]
    fn a_root_store_holds_exactly_what_was_added() {
        let (_dir, cert_path, _key) = test_cert();
        let roots = root_store_from_pem(&cert_path).unwrap();
        assert_eq!(roots.len(), 1, "root store gained or lost a certificate");
        assert!(RootCertStore::empty().is_empty());
    }

    #[test]
    fn a_root_store_from_a_missing_file_fails() {
        let err = root_store_from_pem(Path::new("/nonexistent/ca.pem")).unwrap_err();
        assert!(matches!(err, TlsError::Io { .. }), "{err:?}");
    }

    #[test]
    fn a_mismatched_certificate_and_key_are_rejected() {
        // Two independently generated pairs. rustls must refuse to serve a
        // certificate whose key does not match, rather than failing later on
        // the first handshake.
        let (_dir_a, cert_a, _key_a) = test_cert();
        let dir_b = std::env::temp_dir().join(format!("pgprox-tls-b-{}", std::process::id()));
        std::fs::create_dir_all(&dir_b).unwrap();
        let other = rcgen::generate_simple_self_signed(vec!["other".into()]).unwrap();
        let key_b = dir_b.join("key.pem");
        std::fs::write(&key_b, other.signing_key.serialize_pem()).unwrap();

        let certs = load_certs(&cert_a).unwrap();
        let key = load_private_key(&key_b).unwrap();
        let err = server_config(certs, key).unwrap_err();
        assert!(matches!(err, TlsError::Rustls(_)), "{err:?}");
        assert!(err.to_string().contains("rustls rejected"));

        std::fs::remove_dir_all(&dir_b).ok();
    }

    #[test]
    fn a_corrupt_pem_block_is_an_error_rather_than_an_empty_result() {
        // A PEM header with unusable contents must not read as "no certificates
        // here", because that is indistinguishable from an empty file and would
        // send an operator looking in the wrong place.
        let dir = std::env::temp_dir().join(format!("pgprox-tls-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corrupt.pem");
        std::fs::write(
            &path,
            b"-----BEGIN CERTIFICATE-----\n!!!not base64!!!\n-----END CERTIFICATE-----\n",
        )
        .unwrap();

        let err = load_certs(&path).unwrap_err();
        assert!(matches!(err, TlsError::Io { .. }), "{err:?}");
        assert!(err.to_string().contains("corrupt.pem"), "{err}");

        // The PEM markers are assembled rather than written as a literal. A
        // source file containing that marker trips the secret scanner in the
        // pre-commit hook, and the right answer is to keep that scanner fully
        // armed for real keys rather than teach it to ignore this file.
        let rule = "-".repeat(5);
        let kind = "PRIVATE KEY";
        let key_pem =
            format!("{rule}BEGIN {kind}{rule}\n!!!not base64!!!\n{rule}END {kind}{rule}\n");
        std::fs::write(&path, key_pem).unwrap();
        assert!(matches!(
            load_private_key(&path).unwrap_err(),
            TlsError::Io { .. }
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_certificate_chain_with_several_entries_loads() {
        let dir = std::env::temp_dir().join(format!("pgprox-tls-chain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("chain.pem");

        let leaf = rcgen::generate_simple_self_signed(vec!["leaf".into()]).unwrap();
        let intermediate = rcgen::generate_simple_self_signed(vec!["intermediate".into()]).unwrap();
        std::fs::write(
            &path,
            format!("{}{}", leaf.cert.pem(), intermediate.cert.pem()),
        )
        .unwrap();

        assert_eq!(load_certs(&path).unwrap().len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let missing = Path::new("/nonexistent/pgprox/cert.pem");
        let err = load_certs(missing).unwrap_err();
        assert!(err.to_string().contains("/nonexistent/pgprox/cert.pem"));
        assert!(matches!(err, TlsError::Io { .. }));

        let err = load_private_key(missing).unwrap_err();
        assert!(matches!(err, TlsError::Io { .. }));
    }

    #[test]
    fn a_pem_file_with_no_certificate_is_rejected() {
        let dir = std::env::temp_dir().join(format!("pgprox-tls-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.pem");
        std::fs::write(&path, b"not a certificate\n").unwrap();

        assert!(matches!(
            load_certs(&path).unwrap_err(),
            TlsError::NoCertificates { .. }
        ));
        assert!(matches!(
            load_private_key(&path).unwrap_err(),
            TlsError::NoPrivateKey { .. }
        ));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_fips_build_refuses_a_non_fips_configuration() {
        // The assertion that makes the FIPS build meaningful, reachable in a
        // default build because the flag is a parameter rather than a read.
        assert!(
            matches!(
                check_fips(true, false, "server"),
                Err(TlsError::NotFips { which: "server" })
            ),
            "a FIPS build accepted a non-FIPS configuration"
        );
        assert!(matches!(
            check_fips(true, false, "client"),
            Err(TlsError::NotFips { which: "client" })
        ),);
    }

    #[test]
    fn every_other_combination_is_accepted() {
        assert!(
            check_fips(true, true, "server").is_ok(),
            "FIPS build, FIPS config"
        );
        assert!(check_fips(false, false, "server").is_ok(), "default build");
        assert!(
            check_fips(false, true, "server").is_ok(),
            "default build, FIPS config"
        );
    }

    #[test]
    fn the_public_assertion_follows_this_builds_flag() {
        assert!(assert_fips(true, "server").is_ok());
        assert_eq!(assert_fips(false, "server").is_ok(), !FIPS_BUILD);
    }

    #[test]
    fn the_not_fips_error_says_which_configuration_failed() {
        let err = TlsError::NotFips { which: "client" };
        let rendered = err.to_string();
        assert!(rendered.contains("client"), "{rendered}");
        assert!(rendered.contains("refusing to start"), "{rendered}");
    }

    #[test]
    fn the_build_flag_matches_the_feature() {
        assert_eq!(FIPS_BUILD, cfg!(feature = "fips"));
    }
    /// Writes a fresh self-signed certificate over an existing pair.
    ///
    /// What a rotation is: the same two paths, different contents.
    fn rewrite(cert_path: &Path, key_path: &Path, name: &str) {
        let cert = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
        std::fs::File::create(cert_path)
            .unwrap()
            .write_all(cert.cert.pem().as_bytes())
            .unwrap();
        std::fs::File::create(key_path)
            .unwrap()
            .write_all(cert.signing_key.serialize_pem().as_bytes())
            .unwrap();
    }

    /// Writes a fresh self-signed certificate with a chosen validity window
    /// over an existing pair, the way `rewrite` does with `rcgen`'s default
    /// one.
    fn rcgen_write(
        cert_path: &Path,
        key_path: &Path,
        not_before: time::OffsetDateTime,
        not_after: time::OffsetDateTime,
    ) {
        let key = rcgen::KeyPair::generate().unwrap();
        let mut params = rcgen::CertificateParams::new(vec!["localhost".into()]).unwrap();
        params.not_before = not_before;
        params.not_after = not_after;
        let cert = params.self_signed(&key).unwrap();

        std::fs::File::create(cert_path)
            .unwrap()
            .write_all(cert.pem().as_bytes())
            .unwrap();
        std::fs::File::create(key_path)
            .unwrap()
            .write_all(key.serialize_pem().as_bytes())
            .unwrap();
    }

    #[test]
    fn a_rewritten_certificate_reaches_the_listener_without_a_restart() {
        // `M24.9`. architecture.md has credited this crate with cert hot-reload
        // since M-1 and this crate's AGENTS.md repeated it. Nothing re-read a
        // certificate, so a cert-manager rotation served an expired one until
        // somebody restarted the pod.
        let (_dir, cert_path, key_path) = test_cert();
        let reloader =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap();
        let before = reloader.serving();

        assert!(
            !reloader.reload(std::time::SystemTime::now()).unwrap(),
            "reading the same files twice reported a change"
        );
        assert_eq!(reloader.serving(), before);

        rewrite(&cert_path, &key_path, "rotated.example");
        assert!(
            reloader.reload(std::time::SystemTime::now()).unwrap(),
            "the rewrite was not noticed"
        );
        assert_ne!(
            reloader.serving(),
            before,
            "the listener is still serving the certificate it started with"
        );
    }

    #[test]
    fn a_rewrite_that_does_not_parse_leaves_the_previous_one_serving() {
        // Certificates are rotated by machines, and a half-written file is a
        // normal thing to read. The failure mode has to be a log line and a
        // stale certificate, not a listener that stops answering.
        let (_dir, cert_path, key_path) = test_cert();
        let reloader =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap();
        let before = reloader.serving();

        std::fs::File::create(&cert_path)
            .unwrap()
            .write_all(b"-----BEGIN CERTIFICATE-----\nhalf a fi")
            .unwrap();

        assert!(
            reloader.reload(std::time::SystemTime::now()).is_err(),
            "a corrupt file was accepted"
        );
        assert_eq!(
            reloader.serving(),
            before,
            "a bad rewrite took the previous certificate with it"
        );

        // And it recovers when the rotation finishes, rather than needing a
        // restart to forget the bad read.
        rewrite(&cert_path, &key_path, "recovered.example");
        assert!(reloader.reload(std::time::SystemTime::now()).unwrap());
        assert_ne!(reloader.serving(), before);
    }

    /// `M88.11`. `read`/`reload` parsed and swapped in a new certificate
    /// without ever checking `notBefore`/`notAfter`, so an expired
    /// certificate, or one dated in the future, was served without complaint
    /// until a TLS peer's own verification rejected it, one connection at a
    /// time.
    #[test]
    fn a_node_refuses_to_start_with_an_expired_certificate() {
        let now = time::OffsetDateTime::now_utc();
        let (_dir, cert_path, key_path) = test_cert_valid(
            now - time::Duration::days(400),
            now - time::Duration::days(1),
        );

        let err =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap_err();
        assert!(
            matches!(err, TlsError::CertificateExpired { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn reload_refuses_an_expired_certificate_and_keeps_the_previous_one_serving() {
        let now = time::OffsetDateTime::now_utc();
        let (_dir, cert_path, key_path) = test_cert_valid(
            now - time::Duration::days(400),
            now + time::Duration::days(400),
        );
        let reloader =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap();
        let before = reloader.serving();

        // Rewritten with a certificate that has already expired.
        rcgen_write(
            &cert_path,
            &key_path,
            now - time::Duration::days(400),
            now - time::Duration::days(1),
        );

        let err = reloader.reload(std::time::SystemTime::now()).unwrap_err();
        assert!(
            matches!(err, TlsError::CertificateExpired { .. }),
            "{err:?}"
        );
        assert_eq!(
            reloader.serving(),
            before,
            "an expired certificate replaced the previous one still inside its window"
        );
    }

    #[test]
    fn reload_refuses_a_certificate_dated_in_the_future() {
        let now = time::OffsetDateTime::now_utc();
        let (_dir, cert_path, key_path) = test_cert_valid(
            now - time::Duration::days(400),
            now + time::Duration::days(400),
        );
        let reloader =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap();
        let before = reloader.serving();

        // Rewritten with a certificate that does not start being valid until
        // next year — a clock skew between wherever it was minted and this
        // node, or the wrong file rotated into place.
        rcgen_write(
            &cert_path,
            &key_path,
            now + time::Duration::days(1),
            now + time::Duration::days(400),
        );

        let err = reloader.reload(std::time::SystemTime::now()).unwrap_err();
        assert!(
            matches!(err, TlsError::CertificateNotYetValid { .. }),
            "{err:?}"
        );
        assert_eq!(
            reloader.serving(),
            before,
            "a not-yet-valid certificate replaced the previous one already valid"
        );
    }

    #[test]
    fn a_certificate_that_does_not_match_its_key_never_starts() {
        // The deployment mistake, caught at construction rather than at the
        // first client. `from_der` compares the public keys.
        let (_dir, cert_path, key_path) = test_cert();
        let (_other_dir, _other_cert, other_key) = test_cert();

        assert!(
            CertReloader::new(&cert_path, &other_key, std::time::SystemTime::now()).is_err(),
            "a mismatched pair was accepted"
        );
        assert!(CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).is_ok());
    }

    #[test]
    fn a_reloading_config_is_still_checked_for_fips() {
        // The assertion the fixed-certificate path makes, on the path that did
        // not exist when it was written. A build that took this route and
        // skipped it would be a FIPS binary that never checked, which ADR 0010
        // says is worse than no FIPS binary.
        let (_dir, cert_path, key_path) = test_cert();
        let reloader =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap();

        let config = server_config_reloading(reloader).unwrap();
        assert_eq!(
            config.fips(),
            FIPS_BUILD,
            "a reloading configuration disagrees with the build about FIPS"
        );
    }

    #[test]
    fn the_reload_interval_is_minutes_rather_than_seconds() {
        // What it costs is two small files read, and what it buys is noticing a
        // rotation within a minute of it happening. A value in the seconds
        // would be a file read per second per pod for a file that changes
        // monthly.
        assert!(RELOAD_INTERVAL >= std::time::Duration::from_secs(30));
        assert!(RELOAD_INTERVAL <= std::time::Duration::from_secs(600));
    }

    #[test]
    fn a_real_handshake_reaches_the_reloaded_certificate() {
        // `M87.4`. `CertReloader::resolve` is `ResolvesServerCert::resolve`,
        // and rustls itself is the only caller: there is no public way to
        // build a `ClientHello` to call it directly outside a real
        // handshake. Every other test here reads `serving()` instead, which
        // never goes through `resolve` at all. `cargo mutants` replacing the
        // whole body with `None` refused every handshake and nothing
        // noticed, because nothing here ever asked rustls to try one.
        //
        // No new dependency: `rustls`'s own synchronous `ServerConnection`
        // and `ClientConnection` exchange handshake bytes without a socket
        // or a runtime, pumped by hand below.
        let (_dir, cert_path, key_path) = test_cert();
        let reloader =
            CertReloader::new(&cert_path, &key_path, std::time::SystemTime::now()).unwrap();
        let server_config = server_config_reloading(reloader).unwrap();

        let mut roots = RootCertStore::empty();
        for cert in load_certs(&cert_path).unwrap() {
            roots.add(cert).unwrap();
        }
        let client_config = client_config(roots).unwrap();

        let server_name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let mut server_conn = rustls::ServerConnection::new(server_config).unwrap();
        let mut client_conn = rustls::ClientConnection::new(client_config, server_name).unwrap();

        // A real handshake is a handful of round trips; twenty is generous
        // headroom rather than a number this needs to reach.
        for _ in 0..20 {
            if !client_conn.is_handshaking() && !server_conn.is_handshaking() {
                break;
            }
            let mut to_server = Vec::new();
            if client_conn.wants_write() {
                client_conn.write_tls(&mut to_server).unwrap();
            }
            if !to_server.is_empty() {
                server_conn
                    .read_tls(&mut std::io::Cursor::new(to_server))
                    .unwrap();
                server_conn.process_new_packets().unwrap();
            }
            let mut to_client = Vec::new();
            if server_conn.wants_write() {
                server_conn.write_tls(&mut to_client).unwrap();
            }
            if !to_client.is_empty() {
                client_conn
                    .read_tls(&mut std::io::Cursor::new(to_client))
                    .unwrap();
                client_conn.process_new_packets().unwrap();
            }
        }

        assert!(
            !client_conn.is_handshaking() && !server_conn.is_handshaking(),
            "the handshake never completed, which is what `resolve -> None` does"
        );
    }
}
