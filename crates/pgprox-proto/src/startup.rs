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

/// The prefix Postgres reserves for protocol extension parameters.
///
/// A client asking for one expects to be told if it did not get it, and the
/// only way a server says so is `NegotiateProtocolVersion`, which lists every
/// option it did not recognise. Silence means yes.
pub const EXTENSION_PREFIX: &str = "_pq_.";

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

    /// Parses the `options` parameter into individual `-c name=value` settings.
    ///
    /// libpq packs runtime settings here, and `search_path` is among them. That
    /// makes this correctness-relevant rather than cosmetic: `search_path` is
    /// part of the query cache key, because the same SQL resolves to different
    /// tables under different paths. See ADR 0007 and the cache module.
    ///
    /// The format is space-separated, with backslash escaping a literal space,
    /// and both `-c name=value` and a bare `name=value` are accepted because
    /// libpq emits both.
    #[must_use]
    pub fn options(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.each_option(|name, value| {
            out.push((name.to_owned(), value.to_owned()));
            // Never stop: this one wants all of them.
            false
        });
        out
    }

    /// Looks up one runtime setting from `options`.
    #[must_use]
    pub fn option(&self, name: &str) -> Option<String> {
        let mut found = None;
        self.each_option(|key, value| {
            if key == name {
                found = Some(value.to_owned());
                return true;
            }
            false
        });
        found
    }

    /// Walks the settings in `options`, stopping when `visit` returns true.
    ///
    /// Both public forms go through this. `option` used to build the whole list
    /// and throw all but one entry away, which is a `Vec` and two `String`s per
    /// setting for a single lookup. Now it stops at the one it wants and
    /// allocates only what it returns.
    fn each_option(&self, mut visit: impl FnMut(&str, &str) -> bool) {
        let Some(raw) = self.param("options") else {
            return;
        };

        // One buffer, reused across tokens, rather than one `String` each. It
        // has to be a buffer rather than a subslice because a backslash escape
        // means the token is not contiguous in the input.
        let mut token = String::new();
        let mut escaped = false;
        let mut rest = raw.chars();

        loop {
            let ch = rest.next();
            match ch {
                Some(c) if escaped => {
                    token.push(c);
                    escaped = false;
                    continue;
                }
                Some('\\') => {
                    escaped = true;
                    continue;
                }
                Some(' ') | None => {}
                Some(c) => {
                    token.push(c);
                    continue;
                }
            }

            // A token boundary, or the end of the input.
            if !token.is_empty() {
                // Strip a leading -c, which may be joined to the setting or
                // already separated by the space above.
                let setting = token.strip_prefix("-c").unwrap_or(&token).trim();
                if !setting.is_empty()
                    && setting != "-c"
                    && let Some((name, value)) = setting.split_once('=')
                    && visit(name.trim(), value)
                {
                    return;
                }
                token.clear();
            }
            if ch.is_none() {
                return;
            }
        }
    }

    /// The protocol extensions this client asked for.
    ///
    /// This proxy implements none, so every one of these is unrecognised and
    /// belongs in the `NegotiateProtocolVersion` it is answered with. `M20.3`:
    /// none of them were, so a client asking for one was told nothing, which
    /// the protocol defines as "you got it".
    pub fn extensions(&self) -> impl Iterator<Item = &str> {
        let params: &[StartupParam<'_>] = match self {
            Self::StartupMessage { params, .. } => params,
            _ => &[],
        };
        params
            .iter()
            .map(|p| p.name)
            .filter(|name| name.starts_with(EXTENSION_PREFIX))
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
/// `M20.3` gave this its second argument, and there is no one-argument form.
/// The version is not the only thing that makes an answer necessary: a client
/// that sends a `_pq_.` parameter has to be told it was not recognised, and
/// `NegotiateProtocolVersion` is the only message that says so, so one is owed
/// even at a version this proxy accepts outright. pgbouncer sends one when the
/// version is unsupported *or* the extension list is non-empty, which is the
/// condition the protocol states.
///
/// A `negotiate_version(requested)` wrapper passing `false` existed for one
/// commit and was removed: it is a default argument wearing a function name,
/// and the caller that wants it is the one caller who has no client to ask.
///
/// The minor offered is this proxy's newest, which is what the field means. It
/// is not a rejection of the version the client asked for when that version was
/// acceptable.
#[must_use]
pub fn negotiate(requested: i32, unrecognised_extension: bool) -> VersionResponse {
    let major = requested >> 16;
    let minor = requested & 0xFFFF;

    if major != 3 {
        return VersionResponse::Unsupported;
    }
    if minor == 0 && !unrecognised_extension {
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
    fn an_extension_is_recognised_by_its_prefix_and_nothing_else() {
        // `M20.3`. The prefix is reserved, and everything carrying it is a
        // request this proxy does not implement. Everything else is an ordinary
        // parameter and must not be reported as an unrecognised option.
        let body = startup_body(
            PROTOCOL_3_0,
            &[
                ("user", "acme_app"),
                ("_pq_.first", "1"),
                ("options", "-c search_path=_pq_.not_one"),
                ("_pq_.second", "2"),
            ],
        );
        let parsed = decode(&body).unwrap();

        assert_eq!(
            parsed.extensions().collect::<Vec<_>>(),
            vec!["_pq_.first", "_pq_.second"]
        );
    }

    #[test]
    fn a_client_that_asked_for_nothing_extra_is_told_nothing() {
        let body = startup_body(PROTOCOL_3_0, &[("user", "acme_app")]);
        assert_eq!(decode(&body).unwrap().extensions().count(), 0);
        assert_eq!(negotiate(PROTOCOL_3_0, false), VersionResponse::Accept);
    }

    #[test]
    fn an_extension_makes_an_answer_owed_at_a_version_that_needs_none() {
        // 3.0 is accepted outright, and an unrecognised extension still has to
        // be reported: `NegotiateProtocolVersion` is the only message that
        // says a request was not honoured, so silence means it was. pgbouncer
        // sends one when the version is unsupported *or* the extension list is
        // non-empty, which is the condition the protocol states.
        assert_eq!(
            negotiate(PROTOCOL_3_0, true),
            VersionResponse::Negotiate { minor: 0 },
            "an extension at an accepted version was answered with silence"
        );

        // And the major version still decides first: a client this codec
        // cannot frame for is refused rather than negotiated with.
        assert_eq!(negotiate(2 << 16, true), VersionResponse::Unsupported);
    }

    #[test]
    fn only_a_startup_message_carries_extensions() {
        // A cancel request and an SSL request have no parameters at all, and
        // asking either for its extensions must be empty rather than a panic.
        for body in [
            SSL_REQUEST_CODE.to_be_bytes().to_vec(),
            GSSENC_REQUEST_CODE.to_be_bytes().to_vec(),
        ] {
            assert_eq!(decode(&body).unwrap().extensions().count(), 0);
        }
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
    fn options_are_parsed_into_settings() {
        let body = startup_body(
            PROTOCOL_3_0,
            &[
                ("user", "acme_app"),
                (
                    "options",
                    "-c search_path=tenant_a -c statement_timeout=5000",
                ),
            ],
        );
        let parsed = decode(&body).unwrap();

        assert_eq!(
            parsed.options(),
            vec![
                ("search_path".to_owned(), "tenant_a".to_owned()),
                ("statement_timeout".to_owned(), "5000".to_owned()),
            ]
        );
        assert_eq!(parsed.option("search_path").as_deref(), Some("tenant_a"));
        assert_eq!(parsed.option("nonexistent"), None);
    }

    #[test]
    fn an_escaped_space_does_not_split_a_value() {
        // libpq escapes spaces in values. A naive split would cut a search_path
        // of "a, b" in half and yield a setting nobody sent, which would then
        // become part of a cache key.
        let body = startup_body(
            PROTOCOL_3_0,
            &[
                ("user", "u"),
                ("options", r"-c search_path=tenant_a,\ tenant_b"),
            ],
        );
        assert_eq!(
            decode(&body).unwrap().option("search_path").as_deref(),
            Some("tenant_a, tenant_b")
        );
    }

    #[test]
    fn a_bare_setting_without_dash_c_is_accepted() {
        // libpq emits both forms.
        let body = startup_body(
            PROTOCOL_3_0,
            &[("user", "u"), ("options", "search_path=public")],
        );
        assert_eq!(
            decode(&body).unwrap().option("search_path").as_deref(),
            Some("public")
        );
    }

    #[test]
    fn a_value_containing_an_equals_sign_keeps_it() {
        // Only the first = separates; the rest belongs to the value.
        let body = startup_body(PROTOCOL_3_0, &[("user", "u"), ("options", "-c foo=a=b=c")]);
        assert_eq!(
            decode(&body).unwrap().option("foo").as_deref(),
            Some("a=b=c")
        );
    }

    #[test]
    fn looking_up_one_setting_stops_at_it() {
        // `option` used to build the entire list and throw all but one entry
        // away. It now stops at the match, so the first of a repeated key wins,
        // which is what building the list and calling `find` on it did too.
        let body = startup_body(
            PROTOCOL_3_0,
            &[
                ("user", "u"),
                (
                    "options",
                    "-c search_path=first -c work_mem=64MB -c search_path=second",
                ),
            ],
        );
        let parsed = decode(&body).unwrap();

        assert_eq!(parsed.option("search_path").as_deref(), Some("first"));
        assert_eq!(parsed.option("work_mem").as_deref(), Some("64MB"));
        assert_eq!(parsed.option("nothing_like_this"), None);

        // And the collecting form still sees every one of them, including the
        // duplicate, because a caller listing settings wants what was sent.
        assert_eq!(parsed.options().len(), 3);
    }

    #[test]
    fn a_startup_without_options_yields_nothing_rather_than_failing() {
        let body = startup_body(PROTOCOL_3_0, &[("user", "u")]);
        assert!(decode(&body).unwrap().options().is_empty());
    }

    #[test]
    fn malformed_options_are_skipped_rather_than_rejected() {
        // A token with no = is not a setting. Refusing the whole connection
        // over one would be a worse failure than ignoring it.
        let body = startup_body(
            PROTOCOL_3_0,
            &[("user", "u"), ("options", "-c junk -c search_path=ok -c")],
        );
        assert_eq!(
            decode(&body).unwrap().options(),
            vec![("search_path".to_owned(), "ok".to_owned())]
        );
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
        assert_eq!(negotiate(PROTOCOL_3_0, false), VersionResponse::Accept);
    }

    #[test]
    fn version_3_2_is_negotiated_down_to_3_0() {
        // Postgres 18 introduced 3.2. Every 3.2-capable driver handles a
        // NegotiateProtocolVersion by design, which is what makes speaking only
        // 3.0 safe.
        assert_eq!(
            negotiate(PROTOCOL_3_2, false),
            VersionResponse::Negotiate { minor: 0 }
        );
    }

    #[test]
    fn an_unknown_minor_version_is_negotiated_rather_than_refused() {
        // A future 3.7 must be answered, not rejected. Refusing would break
        // clients that would have been perfectly happy at 3.0.
        assert_eq!(
            negotiate((3 << 16) | 7, false),
            VersionResponse::Negotiate { minor: 0 }
        );
    }

    #[test]
    fn a_different_major_version_is_unsupported() {
        // Major 2 framed messages differently, so there is nothing to negotiate.
        for major in [1, 2, 4] {
            assert_eq!(
                negotiate(major << 16, false),
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
