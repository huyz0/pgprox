//! Argument parsing and the real dependencies.
//!
//! Everything `main.rs` would otherwise hold, put where a test can call it.
//!
//! # What this does today
//!
//! Builds a node and returns. The listener, the accept loop and the drain
//! sequence are separate tasks, so the binary as it stands starts, validates
//! its configuration, wires the node together and exits. That is deliberately
//! short of useful and deliberately not a stub: everything it does is what the
//! running node will do first, and a configuration mistake fails here rather
//! than after a port is bound.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pgprox_auth::client::{SidecarConfig, SidecarResolver};
use pgprox_config::provider::{FileConfig, FileSource};
use pgprox_core::clock::SystemClock;
use pgprox_core::ids::NodeId;

use crate::wiring::{App, Deps, StartupError};

/// The default place a `ConfigMap` is mounted.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/pgprox/config.yaml";

/// The default sidecar socket.
pub const DEFAULT_SIDECAR_SOCKET: &str = "/var/run/pgprox/sidecar.sock";

/// What the process was told to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Where the configuration document is.
    pub config: PathBuf,
    /// Where the credential sidecar listens.
    pub sidecar: PathBuf,
    /// This node's number.
    pub node: NodeId,
    /// This node's name in the configuration document.
    pub node_name: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config: PathBuf::from(DEFAULT_CONFIG_PATH),
            sidecar: PathBuf::from(DEFAULT_SIDECAR_SOCKET),
            node: NodeId::new(1),
            node_name: "pgprox-1".to_owned(),
        }
    }
}

impl Options {
    /// Reads options from command-line arguments.
    ///
    /// Deliberately tiny, and deliberately not a dependency. A proxy takes four
    /// arguments and reads the rest from a document it can reload; an argument
    /// parser would be more code than the thing it parses.
    ///
    /// # Errors
    ///
    /// Fails on an unknown argument, on one missing its value, and on a node
    /// number that is not a number. Every one of those is a deployment mistake
    /// that must not start a node with a default it did not ask for: two nodes
    /// silently sharing node number 1 would issue each other's cancel keys.
    pub fn parse<I, S>(args: I) -> Result<Self, StartupError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(flag) = args.next() {
            let flag = flag.as_ref().to_owned();
            let mut value = || {
                args.next()
                    .map(|v| v.as_ref().to_owned())
                    .ok_or_else(|| StartupError::Arguments {
                        detail: format!("{flag} needs a value"),
                    })
            };

            match flag.as_str() {
                "--config" => options.config = PathBuf::from(value()?),
                "--sidecar" => options.sidecar = PathBuf::from(value()?),
                "--node-name" => options.node_name = value()?,
                "--node" => {
                    let raw = value()?;
                    options.node =
                        NodeId::new(raw.parse().map_err(|_| StartupError::Arguments {
                            detail: format!("--node must be a number, got {raw}"),
                        })?);
                }
                other => {
                    return Err(StartupError::Arguments {
                        detail: format!("unknown argument {other}"),
                    });
                }
            }
        }

        Ok(options)
    }
}

/// Builds the real dependencies and starts a node.
///
/// # Errors
///
/// Fails on a bad argument, an unreadable or invalid configuration, or a
/// sidecar that cannot be reached. All three are startup failures: a node that
/// cannot resolve credentials can authenticate nobody, so coming up anyway
/// would mean a pod that passes its liveness probe and refuses every client.
pub fn run_with<I, S>(args: I) -> Result<(), StartupError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let options = Options::parse(args)?;
    let runtime = tokio::runtime::Runtime::new().map_err(|err| StartupError::Arguments {
        detail: format!("could not start the async runtime: {err}"),
    })?;
    runtime.block_on(async { start(options).await.map(drop) })
}

/// Builds a node from real dependencies.
///
/// # Errors
///
/// As [`run_with`].
pub async fn start(options: Options) -> Result<App, StartupError> {
    // The configuration is read first, so a deployment with both a bad
    // document and no sidecar is told about the one it can fix without a
    // second process being involved.
    let source = FileSource::new(FileConfig::at(&options.config))?;
    let resolver = SidecarResolver::connect(&SidecarConfig {
        socket_path: options.sidecar.clone(),
        timeout: Duration::from_secs(5),
    })
    .await
    .map_err(|err| StartupError::Sidecar {
        detail: err.to_string(),
    })?;

    start_with(options, source, Arc::new(resolver)).await
}

/// Builds a node from a configuration source and a resolver that already
/// exist.
///
/// Split out so everything except the sidecar connection is reachable from a
/// test: connecting needs a second process, and a function that could only be
/// exercised with one would be a function nothing exercises.
///
/// # Errors
///
/// Fails when the configuration cannot be loaded or does not validate.
pub async fn start_with(
    options: Options,
    config: Arc<dyn pgprox_core::config::ConfigSource>,
    resolver: Arc<dyn pgprox_core::auth::CredentialResolver>,
) -> Result<App, StartupError> {
    App::build(Deps {
        node: options.node,
        node_name: options.node_name,
        clock: Arc::new(SystemClock),
        config,
        resolver,
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn no_arguments_means_the_kubernetes_defaults() {
        let options = Options::parse(Vec::<String>::new()).unwrap();

        assert_eq!(options.config, PathBuf::from(DEFAULT_CONFIG_PATH));
        assert_eq!(options.sidecar, PathBuf::from(DEFAULT_SIDECAR_SOCKET));
    }

    #[test]
    fn every_argument_is_read() {
        let options = Options::parse([
            "--config",
            "/tmp/c.yaml",
            "--sidecar",
            "/tmp/s.sock",
            "--node",
            "7",
            "--node-name",
            "pgprox-7",
        ])
        .unwrap();

        assert_eq!(options.config, PathBuf::from("/tmp/c.yaml"));
        assert_eq!(options.sidecar, PathBuf::from("/tmp/s.sock"));
        assert_eq!(options.node, NodeId::new(7));
        assert_eq!(options.node_name, "pgprox-7");
    }

    #[test]
    fn an_unknown_argument_stops_the_start() {
        // Rather than being ignored. A typo in a deployment manifest that
        // silently does nothing is how a node runs for weeks with a setting
        // somebody believes is in force.
        assert!(matches!(
            Options::parse(["--conifg", "/tmp/c.yaml"]),
            Err(StartupError::Arguments { .. })
        ));
    }

    #[test]
    fn an_argument_with_no_value_stops_the_start() {
        assert!(matches!(
            Options::parse(["--config"]),
            Err(StartupError::Arguments { .. })
        ));
    }

    #[test]
    fn a_node_number_that_is_not_a_number_stops_the_start() {
        // Falling back to node 1 would have two nodes issuing each other's
        // cancel keys, and a cancel landing on the wrong query.
        let err = Options::parse(["--node", "one"]).unwrap_err();
        assert!(err.to_string().contains("one"), "{err}");
    }

    /// A configuration document good enough to build a node from.
    fn document() -> &'static str {
        "max_client_conns: 100\nservers:\n  - server: db-1:5432\n    max_connections: 100\n"
    }

    #[test]
    fn a_bad_configuration_path_fails_before_the_runtime_does_anything() {
        // Exercises the same entry point main.rs calls, so the one file
        // excluded from coverage is one line that forwards.
        assert!(matches!(
            run_with(["--config", "/nonexistent/pgprox/config.yaml"]),
            Err(StartupError::Config(_))
        ));
    }

    #[test]
    fn a_bad_argument_fails_before_the_runtime_starts() {
        assert!(matches!(
            run_with(["--nope"]),
            Err(StartupError::Arguments { .. })
        ));
    }

    #[tokio::test]
    async fn a_node_is_built_from_a_real_document_on_disk() {
        // Everything start() does except the sidecar connection, which needs a
        // second process.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(&path, document()).unwrap();

        let options = Options {
            config: path.clone(),
            node: NodeId::new(4),
            node_name: "pgprox-4".to_owned(),
            ..Options::default()
        };
        let source = FileSource::new(FileConfig::at(&path)).unwrap();
        let app = start_with(
            options,
            source,
            Arc::new(pgprox_core::auth::FakeCredentialResolver::new()),
        )
        .await
        .unwrap();

        assert_eq!(app.config.servers.len(), 1);
        assert!(!app.is_draining());
    }

    #[tokio::test]
    async fn a_missing_configuration_file_stops_the_start() {
        let err = start(Options {
            config: PathBuf::from("/nonexistent/pgprox/config.yaml"),
            ..Options::default()
        })
        .await
        .unwrap_err();

        assert!(
            matches!(err, StartupError::Config(_)),
            "a missing config was reported as something else: {err}"
        );
    }

    #[tokio::test]
    async fn a_sidecar_that_is_not_there_stops_the_start() {
        // A node that came up anyway would pass its liveness probe and refuse
        // every client, which is the worst of both.
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.yaml");
        std::fs::write(&config, document()).unwrap();

        let err = start(Options {
            config,
            sidecar: dir.path().join("nothing.sock"),
            ..Options::default()
        })
        .await
        .unwrap_err();

        assert!(
            matches!(err, StartupError::Sidecar { .. }),
            "an unreachable sidecar was reported as something else: {err}"
        );
    }
}
