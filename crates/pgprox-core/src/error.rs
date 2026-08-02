//! Client-visible error taxonomy and its SQLSTATE mapping.
//!
//! Every error that can reach a client maps to a real SQLSTATE, never a generic
//! internal error. The mapping lives here, in one place, so it cannot drift per
//! crate.
//!
//! Two audiences, two renderings. [`ClientError::client_message`] is what goes
//! on the wire and says as little as possible, because the client is untrusted
//! and must not learn the upstream hostname or the internal topology.
//! `Display` is the operator-facing form and carries the detail worth logging.

use std::fmt;
use std::time::Duration;

use crate::ids::{ServerId, TenantId};

/// A five-character Postgres SQLSTATE code.
///
/// See the Postgres documentation, appendix A, for the full list.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SqlState(&'static str);

impl SqlState {
    /// `57P01`, `admin_shutdown`.
    ///
    /// Load-bearing: every mainstream driver treats this as a clean
    /// server-initiated close and reconnects, which is what drain, rebalance
    /// shedding, and socket-pressure eviction all depend on.
    pub const ADMIN_SHUTDOWN: Self = Self("57P01");
    /// `53300`, `too_many_connections`.
    pub const TOO_MANY_CONNECTIONS: Self = Self("53300");
    /// `28000`, `invalid_authorization_specification`.
    pub const INVALID_AUTHORIZATION: Self = Self("28000");
    /// `08006`, `connection_failure`.
    pub const CONNECTION_FAILURE: Self = Self("08006");
    /// `57014`, `query_canceled`.
    pub const QUERY_CANCELED: Self = Self("57014");
    /// `08P01`, `protocol_violation`.
    pub const PROTOCOL_VIOLATION: Self = Self("08P01");
    /// `XX000`, `internal_error`.
    ///
    /// For the failures that are the proxy's own and that no client action can
    /// fix. Rare on purpose: a proxy that answered `XX000` to a condition with
    /// a real code would send every operator reading it to the wrong place.
    pub const INTERNAL_ERROR: Self = Self("XX000");

    /// `feature_not_supported`. Something the client asked for that this proxy
    /// does not do, as opposed to something it got wrong.
    pub const FEATURE_NOT_SUPPORTED: Self = Self("0A000");

    /// The code as it appears in the `ErrorResponse` message.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for SqlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Why an authentication attempt was refused.
///
/// The distinction exists for logs and metrics. All of these render the same
/// way to the client, because telling an untrusted caller which part of their
/// credential was wrong is an oracle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum AuthRejection {
    /// The sidecar rejected the token.
    TokenRejected,
    /// The token expired before it reached us.
    TokenExpired,
    /// The JWT header named an algorithm outside the allowlist.
    AlgorithmNotAllowed,
    /// The token was structurally malformed.
    Malformed,
    /// The tenant exists but is not permitted to reach this database.
    NotPermitted,
}

/// An error that can reach a client.
///
/// Variants carry the detail an operator needs. None of them can carry a
/// credential, which is a property of the type rather than of the code that
/// builds it.
// PartialEq and Eq because callers assert on the error a state machine chose,
// and matches! with a wildcard body would pass on the wrong variant's payload.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// This node is draining and is no longer accepting work.
    #[error("node is draining")]
    Draining,

    /// The connection was closed to rebalance load across nodes.
    #[error("connection shed to rebalance tenant {tenant}")]
    Shed {
        /// The tenant whose connection was shed.
        tenant: TenantId,
    },

    /// The upstream server is at its configured connection cap.
    #[error("upstream {server} is at its connection cap of {cap}")]
    UpstreamAtCap {
        /// The upstream server that is full.
        server: ServerId,
        /// The configured cap.
        cap: u32,
    },

    /// No upstream connection became available in time.
    #[error("timed out after {waited:?} waiting for a connection to {server}")]
    AcquireTimeout {
        /// The upstream server that was being waited on.
        server: ServerId,
        /// How long the client waited.
        waited: Duration,
    },

    /// Authentication was refused.
    #[error("authentication refused: {0:?}")]
    AuthRefused(AuthRejection),

    /// The client did not request TLS, and this listener requires it.
    #[error("TLS is required for token authentication but the client sent no SSLRequest")]
    TlsRequired,

    /// The credential sidecar could not be reached.
    #[error("credential sidecar unavailable")]
    SidecarUnavailable,

    /// The client sent something the protocol does not allow.
    #[error("protocol violation: {0}")]
    ProtocolViolation(&'static str),

    /// The upstream connection this session was using went away.
    ///
    /// A database restart, an administrator's `pg_terminate_backend`, or a
    /// network fault between this node and the server. The client is told,
    /// because the alternative is that the proxy closes its socket in silence
    /// and every driver reports a network fault against the proxy rather than
    /// the condition that actually occurred.
    #[error("the database connection closed while serving this session")]
    UpstreamClosed,

    /// The client asked for something this proxy does not implement.
    ///
    /// Distinct from a protocol violation, which is the client being wrong.
    /// This is the client being right and the proxy being unable, and a client
    /// told the difference knows whether to fix its request or its
    /// expectations.
    ///
    /// The detail is `&'static str` and reaches the client, unlike every other
    /// detail here. That is safe because it is this proxy's own statement about
    /// its own capabilities rather than anything derived from a credential, a
    /// tenant or an upstream: there is nothing in it a client could not have
    /// worked out by trying.
    #[error("unsupported: {0}")]
    Unsupported(&'static str),

    /// The proxy could not do its job, for a reason that is its own.
    ///
    /// The detail is for the operator and never reaches the client. The one
    /// condition that has needed this so far is the system entropy source
    /// failing, where the alternatives were a panic on a connection path or a
    /// guessable cancel key.
    #[error("internal error: {0}")]
    Internal(&'static str),
}

impl ClientError {
    /// The SQLSTATE this error maps to.
    ///
    /// The table is in `standards/error-handling.md`, and the tests here assert
    /// this function matches it.
    #[must_use]
    pub const fn sqlstate(&self) -> SqlState {
        match self {
            Self::Draining | Self::Shed { .. } => SqlState::ADMIN_SHUTDOWN,
            Self::UpstreamAtCap { .. } => SqlState::TOO_MANY_CONNECTIONS,
            Self::AcquireTimeout { .. } => SqlState::QUERY_CANCELED,
            Self::AuthRefused(_) | Self::TlsRequired => SqlState::INVALID_AUTHORIZATION,
            Self::SidecarUnavailable | Self::UpstreamClosed => SqlState::CONNECTION_FAILURE,
            Self::ProtocolViolation(_) => SqlState::PROTOCOL_VIOLATION,
            Self::Unsupported(_) => SqlState::FEATURE_NOT_SUPPORTED,
            Self::Internal(_) => SqlState::INTERNAL_ERROR,
        }
    }

    /// What the client is told.
    ///
    /// Deliberately vague where `Display` is specific. The client is untrusted:
    /// it must not learn upstream hostnames, the connection cap, which tenants
    /// exist, or which part of a credential was wrong.
    #[must_use]
    pub const fn client_message(&self) -> &'static str {
        match self {
            Self::Draining | Self::Shed { .. } => {
                "terminating connection due to administrator command"
            }
            Self::UpstreamAtCap { .. } => "too many connections, please retry",
            Self::AcquireTimeout { .. } => "timed out acquiring a database connection",
            // One message for every rejection reason. Telling a caller which
            // part of their credential was wrong is an oracle.
            Self::AuthRefused(_) => "authentication failed",
            Self::TlsRequired => "SSL connection is required",
            Self::SidecarUnavailable => "authentication service unavailable",
            // Named as the database's connection rather than as an internal
            // failure: a client that reconnects gets a fresh one and carries
            // on, and that is what this message should make it do.
            Self::UpstreamClosed => "the database connection was closed, please retry",
            Self::ProtocolViolation(_) => "protocol violation",
            // The one detail that travels. See the variant's own note.
            Self::Unsupported(what) => what,
            // Vague on purpose, like the rest: which internal condition failed
            // is an operator's business and a prober's gift.
            Self::Internal(_) => "internal error",
        }
    }

    /// Whether a client should reconnect rather than treat this as fatal.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Draining
                | Self::Shed { .. }
                | Self::UpstreamAtCap { .. }
                | Self::AcquireTimeout { .. }
                | Self::SidecarUnavailable
                // A fresh connection gets a fresh upstream, so retrying is
                // exactly right.
                | Self::UpstreamClosed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_client_message_is_the_one_this_module_chose() {
        // `client_message` could return a single literal for every variant.
        // The existing test checks that no message leaks internal detail, which
        // a constant satisfies perfectly: one harmless string leaks nothing.
        // What it does instead is tell every client the same thing, so a
        // connection refused for capacity and one refused for TLS are
        // indistinguishable to the driver that has to decide whether to retry.
        assert_eq!(
            ClientError::Draining.client_message(),
            "terminating connection due to administrator command"
        );
        assert_eq!(
            ClientError::TlsRequired.client_message(),
            "SSL connection is required"
        );
        assert_eq!(
            ClientError::SidecarUnavailable.client_message(),
            "authentication service unavailable"
        );
        assert_eq!(
            ClientError::AuthRefused(AuthRejection::TokenRejected).client_message(),
            "authentication failed"
        );

        // And the messages are distinct where the causes are, which is the
        // property a single literal destroys.
        let distinct = [
            ClientError::Draining.client_message(),
            ClientError::TlsRequired.client_message(),
            ClientError::SidecarUnavailable.client_message(),
        ];
        let unique: std::collections::HashSet<&str> = distinct.iter().copied().collect();
        assert_eq!(unique.len(), distinct.len(), "two causes share a message");
    }

    const HOSTNAME: &str = "db-secret-internal.prod.example";
    const TENANT: &str = "acme-corp";

    fn one_of_each() -> Vec<ClientError> {
        let server = ServerId::new(HOSTNAME, 5432);
        vec![
            ClientError::Draining,
            ClientError::UpstreamClosed,
            ClientError::Shed {
                tenant: TenantId::new(TENANT),
            },
            ClientError::UpstreamAtCap {
                server: server.clone(),
                cap: 4096,
            },
            ClientError::AcquireTimeout {
                server,
                waited: Duration::from_secs(3),
            },
            ClientError::AuthRefused(AuthRejection::TokenExpired),
            ClientError::TlsRequired,
            ClientError::SidecarUnavailable,
            ClientError::ProtocolViolation("unexpected message after Sync"),
            ClientError::Unsupported("replication connections are not proxied"),
            ClientError::Internal("the system entropy source failed"),
        ]
    }

    #[test]
    fn mapping_matches_the_documented_table() {
        // standards/error-handling.md is the source of truth. If this test and
        // that table disagree, one of them is a bug.
        let server = ServerId::new("db", 5432);
        let cases: &[(ClientError, &str)] = &[
            (ClientError::Draining, "57P01"),
            (
                ClientError::Shed {
                    tenant: TenantId::new("t"),
                },
                "57P01",
            ),
            (
                ClientError::UpstreamAtCap {
                    server: server.clone(),
                    cap: 1,
                },
                "53300",
            ),
            (
                ClientError::AcquireTimeout {
                    server,
                    waited: Duration::ZERO,
                },
                "57014",
            ),
            (
                ClientError::AuthRefused(AuthRejection::TokenRejected),
                "28000",
            ),
            (ClientError::TlsRequired, "28000"),
            (ClientError::SidecarUnavailable, "08006"),
            (ClientError::ProtocolViolation("x"), "08P01"),
            (ClientError::Internal("x"), "XX000"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.sqlstate().as_str(), *expected, "wrong code for {err}");
        }
    }

    #[test]
    fn client_messages_leak_no_internal_detail() {
        // The client is untrusted. This is the test that stops a helpful error
        // message from handing out the fleet's topology.
        for err in one_of_each() {
            let msg = err.client_message();
            assert!(!msg.contains(HOSTNAME), "hostname leaked in {msg:?}");
            assert!(!msg.contains(TENANT), "tenant leaked in {msg:?}");
            assert!(!msg.contains("4096"), "cap leaked in {msg:?}");
            assert!(!msg.is_empty(), "empty message for {err}");
        }
    }

    #[test]
    fn every_rejection_reason_gives_the_client_the_same_message() {
        // Distinguishing them would be an oracle: a caller could learn whether
        // a tenant exists, or whether a token was merely expired.
        let reasons = [
            AuthRejection::TokenRejected,
            AuthRejection::TokenExpired,
            AuthRejection::AlgorithmNotAllowed,
            AuthRejection::Malformed,
            AuthRejection::NotPermitted,
        ];
        let messages: Vec<_> = reasons
            .iter()
            .map(|r| ClientError::AuthRefused(*r).client_message())
            .collect();
        assert!(
            messages.windows(2).all(|w| w[0] == w[1]),
            "rejection reasons are distinguishable to the client: {messages:?}"
        );
    }

    #[test]
    fn operator_display_keeps_the_detail_the_client_does_not_get() {
        // The other half of the contract: detail is not discarded, only
        // withheld from the wire.
        let err = ClientError::UpstreamAtCap {
            server: ServerId::new(HOSTNAME, 5432),
            cap: 4096,
        };
        let rendered = err.to_string();
        assert!(rendered.contains(HOSTNAME), "operator lost the hostname");
        assert!(rendered.contains("4096"), "operator lost the cap");
    }

    #[test]
    fn shed_and_drain_use_the_code_drivers_reconnect_on() {
        // 57P01 is what makes rebalancing invisible to applications. A change
        // here silently turns shedding into client-visible errors.
        assert_eq!(ClientError::Draining.sqlstate(), SqlState::ADMIN_SHUTDOWN);
        assert_eq!(SqlState::ADMIN_SHUTDOWN.as_str(), "57P01");
        assert_eq!(SqlState::ADMIN_SHUTDOWN.to_string(), "57P01");
    }

    #[test]
    fn transient_conditions_are_retryable_and_permanent_ones_are_not() {
        assert!(ClientError::Draining.is_retryable());
        assert!(ClientError::SidecarUnavailable.is_retryable());
        assert!(
            ClientError::UpstreamAtCap {
                server: ServerId::new("db", 5432),
                cap: 1,
            }
            .is_retryable()
        );
        assert!(!ClientError::TlsRequired.is_retryable());
        assert!(!ClientError::AuthRefused(AuthRejection::Malformed).is_retryable());
        assert!(!ClientError::ProtocolViolation("x").is_retryable());
        // The proxy's own failures are not the client's to retry: reconnecting
        // into the same broken node just repeats it.
        assert!(!ClientError::Internal("x").is_retryable());
    }

    #[test]
    fn sqlstate_codes_are_five_characters() {
        for err in one_of_each() {
            assert_eq!(err.sqlstate().as_str().len(), 5, "bad code for {err}");
        }
    }
}
