//! The startup phase.
//!
//! Before any tagged message flows, a client sends one untagged message whose
//! first four body bytes are a version number or a magic code. Dispatching on
//! that code is the first decision the proxy makes about a connection.

use pgprox_core::ids::ConnId;

use crate::backend::conn_id_from_key;
use crate::read::{FieldError, Reader};

/// Magic code requesting TLS. Postgres calls this `SSLRequest`.
pub const SSL_REQUEST_CODE: i32 = 80_877_103;

/// Magic code requesting GSSAPI encryption.
pub const GSSENC_REQUEST_CODE: i32 = 80_877_104;

/// Magic code for a cancellation request.
pub const CANCEL_REQUEST_CODE: i32 = 80_877_102;

/// A parameter from the startup packet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartupParam<'a> {
    /// The parameter name.
    pub name: &'a str,
    /// Its value.
    pub value: &'a str,
}

/// What a client sent first.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Startup<'a> {
    /// The client wants TLS. Answer `S` to accept or `N` to refuse.
    SslRequest,
    /// The client wants GSSAPI encryption. Answer `N`; it is not supported.
    GssEncRequest,
    /// The client is cancelling a query on another connection.
    ///
    /// The key was issued by this proxy, so it decodes back to the connection
    /// and, crucially, to the node that owns it.
    CancelRequest {
        /// The connection being cancelled.
        conn: ConnId,
    },
    /// A real startup packet.
    StartupMessage {
        /// The protocol version the client asked for.
        version: i32,
        /// The parameters it sent, in order.
        params: Vec<StartupParam<'a>>,
    },
}

impl Startup<'_> {
    /// The `user` parameter, which Postgres requires.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.param("user")
    }

    /// The `database` parameter, defaulting to the user name as Postgres does.
    #[must_use]
    pub fn database(&self) -> Option<&str> {
        self.param("database").or_else(|| self.user())
    }

    /// Looks up a startup parameter.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        match self {
            Self::StartupMessage { params, .. } => {
                params.iter().find(|p| p.name == name).map(|p| p.value)
            }
            _ => None,
        }
    }
}

/// How the proxy should answer a requested protocol version.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum VersionResponse {
    /// Proceed at the version the client asked for.
    Accept,
    /// Send `NegotiateProtocolVersion` offering this minor version instead.
    Negotiate {
        /// The highest minor version the proxy supports.
        minor: i32,
    },
    /// Refuse: the major version is one this proxy does not speak.
    Unsupported,
}

/// Decides how to answer a requested protocol version.
///
/// The proxy speaks 3.0. A client asking for 3.2, which Postgres 18 introduced,
/// is offered 3.0 through `NegotiateProtocolVersion`, and every 3.2-capable
/// driver handles that by design. Anything outside major version 3 is refused,
/// since the message framing itself would differ.
#[must_use]
pub fn negotiate_version(requested: i32) -> VersionResponse {
    let major = requested >> 16;
    let minor = requested & 0xFFFF;

    if major != 3 {
        return VersionResponse::Unsupported;
    }
    if minor == 0 {
        return VersionResponse::Accept;
    }
    VersionResponse::Negotiate { minor: 0 }
}

/// Why a startup packet could not be decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartupError {
    /// A field could not be read.
    #[error(transparent)]
    Field(#[from] FieldError),
    /// The parameter list ended without its terminating empty key.
    #[error("startup parameters were not terminated")]
    Unterminated,
}

/// Decodes the body of the first untagged message.
///
/// `body` excludes the length prefix, as produced by
/// [`crate::frame::decode_untagged`].
///
/// # Errors
///
/// Fails when the packet is malformed.
pub fn decode(body: &[u8]) -> Result<Startup<'_>, StartupError> {
    let mut r = Reader::new(body);
    let code = r.i32("protocol_version")?;

    Ok(match code {
        SSL_REQUEST_CODE => Startup::SslRequest,
        GSSENC_REQUEST_CODE => Startup::GssEncRequest,
        CANCEL_REQUEST_CODE => Startup::CancelRequest {
            conn: conn_id_from_key(r.i32("process_id")?, r.i32("secret_key")?),
        },
        version => {
            let mut params = Vec::new();
            loop {
                if r.is_empty() {
                    return Err(StartupError::Unterminated);
                }
                let name = r.cstr("parameter_name")?;
                if name.is_empty() {
                    break;
                }
                params.push(StartupParam {
                    name,
                    value: r.cstr("parameter_value")?,
                });
            }
            Startup::StartupMessage { version, params }
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::backend::key_from_conn_id;
    use crate::encode::{PROTOCOL_3_0, PROTOCOL_3_2};
    use pgprox_core::ids::NodeId;

    fn startup_body(version: i32, params: &[(&str, &str)]) -> Vec<u8> {
        let mut body = version.to_be_bytes().to_vec();
        for (name, value) in params {
            body.extend_from_slice(name.as_bytes());
            body.push(0);
            body.extend_from_slice(value.as_bytes());
            body.push(0);
        }
        body.push(0);
        body
    }

    #[test]
    fn ssl_request_is_recognised() {
        let body = SSL_REQUEST_CODE.to_be_bytes();
        assert_eq!(decode(&body).unwrap(), Startup::SslRequest);
    }

    #[test]
    fn gssenc_request_is_recognised() {
        // Recognised so it can be refused with N, rather than being mistaken
        // for a startup packet with an absurd version.
        let body = GSSENC_REQUEST_CODE.to_be_bytes();
        assert_eq!(decode(&body).unwrap(), Startup::GssEncRequest);
    }

    #[test]
    fn a_cancel_request_yields_the_owning_node() {
        // The property that makes cancellation work across pods: any node can
        // decode which node owns the connection.
        let conn = ConnId::new(NodeId::new(4), 0xABCD);
        let (pid, secret) = key_from_conn_id(conn);

        let mut body = CANCEL_REQUEST_CODE.to_be_bytes().to_vec();
        body.extend_from_slice(&pid.to_be_bytes());
        body.extend_from_slice(&secret.to_be_bytes());

        let Startup::CancelRequest { conn: decoded } = decode(&body).unwrap() else {
            unreachable!()
        };
        assert_eq!(decoded, conn);
        assert_eq!(decoded.node(), NodeId::new(4));
    }

    #[test]
    fn a_truncated_cancel_request_is_an_error() {
        let body = CANCEL_REQUEST_CODE.to_be_bytes();
        assert!(decode(&body).is_err());
    }

    #[test]
    fn a_startup_message_yields_its_parameters() {
        let body = startup_body(
            PROTOCOL_3_0,
            &[
                ("user", "acme_app"),
                ("database", "tenant_acme"),
                ("application_name", "psql"),
            ],
        );

        let parsed = decode(&body).unwrap();
        assert_eq!(parsed.user(), Some("acme_app"));
        assert_eq!(parsed.database(), Some("tenant_acme"));
        assert_eq!(parsed.param("application_name"), Some("psql"));
        assert_eq!(parsed.param("nonexistent"), None);
    }

    #[test]
    fn database_defaults_to_the_user_name() {
        // Postgres behaviour: omitting database means connect to one named
        // after the user. Getting this wrong sends a tenant to the wrong place.
        let body = startup_body(PROTOCOL_3_0, &[("user", "acme_app")]);
        let parsed = decode(&body).unwrap();
        assert_eq!(parsed.database(), Some("acme_app"));
    }

    #[test]
    fn a_startup_message_with_no_parameters_decodes() {
        let body = startup_body(PROTOCOL_3_0, &[]);
        let Startup::StartupMessage { version, params } = decode(&body).unwrap() else {
            unreachable!()
        };
        assert_eq!(version, PROTOCOL_3_0);
        assert!(params.is_empty());
    }

    #[test]
    fn unterminated_parameters_are_an_error() {
        // Without the terminator check this would loop or read past the body.
        let mut body = PROTOCOL_3_0.to_be_bytes().to_vec();
        body.extend_from_slice(b"user\x00acme\x00");
        assert_eq!(decode(&body).unwrap_err(), StartupError::Unterminated);
    }

    #[test]
    fn a_parameter_with_no_value_is_an_error() {
        let mut body = PROTOCOL_3_0.to_be_bytes().to_vec();
        body.extend_from_slice(b"user\x00");
        assert!(decode(&body).is_err());
    }

    #[test]
    fn version_3_0_is_accepted() {
        assert_eq!(negotiate_version(PROTOCOL_3_0), VersionResponse::Accept);
    }

    #[test]
    fn version_3_2_is_negotiated_down_to_3_0() {
        // Postgres 18 introduced 3.2. Every 3.2-capable driver handles a
        // NegotiateProtocolVersion by design, which is what makes speaking only
        // 3.0 safe.
        assert_eq!(
            negotiate_version(PROTOCOL_3_2),
            VersionResponse::Negotiate { minor: 0 }
        );
    }

    #[test]
    fn an_unknown_minor_version_is_negotiated_rather_than_refused() {
        // A future 3.7 must be answered, not rejected. Refusing would break
        // clients that would have been perfectly happy at 3.0.
        assert_eq!(
            negotiate_version((3 << 16) | 7),
            VersionResponse::Negotiate { minor: 0 }
        );
    }

    #[test]
    fn a_different_major_version_is_unsupported() {
        // Major 2 framed messages differently, so there is nothing to negotiate.
        for major in [1, 2, 4] {
            assert_eq!(
                negotiate_version(major << 16),
                VersionResponse::Unsupported,
                "major {major}"
            );
        }
    }

    #[test]
    fn magic_codes_are_not_mistaken_for_versions() {
        // Each magic code has a huge major number, so a dispatch that checked
        // the version before the codes would call them unsupported.
        for code in [SSL_REQUEST_CODE, GSSENC_REQUEST_CODE, CANCEL_REQUEST_CODE] {
            assert_ne!(code >> 16, 3, "code {code} collides with major 3");
        }
    }

    #[test]
    fn accessors_return_nothing_for_non_startup_variants() {
        let ssl = Startup::SslRequest;
        assert_eq!(ssl.user(), None);
        assert_eq!(ssl.database(), None);
        assert_eq!(ssl.param("user"), None);
    }

    #[test]
    fn decoding_never_panics_on_arbitrary_input() {
        let mut seed = 0xF0F0_1234_5678_9ABC_u64;
        for _ in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = usize::try_from(seed % 40).unwrap();
            let body: Vec<u8> = (0..len)
                .map(|i| u8::try_from((seed >> (i % 8 * 8)) & 0xFF).unwrap())
                .collect();
            let _ = decode(&body);
        }
    }
}
