//! Identifier newtypes.
//!
//! Every identifier that crosses a module boundary is a distinct type rather
//! than a bare `String` or `u64`. The compiler catching a swapped pair of
//! arguments is worth the extra lines, and these get passed through a lot of
//! call sites:
//!
//! ```compile_fail
//! use pgprox_core::{NodeId, TenantId};
//! fn takes_tenant(_: TenantId) {}
//! // A NodeId is not a TenantId, and this must not compile.
//! takes_tenant(NodeId::new(3));
//! ```
//!
//! [`TenantId`] and [`ServerId`] wrap `Arc<str>` because they are cloned once
//! per connection at 100k connections per node, where a `String` clone per
//! connection is a real allocation cost on a path that should not allocate.

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

/// Identifies a tenant.
///
/// Cheap to clone: the underlying string is shared.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TenantId(Arc<str>);

impl TenantId {
    /// Wraps a tenant identifier.
    pub fn new(id: impl AsRef<str>) -> Self {
        Self(Arc::from(id.as_ref()))
    }

    /// Borrows the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TenantId({})", self.0)
    }
}

/// Identifies a proxy node within the cluster.
///
/// Encoded into [`ConnId`] so a cancel request landing on any node can be
/// routed to the node that owns the connection.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NodeId(u16);

impl NodeId {
    /// Wraps a raw node number.
    #[must_use]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    /// The raw node number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self::new(0)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "node-{}", self.0)
    }
}

/// Identifies an upstream Postgres server, as `host:port`.
///
/// This is the unit the connection cap applies to. Two databases on the same
/// server share one cap.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerId(Arc<str>);

impl ServerId {
    /// Builds a server identifier from a host and port.
    pub fn new(host: impl AsRef<str>, port: u16) -> Self {
        Self(Arc::from(format!("{}:{}", host.as_ref(), port)))
    }

    /// Reads a `host:port` string back into an identifier.
    ///
    /// The inverse of [`ServerId::as_str`], so a value that has been through a
    /// config document, a log line or a URL path round-trips. Lives here rather
    /// than in each caller because two implementations of "what is a valid
    /// server address" is two chances to accept one the other would not.
    ///
    /// The port is required. Defaulting it to 5432 would let a configuration or
    /// a request silently name a different server than it appears to.
    ///
    /// ```
    /// use pgprox_core::ids::ServerId;
    ///
    /// let id = ServerId::new("db-1", 5432);
    /// assert_eq!(ServerId::parse(id.as_str()), Some(id));
    /// assert_eq!(ServerId::parse("db-1"), None, "the port is required");
    /// assert_eq!(ServerId::parse("db-1:0"), None, "nothing listens on port 0");
    /// ```
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        // Split from the right, so the colons inside an IPv6 address do not
        // confuse it.
        let (host, port) = text.rsplit_once(':')?;
        if host.is_empty() {
            return None;
        }
        let port: u16 = port.parse().ok()?;
        if port == 0 {
            return None;
        }
        Some(Self::new(host, port))
    }

    /// Borrows the underlying `host:port` string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The host half.
    ///
    /// Split from the right, so the colons in an IPv6 address stay with the
    /// host where they belong.
    #[must_use]
    pub fn host(&self) -> &str {
        self.0.rsplit_once(':').map_or(&self.0, |(host, _)| host)
    }

    /// The port half.
    ///
    /// Zero for an identifier built without one, which nothing listens on, so
    /// a caller that dials it fails rather than reaching something else.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.0
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .unwrap_or(0)
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ServerId({})", self.0)
    }
}

/// Identifies a client connection, with its owning node encoded in the value.
///
/// The proxy issues its own `BackendKeyData`, so the cancel key is ours to
/// design. Encoding the node here means a `CancelRequest` arriving at any pod
/// can be forwarded to the pod that actually owns the connection. Without it,
/// cancellation silently breaks as soon as there is a second pod.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ConnId(u64);

/// Bits reserved for the per-node counter. The node number occupies the rest.
const CONN_COUNTER_BITS: u32 = 48;
const CONN_COUNTER_MASK: u64 = (1 << CONN_COUNTER_BITS) - 1;

impl ConnId {
    /// Builds a connection identifier from its owning node and a secret.
    ///
    /// `secret` is truncated to 48 bits. It must be random: see the type-level
    /// documentation for why a counter here is a security defect rather than a
    /// style choice.
    #[must_use]
    pub const fn new(node: NodeId, secret: u64) -> Self {
        Self(((node.0 as u64) << CONN_COUNTER_BITS) | (secret & CONN_COUNTER_MASK))
    }

    /// The node that owns this connection.
    #[must_use]
    pub const fn node(self) -> NodeId {
        NodeId((self.0 >> CONN_COUNTER_BITS) as u16)
    }

    /// The per-connection secret.
    #[must_use]
    pub const fn secret(self) -> u64 {
        self.0 & CONN_COUNTER_MASK
    }

    /// Deprecated name for [`ConnId::secret`].
    ///
    /// Kept so the rename is additive rather than a breaking change, and named
    /// so a caller reading "counter" is told what it should be instead.
    #[must_use]
    #[deprecated(note = "a cancel key is a bearer token; use secret() and fill it randomly")]
    pub const fn counter(self) -> u64 {
        self.secret()
    }

    /// The raw value, as sent to the client in `BackendKeyData`.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Reconstructs a connection identifier from a raw cancel key.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

impl fmt::Display for ConnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{:x}", self.node(), self.secret())
    }
}

/// A Postgres log sequence number.
///
/// Ordering is the point of this type: replica eligibility is decided by
/// comparing a replica's replayed LSN against a session's write watermark. The
/// wire format is two 32-bit halves written as `XX/XXXXXXXX` in hex, and
/// comparing those halves as text gives the wrong answer, so they are held as
/// a single `u64`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct Lsn(u64);

impl Lsn {
    /// The zero LSN, ordering before every real one.
    pub const ZERO: Self = Self(0);

    /// Wraps a raw 64-bit LSN.
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw 64-bit value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Lsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:X}/{:X}", self.0 >> 32, self.0 & 0xFFFF_FFFF)
    }
}

/// Why an LSN string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LsnParseError {
    /// The value did not contain exactly one `/` separator.
    #[error("LSN must be in the form XX/XXXXXXXX, got {0:?}")]
    Malformed(String),
    /// One half was not valid hexadecimal.
    #[error("LSN half {half:?} is not valid hexadecimal")]
    NotHex {
        /// The half that failed to parse.
        half: String,
    },
}

impl FromStr for Lsn {
    type Err = LsnParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (hi, lo) = s
            .split_once('/')
            .ok_or_else(|| LsnParseError::Malformed(s.to_owned()))?;
        if hi.is_empty() || lo.is_empty() || lo.contains('/') {
            return Err(LsnParseError::Malformed(s.to_owned()));
        }
        let parse = |half: &str| {
            u64::from_str_radix(half, 16).map_err(|_| LsnParseError::NotHex {
                half: half.to_owned(),
            })
        };
        Ok(Self((parse(hi)? << 32) | parse(lo)?))
    }
}

/// Identifies an upstream connection pool.
///
/// One pool per distinct set of connection credentials, because a pooled
/// connection cannot be handed to a client authenticating as a different role.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct PoolKey {
    /// The upstream server, which is what the connection cap applies to.
    pub server: ServerId,
    /// The database name.
    pub database: Arc<str>,
    /// The role used to connect.
    pub user: Arc<str>,
}

impl PoolKey {
    /// Builds a pool key.
    pub fn new(server: ServerId, database: impl AsRef<str>, user: impl AsRef<str>) -> Self {
        Self {
            server,
            database: Arc::from(database.as_ref()),
            user: Arc::from(user.as_ref()),
        }
    }
}

impl fmt::Display for PoolKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}@{}", self.server, self.database, self.user)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::ServerId as ServerIdForParseTests;

    #[test]
    fn a_server_address_round_trips_through_its_string_form() {
        // The property that matters: a value that has been through a config
        // document, a log line or a URL path comes back the same. A read
        // endpoint rendering a key that a write endpoint then refuses is the
        // failure this prevents.
        for (host, port) in [("db-1", 5432_u16), ("10.0.0.9", 6432), ("[::1]", 5432)] {
            let id = ServerIdForParseTests::new(host, port);
            assert_eq!(
                ServerIdForParseTests::parse(id.as_str()),
                Some(id.clone()),
                "{host}:{port} did not round-trip"
            );
        }
    }

    #[test]
    fn an_ipv6_address_survives_its_own_colons() {
        // Split from the right, so the colons inside the address do not look
        // like the port separator.
        let id = ServerIdForParseTests::parse("[2001:db8::1]:5432").unwrap();
        assert_eq!(id.as_str(), "[2001:db8::1]:5432");
    }

    #[test]
    fn an_address_without_a_port_is_refused() {
        // Defaulting to 5432 would let a configuration or a request silently
        // name a different server than it appears to.
        assert_eq!(ServerIdForParseTests::parse("db-1"), None);
        assert_eq!(ServerIdForParseTests::parse(""), None);
    }

    #[test]
    fn a_nonsense_address_is_refused_rather_than_accepted_oddly() {
        for bad in ["db-1:", "db-1:abc", "db-1:99999", "db-1:-1", ":5432", ":"] {
            assert_eq!(
                ServerIdForParseTests::parse(bad),
                None,
                "{bad} was accepted"
            );
        }
    }

    #[test]
    fn port_zero_is_refused() {
        // Nothing listens on it, so an address naming it is a mistake rather
        // than a server.
        assert_eq!(ServerIdForParseTests::parse("db-1:0"), None);
    }

    use super::*;
    #[test]
    fn the_deprecated_counter_is_the_secret_it_was_renamed_from() {
        // `counter` could return `0` or `1`. It is deprecated in favour of
        // `secret`, kept so the rename stayed additive, and it must go on
        // answering what it always did: a caller that has not migrated would
        // otherwise start reading a constant as a cancel key.
        #[expect(deprecated, reason = "the deprecated method is what is under test")]
        {
            // The secret is truncated to 48 bits, so the expectation is the
            // truncation rather than the literal handed in.
            let id = ConnId::new(NodeId::new(7), 0xdead_beef_cafe_f00d);
            assert_eq!(id.counter(), id.secret());
            assert_eq!(id.counter(), 0xdead_beef_cafe_f00d & CONN_COUNTER_MASK);

            // Two different secrets, so a constant cannot pass.
            let other = ConnId::new(NodeId::new(7), 1);
            assert_eq!(other.counter(), 1);
            assert_ne!(id.counter(), other.counter());
        }
    }

    #[test]
    fn an_lsn_half_wider_than_thirty_two_bits_is_combined_by_or() {
        // `(hi << 32) | lo` could become `^`. The two agree whenever the halves
        // share no set bit, which is every well-formed LSN, because `lo` is at
        // most eight hex digits. The parser does not limit the width, so the
        // case that separates them is an overlong low half, and pinning it says
        // what this parser does with one rather than leaving it to chance.
        let wide: Lsn = "1/FFFFFFFFFFFFFFFF".parse().unwrap();
        assert_eq!(wide.get(), (1_u64 << 32) | u64::MAX);

        // And the ordinary case still round-trips.
        let normal: Lsn = "16/B374D848".parse().unwrap();
        assert_eq!(normal.get(), (0x16_u64 << 32) | 0xB374_D848);
    }

    #[test]
    fn tenant_id_clones_share_the_same_string() {
        let a = TenantId::new("acme");
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "acme");
        assert!(std::ptr::eq(a.as_str(), b.as_str()), "clone should share");
    }

    #[test]
    fn tenant_id_displays_and_debugs_readably() {
        let t = TenantId::new("acme");
        assert_eq!(t.to_string(), "acme");
        assert_eq!(format!("{t:?}"), "TenantId(acme)");
    }

    #[test]
    fn node_id_round_trips() {
        let n = NodeId::new(3);
        assert_eq!(n.get(), 3);
        assert_eq!(n.to_string(), "node-3");
    }

    #[test]
    fn server_id_joins_host_and_port() {
        let s = ServerId::new("db-1.internal", 5432);
        assert_eq!(s.as_str(), "db-1.internal:5432");
        assert_eq!(format!("{s:?}"), "ServerId(db-1.internal:5432)");
    }

    #[test]
    fn conn_id_round_trips_its_node() {
        // The property cancellation depends on: any node can decode the owner.
        for node in [0_u16, 1, 5, 4095, u16::MAX] {
            for counter in [0_u64, 1, 999_999, CONN_COUNTER_MASK] {
                let id = ConnId::new(NodeId::new(node), counter);
                assert_eq!(id.node().get(), node, "node lost for {node}/{counter}");
                assert_eq!(id.secret(), counter, "counter lost for {node}/{counter}");
                assert_eq!(ConnId::from_raw(id.raw()), id, "raw round-trip failed");
            }
        }
    }

    #[test]
    fn conn_id_counter_truncates_without_corrupting_the_node() {
        // A counter that overflows its field must wrap, never bleed into the
        // node bits, or a cancel request would be forwarded to the wrong pod.
        let id = ConnId::new(NodeId::new(7), CONN_COUNTER_MASK + 1);
        assert_eq!(id.node().get(), 7);
        assert_eq!(id.secret(), 0);
    }

    #[test]
    fn conn_id_displays_node_and_counter() {
        let id = ConnId::new(NodeId::new(2), 0xbeef);
        assert_eq!(id.to_string(), "node-2/beef");
    }

    #[test]
    fn lsn_orders_across_the_32_bit_boundary() {
        // The bug this guards: comparing the halves as text, or as 32-bit
        // values, makes 0/FFFFFFFF look larger than 1/00000000.
        let below: Lsn = "0/FFFFFFFF".parse().unwrap();
        let above: Lsn = "1/0".parse().unwrap();
        assert!(above > below, "{above} should sort after {below}");
        assert!(Lsn::ZERO < below);
    }

    #[test]
    fn lsn_round_trips_through_display() {
        let original = "16/B374D848";
        let lsn: Lsn = original.parse().unwrap();
        assert_eq!(lsn.to_string(), original);
        assert_eq!(lsn.get(), 0x0000_0016_B374_D848);
    }

    #[test]
    fn lsn_parse_accepts_lowercase_hex() {
        let lower: Lsn = "16/b374d848".parse().unwrap();
        let upper: Lsn = "16/B374D848".parse().unwrap();
        assert_eq!(lower, upper);
    }

    #[test]
    fn lsn_parse_rejects_malformed_input() {
        for bad in ["", "16", "/", "16/", "/848", "1/2/3"] {
            let err = bad.parse::<Lsn>().unwrap_err();
            assert!(
                matches!(err, LsnParseError::Malformed(_)),
                "{bad:?} gave {err:?}"
            );
        }
    }

    #[test]
    fn lsn_parse_rejects_non_hex() {
        let err = "zz/1".parse::<Lsn>().unwrap_err();
        assert_eq!(err, LsnParseError::NotHex { half: "zz".into() });
        assert!(err.to_string().contains("hexadecimal"));
    }

    #[test]
    fn lsn_zero_is_the_default() {
        assert_eq!(Lsn::default(), Lsn::ZERO);
        assert_eq!(Lsn::ZERO.to_string(), "0/0");
    }

    #[test]
    fn pool_key_distinguishes_user_and_database() {
        let server = ServerId::new("db-1", 5432);
        let a = PoolKey::new(server.clone(), "tenant_a", "role_a");
        let b = PoolKey::new(server.clone(), "tenant_a", "role_b");
        let c = PoolKey::new(server, "tenant_b", "role_a");

        // A pooled connection cannot be handed to a different role, so these
        // must be distinct pools.
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.to_string(), "db-1:5432/tenant_a@role_a");
    }

    #[test]
    fn pool_key_is_usable_as_a_map_key() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        let key = PoolKey::new(ServerId::new("db-1", 5432), "d", "u");
        map.insert(key.clone(), 1);
        assert_eq!(map.get(&key), Some(&1));
    }

    #[test]
    fn a_server_id_splits_back_into_its_host_and_port() {
        let id = ServerId::new("db-1", 5432);
        assert_eq!(id.host(), "db-1");
        assert_eq!(id.port(), 5432);
    }

    #[test]
    fn an_ipv6_host_keeps_its_colons() {
        // Splitting from the left would dial 2001 on port 0db8, which is a
        // failure that looks like a network problem.
        let id = ServerId::new("[2001:db8::1]", 5432);
        assert_eq!(id.host(), "[2001:db8::1]");
        assert_eq!(id.port(), 5432);
    }
}
