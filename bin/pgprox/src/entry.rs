//! Argument parsing and the real dependencies.
//!
//! Everything `main.rs` would otherwise hold, put where a test can call it.
//!
//! # The order of a start
//!
//! Configuration, then sidecar, then ports, then serving. Everything that can
//! fail on a deployment mistake fails before a port is bound, so a bad rollout
//! is a pod that never became ready rather than one accepting clients it
//! cannot serve.
//!
//! # Stopping
//!
//! `SIGTERM` fires the same [`Shutdown`] the drain sequence uses. Kubernetes
//! sends it on pod termination, and a proxy that ignored it would have every
//! client cut when the grace period ran out instead of finishing what it held.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pgprox_auth::client::{SidecarConfig, SidecarResolver};
use pgprox_config::provider::{FileConfig, FileSource};
use pgprox_core::clock::SystemClock;
use pgprox_core::ids::NodeId;

use crate::run::{Addrs, Listeners, Shutdown};
use crate::wiring::{App, Deps, StartupError};

/// The default place a `ConfigMap` is mounted.
pub const DEFAULT_CONFIG_PATH: &str = "/etc/pgprox/config.yaml";

/// The default sidecar socket.
pub const DEFAULT_SIDECAR_SOCKET: &str = "/var/run/pgprox/sidecar.sock";

/// Where clients arrive by default.
///
/// 6432 rather than 5432: a proxy sharing a port with the database it fronts
/// makes every connection string ambiguous about which one answered.
pub const DEFAULT_LISTEN: &str = "0.0.0.0:6432";

/// Where the probes and the admin API are served by default.
pub const DEFAULT_ADMIN: &str = "0.0.0.0:9090";

/// Where peers gossip by default.
pub const DEFAULT_GOSSIP: &str = "0.0.0.0:6433";

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
    /// Where clients arrive.
    pub listen: SocketAddr,
    /// Where the probes and the admin API are served.
    pub admin: SocketAddr,
    /// Where peers gossip.
    pub gossip: SocketAddr,
    /// The peers this node gossips to, by node number.
    ///
    /// Given rather than discovered: a node that discovered its own fleet
    /// would be deciding the fleet size, and the guaranteed share is divided
    /// by the configured size on purpose. Keyed by node number because a quota
    /// request has to reach one specific node, the leader, rather than
    /// whichever peer answers first.
    pub peers: std::collections::BTreeMap<NodeId, SocketAddr>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config: PathBuf::from(DEFAULT_CONFIG_PATH),
            sidecar: PathBuf::from(DEFAULT_SIDECAR_SOCKET),
            node: NodeId::new(1),
            node_name: "pgprox-1".to_owned(),
            // Both parse, and a default that could fail to would make every
            // caller handle an error that cannot happen. Asserted by a test
            // rather than by unwrapping here.
            listen: SocketAddr::from(([0, 0, 0, 0], 6432)),
            admin: SocketAddr::from(([0, 0, 0, 0], 9090)),
            gossip: SocketAddr::from(([0, 0, 0, 0], 6433)),
            peers: std::collections::BTreeMap::new(),
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
                "--listen" => options.listen = address(&value()?, "--listen")?,
                "--admin" => options.admin = address(&value()?, "--admin")?,
                "--gossip" => options.gossip = address(&value()?, "--gossip")?,
                // Repeatable, one flag per peer, and each carries the peer's
                // node number: `--peer 2=10.0.0.2:6433`. The number is what
                // lets a quota request reach the leader specifically.
                "--peer" => {
                    let (node, addr) = peer(&value()?)?;
                    options.peers.insert(node, addr);
                }
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

    /// Both ports, as the run loop wants them.
    #[must_use]
    pub const fn addrs(&self) -> Addrs {
        Addrs {
            client: self.listen,
            admin: self.admin,
            gossip: self.gossip,
        }
    }
}

/// Parses a `node=host:port` peer.
fn peer(raw: &str) -> Result<(NodeId, SocketAddr), StartupError> {
    let (node, addr) = raw.split_once('=').ok_or_else(|| StartupError::Arguments {
        detail: format!("--peer must be node=host:port, got {raw}"),
    })?;
    let node = node.parse().map_err(|_| StartupError::Arguments {
        detail: format!("--peer node must be a number, got {node}"),
    })?;
    Ok((NodeId::new(node), address(addr, "--peer")?))
}

/// Parses a listen address, naming the flag that carried it.
fn address(raw: &str, flag: &str) -> Result<SocketAddr, StartupError> {
    raw.parse().map_err(|_| StartupError::Arguments {
        detail: format!("{flag} must be host:port, got {raw}"),
    })
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
    runtime.block_on(serve(options))
}

/// Starts a node and runs it until it is told to stop.
///
/// # Errors
///
/// As [`start`], plus a port that cannot be bound.
pub async fn serve(options: Options) -> Result<(), StartupError> {
    let addrs = options.addrs();
    let peers = options.peers.clone();
    let app = start(options).await?;
    let listeners = Listeners::bind(addrs)
        .await
        .map_err(|err| StartupError::Arguments {
            detail: format!("could not bind {}: {err}", addrs.client),
        })?;

    let shutdown = Shutdown::new();
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            terminated().await;
            shutdown.fire();
        }
    });

    crate::run::run_with_peers(app, listeners, peers, shutdown)
        .await
        .map_err(|err| StartupError::Arguments {
            detail: format!("the node stopped serving: {err}"),
        })
}

/// Resolves when the process is asked to stop.
///
/// `SIGTERM` is what Kubernetes sends; `SIGINT` is what an operator running it
/// by hand sends. A platform without signals waits forever, which is right:
/// there is nothing there to ask it to stop.
async fn terminated() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        // Nothing left to listen with means the node runs until it is killed,
        // which beats exiting because a handler could not be installed.
        let Ok(mut term) = signal(SignalKind::terminate()) else {
            return std::future::pending().await;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
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
        // An empty root store until certificates are configured. A backend
        // that asks for a verified connection therefore fails to verify, which
        // is the safe direction: the alternative is trusting whatever answers.
        tls: pgprox_tls::client_config(tokio_rustls::rustls::RootCertStore::empty()).map_err(
            |err| StartupError::Arguments {
                detail: format!("could not build the upstream TLS configuration: {err}"),
            },
        )?,
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
    fn the_ports_are_read_and_the_defaults_parse() {
        let options =
            Options::parse(["--listen", "127.0.0.1:1", "--admin", "127.0.0.1:2"]).unwrap();
        assert_eq!(options.addrs().client.port(), 1);
        assert_eq!(options.addrs().admin.port(), 2);

        // The defaults are built rather than parsed, so this is what says they
        // are the addresses the documentation claims.
        assert_eq!(
            Options::default().listen,
            DEFAULT_LISTEN.parse::<SocketAddr>().unwrap()
        );
        assert_eq!(
            Options::default().admin,
            DEFAULT_ADMIN.parse::<SocketAddr>().unwrap()
        );
        assert_eq!(
            Options::default().gossip,
            DEFAULT_GOSSIP.parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn peers_accumulate_rather_than_replacing_each_other() {
        // One flag per peer. A second --peer overwriting the first would leave
        // a three-node fleet gossiping to one node and never converging.
        let options =
            Options::parse(["--peer", "2=10.0.0.2:6433", "--peer", "3=10.0.0.3:6433"]).unwrap();

        assert_eq!(options.peers.len(), 2);
        assert_eq!(options.peers[&NodeId::new(2)].port(), 6433);
    }

    #[test]
    fn a_peer_without_its_node_number_stops_the_start() {
        // The number is what a quota request is addressed by. A peer table
        // that guessed would send a request to the wrong node, and the wrong
        // node would answer NoLeader forever.
        let err = Options::parse(["--peer", "10.0.0.2:6433"]).unwrap_err();
        assert!(err.to_string().contains("node=host:port"), "{err}");
    }

    #[test]
    fn an_address_that_is_not_one_stops_the_start() {
        // A node that fell back to the default port would be reachable at an
        // address nobody configured, and unreachable at the one they did.
        let err = Options::parse(["--listen", "6432"]).unwrap_err();
        assert!(err.to_string().contains("--listen"), "{err}");
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
