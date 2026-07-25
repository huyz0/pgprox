//! The configuration document, as an operator writes it.
//!
//! # Why this is not `Config` with `Deserialize` on it
//!
//! `pgprox_core::Config` is the shape the code wants. This is the shape a human
//! wants, and they are not the same shape. `mode: drain` reads better than the
//! enum variant name, `drain_grace: 60s` reads better than a count of seconds,
//! and `db-1:5432` reads better than a struct with a host and a port in it.
//!
//! Keeping them apart also means the file format is a decision this crate owns
//! rather than a consequence of how a struct in `pgprox-core` happens to be
//! laid out. A field can be renamed there without every deployment's `ConfigMap`
//! becoming invalid.
//!
//! # Every error names its field
//!
//! A config error with no field name means reading the whole document to guess
//! which line is wrong, at the moment the node will not start. `serde_yaml`
//! reports a line and column, and the conversion below reports a field path, so
//! between them an operator gets pointed at the problem rather than at the
//! file.

use std::collections::BTreeMap;
use std::time::Duration;

use pgprox_core::cluster::NodeMode;
use pgprox_core::config::{Config, ConfigError, NodeOverride, ServerConfig};
use pgprox_core::ids::ServerId;
use serde::Deserialize;

/// The document, as written.
///
/// `deny_unknown_fields` on purpose. A misspelled key that is silently ignored
/// is the worst kind of configuration bug: the operator sees their edit in git,
/// the node reports no error, and the setting they meant to change never took
/// effect.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDocument {
    /// Per-upstream-server limits.
    #[serde(default)]
    pub servers: Vec<ServerDocument>,
    /// Per-node overrides. This is where drain lives.
    #[serde(default)]
    pub nodes: BTreeMap<String, NodeDocument>,
    /// Client connections this node accepts.
    #[serde(default)]
    pub max_client_conns: Option<u32>,
    /// How long a draining node waits before force-closing what remains.
    #[serde(default)]
    pub drain_grace: Option<String>,
    /// Upper bound on how long a resolved grant may be cached.
    #[serde(default)]
    pub grant_ttl_cap: Option<String>,
}

/// One upstream server's limits.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerDocument {
    /// `host:port`.
    pub server: String,
    /// The cluster-wide cap.
    ///
    /// Set this to the server's `max_connections` minus a reserve for superuser
    /// and maintenance sessions.
    pub max_connections: u32,
    /// Fraction of the cap handed out as guaranteed per-node share.
    #[serde(default)]
    pub guaranteed_fraction: Option<f64>,
}

/// One node's override.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDocument {
    /// `active` or `drain`.
    pub mode: String,
}

impl ConfigDocument {
    /// Parses a document.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] with the location `serde_yaml` reported, so an
    /// operator gets a line rather than a file.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        serde_yaml::from_str(text).map_err(|err| {
            let field = err.location().map_or_else(
                || "document".to_owned(),
                |at| format!("line {}, column {}", at.line(), at.column()),
            );
            ConfigError::Invalid {
                field,
                reason: err.to_string(),
            }
        })
    }

    /// Converts to the shape the code wants, validating on the way.
    ///
    /// Validation is `Config::validate`, the same function the fake calls, so
    /// this crate and the fake cannot disagree about what is acceptable.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] naming the field, whether the problem is in the
    /// conversion or in the resulting configuration.
    pub fn into_config(self) -> Result<Config, ConfigError> {
        let mut servers = Vec::with_capacity(self.servers.len());
        for server in self.servers {
            servers.push(server.into_server_config()?);
        }

        let mut nodes = BTreeMap::new();
        for (name, node) in self.nodes {
            let mode = parse_mode(&node.mode).ok_or_else(|| ConfigError::Invalid {
                field: format!("nodes.{name}.mode"),
                reason: format!("expected `active` or `drain`, got `{}`", node.mode),
            })?;
            nodes.insert(name, NodeOverride { mode });
        }

        let defaults = Config::default();
        let config = Config {
            servers,
            nodes,
            max_client_conns: self.max_client_conns.unwrap_or(defaults.max_client_conns),
            drain_grace: optional_duration(self.drain_grace.as_deref(), "drain_grace")?
                .unwrap_or(defaults.drain_grace),
            grant_ttl_cap: optional_duration(self.grant_ttl_cap.as_deref(), "grant_ttl_cap")?
                .unwrap_or(defaults.grant_ttl_cap),
        };

        config.validate()?;
        Ok(config)
    }
}

impl ServerDocument {
    fn into_server_config(self) -> Result<ServerConfig, ConfigError> {
        let server = parse_server(&self.server)?;
        Ok(ServerConfig {
            server,
            max_connections: self.max_connections,
            // Matches `pgprox-cluster`'s own default. Named here rather than
            // left implicit, because an operator omitting it should get the
            // documented behaviour and not whatever a struct default happens to
            // be.
            guaranteed_fraction: self.guaranteed_fraction.unwrap_or(0.5),
        })
    }
}

/// Reads `host:port`.
///
/// The port is required. A bare host would have to default to 5432, and a
/// configuration that silently points at the wrong port is worse than one that
/// refuses to start.
fn parse_server(text: &str) -> Result<ServerId, ConfigError> {
    let invalid = |reason: String| ConfigError::Invalid {
        field: format!("servers[{text}].server"),
        reason,
    };

    let (host, port) = text
        .rsplit_once(':')
        .ok_or_else(|| invalid("expected `host:port`, with the port written out".to_owned()))?;
    if host.is_empty() {
        return Err(invalid("the host is empty".to_owned()));
    }
    let port: u16 = port
        .parse()
        .map_err(|_| invalid(format!("`{port}` is not a port number")))?;
    if port == 0 {
        return Err(invalid(
            "port 0 is not a port anything listens on".to_owned(),
        ));
    }

    Ok(ServerId::new(host, port))
}

/// Reads `active` or `drain`.
fn parse_mode(text: &str) -> Option<NodeMode> {
    match text.trim().to_ascii_lowercase().as_str() {
        "active" => Some(NodeMode::Active),
        // `draining` too, because that is the word gossip and the metrics use,
        // and an operator copying it out of a dashboard should not be punished.
        "drain" | "draining" => Some(NodeMode::Draining),
        _ => None,
    }
}

/// Reads an optional duration, naming the field if it is wrong.
fn optional_duration(text: Option<&str>, field: &str) -> Result<Option<Duration>, ConfigError> {
    text.map(|text| parse_duration(text, field)).transpose()
}

/// Reads `500ms`, `60s`, `5m` or `1h`.
///
/// A unit is required. A bare number would have to mean seconds, and somebody
/// writing `drain_grace: 500` meaning milliseconds would get eight minutes of
/// drain instead, which they would discover during a deploy.
fn parse_duration(text: &str, field: &str) -> Result<Duration, ConfigError> {
    let invalid = |reason: String| ConfigError::Invalid {
        field: field.to_owned(),
        reason,
    };

    let trimmed = text.trim();
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| invalid(format!("`{trimmed}` has no unit; write `{trimmed}s`")))?;
    let (value, unit) = trimmed.split_at(split);

    let value: u64 = value
        .parse()
        .map_err(|_| invalid(format!("`{trimmed}` does not start with a number")))?;

    let millis = match unit.trim() {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60 * 1_000,
        "h" => 60 * 60 * 1_000,
        other => {
            return Err(invalid(format!(
                "`{other}` is not a unit; use ms, s, m or h"
            )));
        }
    };

    value
        .checked_mul(millis)
        .map(Duration::from_millis)
        .ok_or_else(|| invalid(format!("`{trimmed}` is longer than any useful timeout")))
}

/// Parses a document and converts it in one step.
///
/// # Errors
///
/// [`ConfigError::Invalid`] naming the field or the line.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    ConfigDocument::parse(text)?.into_config()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// The document from ADR 0006, as an operator would write it.
    const EXAMPLE: &str = r"
max_client_conns: 50000
drain_grace: 60s
grant_ttl_cap: 5m
servers:
  - server: db-1:5432
    max_connections: 4000
    guaranteed_fraction: 0.5
nodes:
  pgprox-2: { mode: drain }
";

    fn field_of(err: &ConfigError) -> String {
        match err {
            ConfigError::Invalid { field, .. } => field.clone(),
            other => unreachable!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn the_documented_example_parses() {
        let config = parse(EXAMPLE).unwrap();

        assert_eq!(config.max_client_conns, 50_000);
        assert_eq!(config.drain_grace, Duration::from_secs(60));
        assert_eq!(config.grant_ttl_cap, Duration::from_secs(300));
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].server, ServerId::new("db-1", 5432));
        assert_eq!(config.servers[0].max_connections, 4_000);
        assert_eq!(config.mode_for("pgprox-2"), NodeMode::Draining);
        assert_eq!(config.mode_for("pgprox-0"), NodeMode::Active);
    }

    #[test]
    fn an_empty_document_is_the_default_configuration() {
        // A ConfigMap that exists but says nothing should start the node, not
        // stop it.
        for text in ["", "{}", "# just a comment\n"] {
            let config = parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(config, Config::default(), "{text:?}");
        }
    }

    #[test]
    fn a_misspelled_key_is_rejected_rather_than_ignored() {
        // The worst configuration bug there is: the operator sees their edit in
        // git, the node reports nothing, and the setting never took effect.
        let err = parse("max_client_connections: 10\n").unwrap_err();
        assert!(
            err.to_string().contains("max_client_connections"),
            "the error should name the key that was not understood, got {err}"
        );
    }

    #[test]
    fn a_misspelled_nested_key_is_rejected_too() {
        let err = parse("servers:\n  - server: db-1:5432\n    max_conns: 10\n").unwrap_err();
        assert!(err.to_string().contains("max_conns"), "got {err}");
    }

    #[test]
    fn malformed_yaml_reports_where() {
        // An operator gets a line rather than a file.
        let err = parse("servers:\n  - server: [unclosed\n").unwrap_err();
        assert!(field_of(&err).contains("line"), "got {err:?}");
    }

    #[test]
    fn a_server_needs_its_port_written_out() {
        // Defaulting to 5432 would let a configuration silently point at the
        // wrong port, which is worse than refusing to start.
        let err = parse("servers:\n  - server: db-1\n    max_connections: 10\n").unwrap_err();
        assert!(err.to_string().contains("host:port"), "got {err}");
        assert!(field_of(&err).contains("db-1"), "got {err:?}");
    }

    #[test]
    fn a_bad_server_address_names_itself() {
        for (address, expected) in [
            ("db-1:", "not a port number"),
            ("db-1:abc", "not a port number"),
            ("db-1:99999", "not a port number"),
            ("db-1:0", "port 0"),
            (":5432", "host is empty"),
        ] {
            let text = format!("servers:\n  - server: \"{address}\"\n    max_connections: 10\n");
            let err = parse(&text).unwrap_err();
            assert!(
                err.to_string().contains(expected),
                "{address}: expected {expected:?}, got {err}"
            );
        }
    }

    #[test]
    fn an_ipv6_address_still_parses() {
        // The port is split from the right, so the colons inside the address do
        // not confuse it.
        let text = "servers:\n  - server: \"[::1]:5432\"\n    max_connections: 10\n";
        let config = parse(text).unwrap();
        assert_eq!(config.servers[0].server, ServerId::new("[::1]", 5432));
    }

    #[test]
    fn the_guaranteed_fraction_defaults_to_the_documented_value() {
        let text = "servers:\n  - server: db-1:5432\n    max_connections: 10\n";
        let config = parse(text).unwrap();
        assert!((config.servers[0].guaranteed_fraction - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn durations_take_a_unit() {
        for (written, expected) in [
            ("500ms", Duration::from_millis(500)),
            ("60s", Duration::from_secs(60)),
            ("5m", Duration::from_secs(300)),
            ("2h", Duration::from_secs(7_200)),
            (" 30s ", Duration::from_secs(30)),
        ] {
            let config = parse(&format!("drain_grace: \"{written}\"\n")).unwrap();
            assert_eq!(config.drain_grace, expected, "{written}");
        }
    }

    #[test]
    fn a_duration_without_a_unit_is_refused_with_a_suggestion() {
        // Somebody writing `drain_grace: 500` meaning milliseconds would get
        // eight minutes of drain instead, and find out during a deploy.
        let err = parse("drain_grace: \"500\"\n").unwrap_err();
        assert!(err.to_string().contains("500s"), "got {err}");
        assert_eq!(field_of(&err), "drain_grace");
    }

    #[test]
    fn a_duration_with_a_bad_unit_lists_the_ones_that_work() {
        let err = parse("grant_ttl_cap: \"5 fortnights\"\n").unwrap_err();
        assert!(err.to_string().contains("ms, s, m or h"), "got {err}");
        assert_eq!(field_of(&err), "grant_ttl_cap");
    }

    #[test]
    fn an_absurd_duration_is_refused_rather_than_wrapping() {
        let err = parse("drain_grace: \"99999999999999999999h\"\n").unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
    }

    #[test]
    fn a_node_mode_accepts_the_word_the_dashboard_shows() {
        // `draining` is what gossip and the metrics call it, and an operator
        // copying it out of a dashboard should not be punished.
        for word in ["drain", "draining", "DRAIN", " Draining "] {
            let text = format!("nodes:\n  pgprox-2: {{ mode: \"{word}\" }}\n");
            let config = parse(&text).unwrap();
            assert_eq!(config.mode_for("pgprox-2"), NodeMode::Draining, "{word}");
        }

        let config = parse("nodes:\n  pgprox-2: { mode: active }\n").unwrap();
        assert_eq!(config.mode_for("pgprox-2"), NodeMode::Active);
    }

    #[test]
    fn an_unknown_node_mode_names_the_node_and_the_options() {
        let err = parse("nodes:\n  pgprox-2: { mode: paused }\n").unwrap_err();
        assert_eq!(field_of(&err), "nodes.pgprox-2.mode");
        assert!(err.to_string().contains("active"), "got {err}");
        assert!(err.to_string().contains("paused"), "got {err}");
    }

    #[test]
    fn a_document_that_parses_but_is_invalid_is_still_rejected() {
        // Validation is Config::validate, the same function the fake calls, so
        // this crate and the fake cannot disagree about what is acceptable.
        let err = parse("max_client_conns: 0\n").unwrap_err();
        assert_eq!(field_of(&err), "max_client_conns");

        let duplicate = "servers:\n  - server: db-1:5432\n    max_connections: 10\n  - server: db-1:5432\n    max_connections: 20\n";
        let err = parse(duplicate).unwrap_err();
        assert!(err.to_string().contains("twice"), "got {err}");

        let fraction = "servers:\n  - server: db-1:5432\n    max_connections: 10\n    guaranteed_fraction: 1.5\n";
        let err = parse(fraction).unwrap_err();
        assert!(err.to_string().contains("guaranteed_fraction"), "got {err}");
    }

    #[test]
    fn parsing_never_panics_on_arbitrary_text() {
        // A ConfigMap is operator-controlled rather than hostile, but a node
        // that panics on a bad edit is a node that will not start and cannot
        // say why.
        for text in [
            "\0",
            "servers: [",
            "nodes:\n  - not a map",
            "max_client_conns: not a number",
            "servers: 3",
            &"a: ".repeat(1_000),
            "\u{1f600}",
        ] {
            let _ = parse(text);
        }
    }
}
