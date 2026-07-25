//! Span names, log fields, and what may never appear in either.
//!
//! # Names are code, attributes are data
//!
//! A span named for the tenant, or for the query, is a span name with unbounded
//! cardinality, and a tracing backend groups by name exactly as Prometheus
//! groups by label. So names come from a fixed list here, and everything that
//! varies goes in an attribute.
//!
//! # What may never be recorded
//!
//! Credentials, obviously, but the interesting case is SQL. Query text routinely
//! carries customer data in literals: `WHERE email = '...'`, `SET
//! search_path = tenant_...`, an `INSERT` with a whole row in it. Logging it by
//! default would put customer data in a log aggregator that has a different
//! retention policy and a different set of people with access.
//!
//! So query text is `debug` only *and* opt-in per tenant, which are two
//! separate switches on purpose. Turning up the log level fleet-wide during an
//! incident must not start recording every tenant's data as a side effect.
//!
//! # Why this is a module rather than a convention
//!
//! [`redact`] is what a caller reaches for instead of formatting a value
//! themselves, and [`is_recordable`] is what a test asserts. A convention in a
//! document is checked when somebody remembers; a function is checked by the
//! compiler being the only way to do the thing.

use std::fmt;

/// The spans the proxy emits.
///
/// A fixed list, because a tracing backend groups by name and a name built from
/// data is a name with unbounded cardinality. Anything that varies is an
/// attribute.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Span {
    /// A client connection, from accept to close.
    Connection,
    /// Authenticating one client, including the sidecar call.
    Authenticate,
    /// One transaction, from first statement to `ReadyForQuery('I')`.
    Transaction,
    /// Acquiring an upstream connection.
    Acquire,
    /// One gossip round.
    Gossip,
    /// One configuration reload.
    ConfigReload,
    /// One admin request.
    Admin,
}

impl Span {
    /// The span name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connection => "pgprox.connection",
            Self::Authenticate => "pgprox.authenticate",
            Self::Transaction => "pgprox.transaction",
            Self::Acquire => "pgprox.acquire",
            Self::Gossip => "pgprox.gossip",
            Self::ConfigReload => "pgprox.config_reload",
            Self::Admin => "pgprox.admin",
        }
    }

    /// Every span, for the tests that hold the conventions in place.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Connection,
            Self::Authenticate,
            Self::Transaction,
            Self::Acquire,
            Self::Gossip,
            Self::ConfigReload,
            Self::Admin,
        ]
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Attribute and log field names.
///
/// Named constants rather than string literals at call sites, so a field is
/// spelled the same way in every span it appears in. A query that filters on
/// `tenant_id` and misses the spans that wrote `tenant` is a query that returns
/// a confident, wrong answer.
pub mod field {
    /// Which tenant. An attribute, never part of a span name.
    pub const TENANT: &str = "tenant_id";
    /// Which node.
    pub const NODE: &str = "node_id";
    /// Which client connection.
    pub const CONN: &str = "conn_id";
    /// Which pool.
    pub const POOL_KEY: &str = "pool_key";
    /// Where a statement was routed.
    pub const ROUTE: &str = "route";
    /// Why a session was pinned.
    pub const PIN_REASON: &str = "pin_reason";
    /// The SQLSTATE of an error shown to a client.
    pub const SQLSTATE: &str = "sqlstate";
    /// How long something took, in milliseconds.
    pub const DURATION_MS: &str = "duration_ms";

    /// Every field, for the tests that hold the conventions in place.
    pub const ALL: &[&str] = &[
        TENANT,
        NODE,
        CONN,
        POOL_KEY,
        ROUTE,
        PIN_REASON,
        SQLSTATE,
        DURATION_MS,
    ];
}

/// What replaces a value that must not be recorded.
///
/// The same marker `SecretString` uses, so a leak looks the same wherever it
/// surfaces and a search for it finds every case.
pub const REDACTED: &str = "[redacted]";

/// Field names that must never carry a real value.
///
/// Checked by [`is_recordable`] rather than trusted. A field called `password`
/// is not something anyone adds on purpose; it arrives through a struct that
/// grew a field, or a map that got serialised whole.
const FORBIDDEN: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "jwt",
    "authorization",
    "credential",
    "api_key",
    "apikey",
    "private_key",
];

/// Whether a field may be recorded with its real value.
///
/// Matches on substring rather than equality, because the field that leaks is
/// never called `password`. It is called `upstream_password`, or
/// `grant.password`, or `auth_token`.
#[must_use]
pub fn is_recordable(field: &str) -> bool {
    let lower = field.to_ascii_lowercase();
    !FORBIDDEN.iter().any(|forbidden| lower.contains(forbidden))
}

/// The value to record for a field.
///
/// Returns [`REDACTED`] for anything [`is_recordable`] refuses. Reach for this
/// instead of formatting a value directly: a convention is checked when
/// somebody remembers, and a function is checked every time it is called.
#[must_use]
pub fn redact<'a>(field: &str, value: &'a str) -> &'a str {
    if is_recordable(field) {
        value
    } else {
        REDACTED
    }
}

/// Whether query text may be recorded.
///
/// Two switches, deliberately. `debug` alone is not enough because turning the
/// log level up fleet-wide during an incident must not start recording every
/// tenant's data as a side effect; the per-tenant opt-in alone is not enough
/// because a tenant who agreed once should not have their SQL in every
/// production log line forever.
///
/// ```
/// use pgprox_observe::spans::may_record_query;
///
/// assert!(!may_record_query(false, true), "debug alone is not enough");
/// assert!(!may_record_query(true, false), "opt-in alone is not enough");
/// assert!(may_record_query(true, true));
/// ```
#[must_use]
pub const fn may_record_query(debug_enabled: bool, tenant_opted_in: bool) -> bool {
    debug_enabled && tenant_opted_in
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn span_names_are_fixed_and_namespaced() {
        // A tracing backend groups by name, so a name built from data is a name
        // with unbounded cardinality.
        for span in Span::all() {
            assert!(
                span.as_str().starts_with("pgprox."),
                "{span} is not namespaced"
            );
            assert!(
                span.as_str()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{span} is not a stable low-cardinality name"
            );
        }
    }

    #[test]
    fn no_two_spans_share_a_name() {
        let names: BTreeSet<&str> = Span::all().iter().map(|span| span.as_str()).collect();
        assert_eq!(names.len(), Span::all().len(), "two spans share a name");
    }

    #[test]
    fn the_tenant_is_an_attribute_and_never_part_of_a_name() {
        // The whole reason names come from a fixed list.
        for span in Span::all() {
            assert!(
                !span.as_str().contains("tenant"),
                "{span} puts the tenant in the span name"
            );
        }
        assert!(field::ALL.contains(&field::TENANT));
    }

    #[test]
    fn field_names_are_spelled_one_way() {
        // A query filtering on tenant_id that misses the spans writing tenant
        // returns a confident, wrong answer.
        let names: BTreeSet<&str> = field::ALL.iter().copied().collect();
        assert_eq!(names.len(), field::ALL.len(), "two fields share a name");
        for name in field::ALL {
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name} is not a stable field name"
            );
        }
    }

    #[test]
    fn a_credential_field_is_never_recordable() {
        for field in [
            "password",
            "PASSWORD",
            "secret",
            "token",
            "jwt",
            "authorization",
            "credential",
            "api_key",
            "private_key",
        ] {
            assert!(!is_recordable(field), "{field} was recordable");
            assert_eq!(redact(field, "hunter2"), REDACTED);
        }
    }

    #[test]
    fn the_field_that_leaks_is_never_called_password() {
        // It arrives through a struct that grew a field or a map serialised
        // whole, so the check is on substring rather than equality.
        for field in [
            "upstream_password",
            "grant.password",
            "auth_token",
            "bearer_token",
            "sidecar_credential",
            "backend.api_key",
            "user_apikey",
        ] {
            assert!(!is_recordable(field), "{field} was recordable");
            assert_eq!(redact(field, "hunter2"), REDACTED);
        }
    }

    #[test]
    fn an_ordinary_field_is_recorded_as_it_is() {
        // A redactor that redacts everything is one somebody will switch off.
        for field in field::ALL {
            assert!(is_recordable(field), "{field} was refused");
        }
        assert_eq!(redact(field::TENANT, "acme"), "acme");
        assert_eq!(redact("duration_ms", "12"), "12");
    }

    #[test]
    fn query_text_needs_both_switches() {
        // Turning the log level up fleet-wide during an incident must not start
        // recording every tenant's data as a side effect, and a tenant who
        // agreed once should not have their SQL in every log line forever.
        assert!(!may_record_query(false, false));
        assert!(
            !may_record_query(true, false),
            "debug alone recorded query text"
        );
        assert!(
            !may_record_query(false, true),
            "an opt-in alone recorded query text"
        );
        assert!(may_record_query(true, true));
    }

    #[test]
    fn there_is_no_field_for_query_text_in_the_default_set() {
        // Because it is not recorded by default, and a field name that exists
        // is a field name somebody fills in.
        // `sqlstate` is an error code, not query text, which is why this looks
        // for the names query text would actually arrive under rather than for
        // the substring `sql`.
        for name in field::ALL {
            assert!(
                !matches!(
                    *name,
                    "query" | "sql" | "sql_text" | "statement" | "query_text"
                ),
                "the default field set has a place to put query text: {name}"
            );
        }
    }

    #[test]
    fn the_redaction_marker_matches_the_one_secrets_use() {
        // So a leak looks the same wherever it surfaces and one search finds
        // every case.
        let secret = pgprox_core::SecretString::new("hunter2");
        assert_eq!(format!("{secret:?}"), REDACTED);
    }

    #[test]
    fn spans_display_as_their_names() {
        assert_eq!(Span::Transaction.to_string(), "pgprox.transaction");
        assert_eq!(Span::Acquire.as_str(), "pgprox.acquire");
    }
}
