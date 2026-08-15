//! What the process was told to do.

use std::path::PathBuf;

/// What went wrong before any load was generated.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// The command line was wrong.
    #[error("arguments: {detail}")]
    Arguments {
        /// What was wrong with it.
        detail: String,
    },
    /// The workload document could not be read.
    #[error("workload {path}: {detail}")]
    Workload {
        /// Where it was read from.
        path: String,
        /// Why it failed.
        detail: String,
    },
    /// The report could not be written.
    #[error("writing the report to {path}: {source}")]
    Report {
        /// Where it was going.
        path: String,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// Nothing could connect, so there is nothing to report.
    ///
    /// Distinguished from a run with errors in it on purpose: a run where
    /// every connection failed is a broken target or a wrong password, and
    /// reporting a beautiful p99 over zero transactions is the failure mode a
    /// load client has to make impossible.
    #[error("no connection succeeded: {detail}")]
    NoConnection {
        /// What the most recent failure said.
        ///
        /// `M88.10`. Not the first: a connection retries for the life of the
        /// run, and a target can change why it refuses partway through, so
        /// the first reason seen can describe a moment already gone.
        detail: String,
    },
}

/// Where the load goes and how much of it there is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// `host:port`.
    pub target: String,
    /// The workload document.
    pub workload: PathBuf,
    /// Client connections to open.
    pub connections: u32,
    /// How long to keep going, in seconds.
    pub duration_secs: u64,
    /// The sampler seed, so a run can be repeated exactly.
    pub seed: u64,
    /// The user in the startup packet.
    pub user: String,
    /// The database in the startup packet.
    pub database: String,
    /// What to answer a cleartext password request with.
    ///
    /// Against the proxy this is a JWT. Against Postgres directly it is a
    /// password, and only if that server asks for one in the clear.
    pub password: String,
    /// Where the JSON report is written.
    ///
    /// A file rather than standard output: this binary logs to stderr like
    /// every other one in the workspace, and a report that shared a stream
    /// with log lines would be a report a script has to filter.
    pub out: PathBuf,
    /// How long to give one connection's startup before giving up on it.
    pub connect_timeout_secs: u64,
    /// Over how many seconds the connections arrive.
    ///
    /// Zero means all at once, which is a reconnect storm rather than a steady
    /// state: every connection runs its first transaction before any of them
    /// has begun to think, so ten thousand connections offer ten thousand
    /// transactions in the same instant and a run measures the queue that
    /// forms. Real clients arrive spread out, and so does a run that means to
    /// measure what connections cost rather than what a stampede costs.
    pub ramp_secs: u64,
    /// Whether to ask for TLS, and to accept whatever certificate arrives.
    ///
    /// Off by default, which is what a direct connection to Postgres in the
    /// test stack wants. See `crate::tls` for what "insecure" means here and
    /// why it is acceptable for a load generator and nowhere else.
    pub tls: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            target: "127.0.0.1:6432".to_owned(),
            workload: PathBuf::from("docs/internal/product/perf/workload.yaml"),
            connections: 100,
            duration_secs: 30,
            seed: 1,
            user: "acme_app".to_owned(),
            database: "tenant_acme".to_owned(),
            password: String::new(),
            out: PathBuf::from("report.json"),
            connect_timeout_secs: 30,
            ramp_secs: 0,
            tls: false,
        }
    }
}

impl Options {
    /// Reads options from command-line arguments.
    ///
    /// # Errors
    ///
    /// Fails on an unknown argument, on one missing its value, on a number
    /// that is not one, and on a connection count of zero. A run with a
    /// mistyped flag must not quietly measure the default instead.
    pub fn parse<I, S>(args: I) -> Result<Self, LoadError>
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
                    .ok_or_else(|| LoadError::Arguments {
                        detail: format!("{flag} needs a value"),
                    })
            };

            match flag.as_str() {
                "--target" => options.target = value()?,
                "--workload" => options.workload = PathBuf::from(value()?),
                "--connections" => options.connections = number(&value()?, "--connections")?,
                "--duration" => options.duration_secs = number(&value()?, "--duration")?,
                "--seed" => options.seed = number(&value()?, "--seed")?,
                "--user" => options.user = value()?,
                "--database" => options.database = value()?,
                "--password" => options.password = value()?,
                "--out" => options.out = PathBuf::from(value()?),
                "--ramp" => options.ramp_secs = number(&value()?, "--ramp")?,
                "--tls-insecure" => options.tls = true,
                "--connect-timeout" => {
                    options.connect_timeout_secs = number(&value()?, "--connect-timeout")?;
                }
                other => {
                    return Err(LoadError::Arguments {
                        detail: format!("unknown argument {other}"),
                    });
                }
            }
        }

        if options.connections == 0 {
            return Err(LoadError::Arguments {
                detail: "--connections 0 would measure nothing".to_owned(),
            });
        }
        if options.duration_secs == 0 {
            return Err(LoadError::Arguments {
                detail: "--duration 0 would measure nothing".to_owned(),
            });
        }
        Ok(options)
    }
}

fn number<T: std::str::FromStr>(raw: &str, flag: &str) -> Result<T, LoadError> {
    raw.parse().map_err(|_| LoadError::Arguments {
        detail: format!("{flag} must be a number, got {raw}"),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Options {
        Options::parse(args).unwrap()
    }

    fn refused(args: &[&str]) -> String {
        match Options::parse(args) {
            Err(error) => format!("{error}"),
            Ok(options) => panic!("accepted: {options:?}"),
        }
    }

    #[test]
    fn every_flag_reaches_the_field_it_names() {
        let options = parsed(&[
            "--target",
            "pgprox-1:6432",
            "--workload",
            "/w.yaml",
            "--connections",
            "1000",
            "--duration",
            "60",
            "--seed",
            "77",
            "--user",
            "someone",
            "--database",
            "somewhere",
            "--password",
            "a-token",
            "--out",
            "/run.json",
            "--connect-timeout",
            "5",
            "--ramp",
            "30",
            "--tls-insecure",
        ]);

        assert_eq!(options.target, "pgprox-1:6432");
        assert_eq!(options.workload, PathBuf::from("/w.yaml"));
        assert_eq!(options.connections, 1000);
        assert_eq!(options.duration_secs, 60);
        assert_eq!(options.seed, 77);
        assert_eq!(options.user, "someone");
        assert_eq!(options.database, "somewhere");
        assert_eq!(options.password, "a-token");
        assert_eq!(options.out, PathBuf::from("/run.json"));
        assert_eq!(options.connect_timeout_secs, 5);
        assert!(options.tls, "--tls-insecure did not reach the field");
        assert_eq!(options.ramp_secs, 30);
    }

    #[test]
    fn no_arguments_is_the_default_run() {
        let options: &[&str] = &[];
        assert_eq!(Options::parse(options).unwrap(), Options::default());
    }

    #[test]
    fn a_mistyped_flag_is_refused_rather_than_ignored() {
        // Ignoring it would measure the default and report it as though the
        // flag had been honoured, which is a wrong number nobody would catch.
        assert!(refused(&["--connectons", "10"]).contains("--connectons"));
    }

    #[test]
    fn a_flag_with_no_value_is_refused() {
        assert!(refused(&["--target"]).contains("needs a value"));
    }

    #[test]
    fn a_count_that_is_not_a_number_is_refused() {
        let message = refused(&["--connections", "lots"]);
        assert!(message.contains("--connections"), "{message}");
        assert!(message.contains("lots"), "{message}");
    }

    #[test]
    fn a_run_that_would_measure_nothing_is_refused() {
        assert!(refused(&["--connections", "0"]).contains("measure nothing"));
        assert!(refused(&["--duration", "0"]).contains("measure nothing"));
    }
}
