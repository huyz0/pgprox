//! Opening the sockets everything else is written against.
//!
//! `pgprox-session` describes what it needs from an upstream: a stream, and
//! somebody to do SCRAM arithmetic. Both are traits there because that crate
//! may not depend on `pgprox-tls` or on `pgprox-auth`. This is where they are
//! filled in, which is the whole reason a composition root exists.
//!
//! # TLS to the backend is not optional by omission
//!
//! A `Backend` says whether it wants TLS, and there is deliberately no "verify
//! nothing" mode. A backend that asks for `Verified` and cannot get a verified
//! connection is refused rather than downgraded, because a proxy that quietly
//! fell back to plaintext would move a tenant's credentials onto the wire in
//! the clear and report nothing.

use std::sync::Arc;

use pgprox_auth::scram;
use pgprox_core::auth::{Backend, TlsMode};
use pgprox_core::pool::PoolError;
use pgprox_core::secret::SecretString;
use pgprox_session::connect::Upstreamed;
use pgprox_session::connect::{Upstream, UpstreamScram};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

/// A stream to a backend, encrypted or not.
///
/// An enum rather than a boxed trait object: there are two cases, they are
/// known at compile time, and the relay loop is a declared hot path where a
/// virtual call per read would be paid on every byte.
#[derive(Debug)]
pub enum Stream {
    /// Plaintext, for a backend inside a trusted boundary.
    Plain(TcpStream),
    /// TLS, with the certificate verified against the configured roots.
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl tokio::io::AsyncRead for Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_read(cx, buf),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_write(cx, buf),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_flush(cx),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => std::pin::Pin::new(io).poll_shutdown(cx),
            Self::Tls(io) => std::pin::Pin::new(io.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Dials backends over TCP, with TLS where the backend asks for it.
///
/// `Clone` because a node has one TLS configuration and several things that
/// dial with it: the pool's connector and one replica prober per replica set.
/// The configuration itself is shared rather than copied.
#[derive(Debug, Clone)]
pub struct TcpUpstream {
    tls: Arc<ClientConfig>,
}

impl TcpUpstream {
    /// A dialer using this client configuration for verified connections.
    #[must_use]
    pub const fn new(tls: Arc<ClientConfig>) -> Self {
        Self { tls }
    }
}

#[async_trait::async_trait]
impl Upstream for TcpUpstream {
    type Stream = Stream;

    async fn dial(&self, backend: &Backend) -> Result<Self::Stream, PoolError> {
        let refused = |reason: String| PoolError::ConnectFailed {
            server: backend.server.clone(),
            reason,
        };

        // The server name is checked before the socket is opened. A name TLS
        // cannot verify cannot be verified after connecting either, and
        // dialling first means waiting out a connection attempt to learn
        // something already known.
        let name = match backend.tls {
            TlsMode::Verified => Some(
                ServerName::try_from(backend.server.host().to_owned()).map_err(|_| {
                    refused(format!("{} is not a valid server name", backend.server))
                })?,
            ),
            _ => None,
        };

        let socket = TcpStream::connect((backend.server.host(), backend.server.port()))
            .await
            .map_err(|err| refused(err.to_string()))?;
        // Postgres is request-response on a latency-sensitive path, so waiting
        // to coalesce a write with one that has not been asked for yet costs
        // every round trip.
        let _ = socket.set_nodelay(true);

        match backend.tls {
            TlsMode::Disabled => Ok(Stream::Plain(socket)),
            TlsMode::Verified => {
                let name = name.ok_or_else(|| refused("no server name to verify".to_owned()))?;
                let stream = TlsConnector::from(Arc::clone(&self.tls))
                    .connect(name, socket)
                    .await
                    .map_err(|err| refused(format!("TLS handshake failed: {err}")))?;
                Ok(Stream::Tls(Box::new(stream)))
            }
            // TlsMode is non_exhaustive. A mode added later must not silently
            // take the plaintext path, which is the one that puts a tenant's
            // credentials on the wire in the clear.
            _ => Err(refused(
                "this proxy does not implement the requested TLS mode".to_owned(),
            )),
        }
    }

    fn scram(&self) -> Box<dyn UpstreamScram> {
        Box::new(ClientScram::default())
    }
}

/// The client half of SCRAM, as `pgprox-session`'s trait wants it.
///
/// The exchange itself is `pgprox_auth::ClientExchange` and this is a wrapper
/// over it. The split is a layering one rather than a design one:
/// [`UpstreamScram`] lives in `pgprox-session`, which `pgprox-auth` may not
/// depend on, so the type is shared and the trait implementation is not.
///
/// It was the exchange until `M32.1`, when `bin/pgload` needed the same three
/// messages to reach a pooler that asks for SCRAM. Two implementations of an
/// authentication exchange is a worse version of the mistake
/// `pgprox_core::sql` exists to prevent.
#[derive(Debug, Default)]
pub struct ClientScram(scram::ClientExchange);

impl UpstreamScram for ClientScram {
    fn client_first(&mut self, user: &str) -> String {
        self.0.client_first(user)
    }

    fn client_final(
        &mut self,
        password: &SecretString,
        server_first: &str,
    ) -> Result<String, String> {
        self.0
            .client_final(password, server_first)
            .map_err(|err| err.to_string())
    }

    fn verify(&mut self, server_final: &str) -> Result<(), String> {
        self.0.verify(server_final).map_err(|err| err.to_string())
    }
}

/// Says goodbye to connections the pool has finished with, then drops them.
///
/// `M20.4`. The reaper decides which connections go while holding a lock it may
/// not await inside, so it hands the sockets back and this is where they are
/// told. Postgres logs a client that vanishes without a `Terminate`, and this
/// node reaps idle connections after thirty seconds with `min_pool` at zero, on
/// purpose: reaping is the steady state here, so without this every routine
/// close is a line on the database that reads like a crash.
///
/// Sequential rather than concurrent. These are connections nobody is waiting
/// on, and a `Terminate` is five bytes into a socket that is already writable;
/// spawning for them would trade a real cost against a saving nobody would
/// measure.
pub async fn retire(connections: Vec<Upstreamed<Stream>>) {
    for mut connection in connections {
        connection.goodbye().await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    // The trait, for the `encode` and `decode` the SCRAM tests below call. It
    // was imported at module level until `M32.1` moved the exchange into
    // `pgprox-auth`, after which nothing outside these tests encodes anything.
    use base64::Engine as _;
    use pgprox_core::ids::ServerId;
    use tokio_rustls::rustls::RootCertStore;

    fn dialer() -> TcpUpstream {
        TcpUpstream::new(pgprox_tls::client_config(RootCertStore::empty()).unwrap())
    }

    fn backend(tls: TlsMode, server: ServerId) -> Backend {
        Backend {
            server,
            database: "acme".into(),
            user: "acme_app".into(),
            password: SecretString::new("hunter2"),
            tls,
        }
    }

    #[tokio::test]
    async fn a_plaintext_backend_is_dialled_over_tcp() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let stream = dialer()
            .dial(&backend(
                TlsMode::Disabled,
                ServerId::new("127.0.0.1", addr.port()),
            ))
            .await
            .unwrap();
        assert!(matches!(stream, Stream::Plain(_)));
    }

    #[tokio::test]
    async fn a_backend_that_closes_immediately_still_dials() {
        // The connect-refused path is deliberately not tested here. This
        // environment drops connections to a closed port rather than refusing
        // them, so a test that dialled one would hang for two minutes and take
        // the tier-1 budget with it. What is asserted instead is that a socket
        // which opens and closes is still a successful dial, because the
        // failure belongs to the handshake that follows.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                drop(socket);
            }
        });

        assert!(
            dialer()
                .dial(&backend(
                    TlsMode::Disabled,
                    ServerId::new("127.0.0.1", addr.port())
                ))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_backend_that_wants_tls_and_gets_a_plain_socket_is_refused() {
        // The failure that must not be a downgrade. A proxy that fell back to
        // plaintext would put a tenant's credentials on the wire and say
        // nothing.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                // Answer the TLS ClientHello with nonsense.
                use tokio::io::AsyncWriteExt;
                let _ = socket.write_all(b"not a tls record").await;
            }
        });

        assert!(
            dialer()
                .dial(&backend(
                    TlsMode::Verified,
                    ServerId::new("127.0.0.1", addr.port())
                ))
                .await
                .is_err(),
            "a backend asking for TLS was given something else"
        );
    }

    #[tokio::test]
    async fn a_server_name_tls_cannot_verify_is_refused_before_dialling() {
        // An empty host cannot be a certificate subject, so there is nothing
        // to verify against and no safe way to proceed.
        let err = dialer()
            .dial(&backend(TlsMode::Verified, ServerId::new(" ", 5432)))
            .await
            .unwrap_err();
        assert!(matches!(err, PoolError::ConnectFailed { .. }));

        // The reason, which is what makes the name of this test true.
        // `M17.4`: deleting the `Verified` arm that resolves the name survived,
        // because without it the dial fails anyway, just later and for a
        // different reason. "Before dialling" is the whole point: a name TLS
        // cannot verify cannot be verified after connecting either, so waiting
        // out a connection attempt buys nothing, and on a host that blackholes
        // rather than refuses that wait is minutes long.
        let PoolError::ConnectFailed { reason, .. } = &err else {
            unreachable!("the assertion above")
        };
        assert!(
            reason.contains("not a valid server name"),
            "the dial failed for the wrong reason: {reason}"
        );
    }

    #[test]
    fn the_scram_exchange_refuses_a_server_final_it_never_challenged() {
        let mut exchange = ClientScram::default();
        assert!(exchange.verify("v=U0lHTg==").is_err());
    }

    #[test]
    fn the_scram_exchange_produces_a_client_first_naming_the_user() {
        let mut exchange = ClientScram::default();
        let first = exchange.client_first("acme_app");

        assert!(first.starts_with("n,,n=acme_app,r="), "{first}");
        assert!(
            first.len() > "n,,n=acme_app,r=".len(),
            "the client-first message carried no nonce"
        );
    }

    #[test]
    fn two_exchanges_do_not_share_a_nonce() {
        // A repeated client nonce lets a recorded exchange be replayed.
        let mut one = ClientScram::default();
        let mut two = ClientScram::default();

        assert_ne!(one.client_first("u"), two.client_first("u"));
    }

    #[test]
    fn a_malformed_server_first_is_an_error_rather_than_a_panic() {
        let mut exchange = ClientScram::default();
        exchange.client_first("acme_app");

        assert!(
            exchange
                .client_final(&SecretString::new("hunter2"), "not a scram message")
                .is_err()
        );
    }

    #[test]
    fn the_client_half_and_the_server_half_agree() {
        // Both halves of SCRAM exist in this project: the client one here, and
        // the server-side arithmetic in pgprox-auth that pgprox-session's
        // static-user path uses. This runs one against the other, which is the
        // only way to find out that they agree about the AuthMessage.
        use base64::engine::general_purpose::STANDARD as BASE64;

        let password = SecretString::new("hunter2");
        let salt = b"a-sixteen-byte!!";
        let iterations = 4096;

        let mut exchange = ClientScram::default();
        let client_first = exchange.client_first("acme_app");
        let client_nonce = client_first
            .rsplit_once("r=")
            .expect("client-first carries a nonce")
            .1
            .to_owned();

        let server_first = format!(
            "r={client_nonce}SERVERNONCE,s={},i={iterations}",
            BASE64.encode(salt)
        );
        let client_final = exchange
            .client_final(&password, &server_first)
            .expect("a well-formed server-first is answerable");

        // What the server would compute, from what it stores.
        let keys = scram::ScramKeys::derive(password.expose().as_bytes(), salt, iterations)
            .expect("the keys derive");
        let without_proof = client_final
            .rsplit_once(",p=")
            .expect("client-final carries a proof");
        let auth_message = scram::auth_message(
            &scram::client_first_bare("acme_app", &client_nonce),
            &server_first,
            without_proof.0,
        );
        let proof = BASE64.decode(without_proof.1).expect("the proof is base64");

        scram::verify_client_proof(&proof, &keys.stored_key, &auth_message)
            .expect("the server rejected a proof this client computed");

        // And the other direction: the client checks the server back.
        let signature = scram::server_signature(&keys, &auth_message);
        exchange
            .verify(&format!("v={}", BASE64.encode(signature)))
            .expect("the client rejected a signature the server computed");
    }

    #[test]
    fn a_server_signature_that_does_not_match_is_refused() {
        // The direction that authenticates the server. Skipping it turns SCRAM
        // into a one-way check and lets anything that answered the socket
        // through.
        use base64::engine::general_purpose::STANDARD as BASE64;

        let mut exchange = ClientScram::default();
        let client_first = exchange.client_first("acme_app");
        let nonce = client_first.rsplit_once("r=").unwrap().1.to_owned();
        let server_first = format!("r={nonce}X,s={},i=4096", BASE64.encode(b"salt-salt-salt!!"));

        exchange
            .client_final(&SecretString::new("hunter2"), &server_first)
            .unwrap();

        assert!(
            exchange
                .verify(&format!("v={}", BASE64.encode([0_u8; 32])))
                .is_err(),
            "a wrong server signature was accepted"
        );
    }

    #[test]
    fn a_server_first_with_a_nonce_that_is_not_ours_is_refused() {
        // A server that replaced the client's nonce rather than extending it
        // is replaying somebody else's exchange.
        use base64::engine::general_purpose::STANDARD as BASE64;

        let mut exchange = ClientScram::default();
        exchange.client_first("acme_app");

        let forged = format!(
            "r=SOMEONEELSE,s={},i=4096",
            BASE64.encode(b"salt-salt-salt!!")
        );
        assert!(
            exchange
                .client_final(&SecretString::new("hunter2"), &forged)
                .is_err()
        );
    }

    #[tokio::test]
    async fn bytes_travel_through_a_plaintext_stream_both_ways() {
        // The enum exists so the relay loop pays no virtual call per read. It
        // is also four trait methods that forward, and a forwarding method
        // that forwards to the wrong half is the kind of bug that only shows
        // up as a hang.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 5];
            socket.read_exact(&mut buf).await.unwrap();
            socket.write_all(&buf).await.unwrap();
        });

        let mut stream = dialer()
            .dial(&backend(
                TlsMode::Disabled,
                ServerId::new("127.0.0.1", addr.port()),
            ))
            .await
            .unwrap();

        stream.write_all(b"hello").await.unwrap();
        stream.flush().await.unwrap();

        let mut echoed = [0_u8; 5];
        stream.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"hello");

        stream.shutdown().await.unwrap();
    }

    #[test]
    fn the_dialer_supplies_a_fresh_scram_exchange_each_time() {
        // Shared state between two connections' exchanges would leak one
        // connection's nonce into another's proof.
        let dialer = dialer();
        let mut first = dialer.scram();
        let mut second = dialer.scram();

        assert_ne!(first.client_first("u"), second.client_first("u"));
    }

    #[tokio::test]
    async fn a_verified_backend_is_dialled_over_tls() {
        // `M17.4`. Every `TlsMode::Verified` test in this file asserted a
        // *failure*: a name TLS cannot verify, and a server that answers the
        // ClientHello with nonsense. Nothing had ever proved the path works,
        // so deleting the arm that performs the handshake survived, and what
        // that leaves is a proxy that refuses every verified backend with "an
        // upstream TLS mode this build does not know" while the mode is one it
        // has known since M1.
        //
        // The certificate is generated here and trusted here, which is the
        // whole point: a client config that trusts nothing, which is what the
        // rest of this module uses, cannot complete a handshake and so cannot
        // tell a working path from a missing one.
        let generated = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert =
            tokio_rustls::rustls::pki_types::CertificateDer::from(generated.cert.der().to_vec());
        let key = tokio_rustls::rustls::pki_types::PrivateKeyDer::try_from(
            generated.signing_key.serialize_der(),
        )
        .expect("the generated key is a valid DER private key");

        let mut roots = RootCertStore::empty();
        roots
            .add(cert.clone())
            .expect("the generated cert is valid");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(
            pgprox_tls::server_config(vec![cert], key).expect("a valid server config"),
        );
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                // Held rather than dropped: the handshake completes on this
                // side before the dialler's future resolves.
                let _ = acceptor.accept(socket).await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });

        let dialler = TcpUpstream::new(pgprox_tls::client_config(roots).unwrap());
        let stream = dialler
            .dial(&backend(
                TlsMode::Verified,
                ServerId::new("localhost", addr.port()),
            ))
            .await
            .expect("a verified backend with a trusted certificate was refused");
        assert!(
            matches!(stream, Stream::Tls(_)),
            "a verified backend was given a plaintext socket"
        );
    }
}
