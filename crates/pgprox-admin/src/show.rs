//! The `SHOW` pseudo-database.
//!
//! # Why a second surface at all
//!
//! The HTTP API is the better one to build against. `SHOW` exists because
//! `PgBouncer` has it, and a fleet migrating from `PgBouncer` arrives with
//! dashboards, runbooks and muscle memory that all speak it. Keeping the shared
//! subset compatible means those keep working, which is worth a parser.
//!
//! It reads the same [`pgprox_core::admin::Observatory`] the HTTP handlers do,
//! so the two surfaces cannot drift into different answers to the same
//! question. That is the property worth protecting; matching `PgBouncer`'s column
//! names is the reason it exists.
//!
//! # `SHOW LOCAL`
//!
//! `SHOW POOLS` answers for the cluster, `SHOW LOCAL POOLS` for the node that
//! answered. `PgBouncer` has no such distinction because `PgBouncer` is one
//! process, so this is an addition rather than an incompatibility: the
//! unqualified form is the one existing tooling sends, and it keeps working.
//!
//! # An unknown command is an error
//!
//! Not an empty result. A dashboard sending `SHOW MEM` and receiving no rows
//! concludes there is nothing to report; one receiving an error learns the
//! command does not exist here. Empty and unsupported are different facts and a
//! psql session is exactly where somebody is trying to tell them apart.

use std::fmt;

use pgprox_core::admin::Scope;
use pgprox_core::sql::{Lexer, Token};

/// What a `SHOW` command asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ShowTarget {
    /// Upstream pools. Shared with `PgBouncer`.
    Pools,
    /// Upstream servers. Shared with `PgBouncer`.
    Servers,
    /// Client connections. Shared with `PgBouncer`.
    Clients,
    /// Fleet counters. Shared with `PgBouncer`.
    Stats,
    /// The configuration in force. Shared with `PgBouncer`.
    Config,
    /// Other proxy nodes. `pgprox` only: `PgBouncer` is one process.
    Peers,
    /// Upstream connection quota. `pgprox` only.
    Quota,
    /// Tenants. `pgprox` only.
    Tenants,
}

impl ShowTarget {
    /// The keyword, as an operator types it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pools => "pools",
            Self::Servers => "servers",
            Self::Clients => "clients",
            Self::Stats => "stats",
            Self::Config => "config",
            Self::Peers => "peers",
            Self::Quota => "quota",
            Self::Tenants => "tenants",
        }
    }

    /// Whether `PgBouncer` has this command too.
    ///
    /// The ones that do must keep its column names and order; the ones that do
    /// not are free, because no existing dashboard can be reading them.
    #[must_use]
    pub const fn is_pgbouncer_compatible(self) -> bool {
        matches!(
            self,
            Self::Pools | Self::Servers | Self::Clients | Self::Stats | Self::Config
        )
    }

    /// Every command, for the tests and for the error message.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::Pools,
            Self::Servers,
            Self::Clients,
            Self::Stats,
            Self::Config,
            Self::Peers,
            Self::Quota,
            Self::Tenants,
        ]
    }

    /// Reads a target keyword.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        Self::all()
            .iter()
            .copied()
            .find(|target| word.eq_ignore_ascii_case(target.as_str()))
    }
}

impl fmt::Display for ShowTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed `SHOW` command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShowCommand {
    /// What was asked for.
    pub target: ShowTarget,
    /// Whether `LOCAL` narrowed it.
    pub scope: Scope,
}

/// Why a statement was not a `SHOW` this proxy understands.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShowError {
    /// The statement was not a `SHOW` at all, so the caller should relay it.
    ///
    /// Distinct from an unknown target: a `SELECT` is somebody's query and
    /// belongs upstream, while `SHOW MEM` was aimed at the proxy and missed.
    #[error("not a SHOW command")]
    NotShow,
    /// `SHOW` with nothing after it.
    #[error("SHOW needs something to show; try one of: {available}")]
    Incomplete {
        /// The commands that exist.
        available: String,
    },
    /// A `SHOW` this proxy does not have.
    #[error("SHOW {what} is not supported; try one of: {available}")]
    Unknown {
        /// What was asked for.
        what: String,
        /// The commands that exist.
        available: String,
    },
}

/// The commands, for an error message.
fn available() -> String {
    ShowTarget::all()
        .iter()
        .map(|target| target.as_str().to_uppercase())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads a `SHOW` command.
///
/// Uses the shared lexer, so a `SHOW` inside a string literal is not one and a
/// comment before it does not hide it. Same reasoning as everywhere else that
/// reads SQL: text that is data must not be read as SQL.
///
/// # Errors
///
/// [`ShowError::NotShow`] if the statement is somebody's query, which the
/// caller should relay upstream. The other variants mean the statement was
/// aimed at the proxy and missed.
///
/// ```
/// use pgprox_admin::show::{ShowTarget, parse};
/// use pgprox_core::admin::Scope;
///
/// let command = parse("SHOW POOLS").unwrap();
/// assert_eq!(command.target, ShowTarget::Pools);
/// assert_eq!(command.scope, Scope::Cluster);
///
/// assert_eq!(parse("SHOW LOCAL POOLS").unwrap().scope, Scope::Local);
/// assert!(parse("SELECT 1").is_err());
/// ```
pub fn parse(sql: &str) -> Result<ShowCommand, ShowError> {
    let mut words = Lexer::new(sql).filter_map(|token| match token {
        Token::Word(word) => Some(word),
        _ => None,
    });

    let Some(first) = words.next() else {
        return Err(ShowError::NotShow);
    };
    if !first.eq_ignore_ascii_case("show") {
        return Err(ShowError::NotShow);
    }

    let Some(second) = words.next() else {
        return Err(ShowError::Incomplete {
            available: available(),
        });
    };

    // `SHOW LOCAL POOLS`. PgBouncer has no such form, so this is an addition
    // rather than an incompatibility.
    let (scope, target_word) = if second.eq_ignore_ascii_case("local") {
        let Some(third) = words.next() else {
            return Err(ShowError::Incomplete {
                available: available(),
            });
        };
        (Scope::Local, third)
    } else {
        (Scope::Cluster, second)
    };

    let target = ShowTarget::parse(target_word).ok_or_else(|| ShowError::Unknown {
        what: target_word.to_uppercase(),
        available: available(),
    })?;

    // Anything after the target is a form this does not implement. Ignoring it
    // would answer a different question than the one asked, which in a psql
    // session is worse than saying so.
    if let Some(extra) = words.next() {
        return Err(ShowError::Unknown {
            what: format!(
                "{} {}",
                target.as_str().to_uppercase(),
                extra.to_uppercase()
            ),
            available: available(),
        });
    }

    Ok(ShowCommand { target, scope })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_command_the_adr_names_parses() {
        for (sql, target) in [
            ("SHOW POOLS", ShowTarget::Pools),
            ("SHOW SERVERS", ShowTarget::Servers),
            ("SHOW CLIENTS", ShowTarget::Clients),
            ("SHOW PEERS", ShowTarget::Peers),
            ("SHOW QUOTA", ShowTarget::Quota),
            ("SHOW TENANTS", ShowTarget::Tenants),
            ("SHOW CONFIG", ShowTarget::Config),
            ("SHOW STATS", ShowTarget::Stats),
        ] {
            let command = parse(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
            assert_eq!(command.target, target, "{sql}");
            assert_eq!(command.scope, Scope::Cluster, "{sql}");
        }
    }

    #[test]
    fn every_command_has_a_local_form() {
        for target in ShowTarget::all() {
            let sql = format!("SHOW LOCAL {}", target.as_str().to_uppercase());
            let command = parse(&sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
            assert_eq!(command.target, *target);
            assert_eq!(command.scope, Scope::Local, "{sql}");
        }
    }

    #[test]
    fn case_and_whitespace_do_not_matter() {
        for sql in [
            "show pools",
            "SHOW POOLS",
            "ShOw PoOlS",
            "  show   pools  ",
            "show\tpools",
            "show pools;",
            "/* dashboard */ SHOW POOLS",
            "-- a comment\nSHOW POOLS",
        ] {
            let command = parse(sql).unwrap_or_else(|e| panic!("{sql:?}: {e}"));
            assert_eq!(command.target, ShowTarget::Pools, "{sql:?}");
        }
    }

    #[test]
    fn an_ordinary_query_is_relayed_rather_than_refused() {
        // A SELECT is somebody's query and belongs upstream. Refusing it here
        // would break every client that ever sends one.
        for sql in ["SELECT 1", "INSERT INTO t VALUES (1)", "BEGIN", "", "   "] {
            assert_eq!(parse(sql), Err(ShowError::NotShow), "{sql:?}");
        }
    }

    #[test]
    fn a_postgres_show_is_relayed_rather_than_answered_wrongly() {
        // `SHOW work_mem` is a real Postgres command and the client wants the
        // server's answer, not ours.
        let err = parse("SHOW work_mem").unwrap_err();
        assert!(matches!(err, ShowError::Unknown { .. }), "{err:?}");
    }

    #[test]
    fn an_unknown_show_is_an_error_rather_than_an_empty_result() {
        // A dashboard receiving no rows concludes there is nothing to report;
        // one receiving an error learns the command does not exist here. Empty
        // and unsupported are different facts.
        let err = parse("SHOW MEM").unwrap_err();
        let ShowError::Unknown { what, available } = &err else {
            unreachable!("wrong variant: {err:?}");
        };
        assert_eq!(what, "MEM");
        assert!(available.contains("POOLS"), "{available}");
        assert!(err.to_string().contains("MEM"), "{err}");
    }

    #[test]
    fn a_bare_show_lists_what_it_could_have_been() {
        for sql in ["SHOW", "SHOW LOCAL", "show local ;"] {
            let err = parse(sql).unwrap_err();
            assert!(
                matches!(err, ShowError::Incomplete { .. }),
                "{sql:?} gave {err:?}"
            );
            assert!(err.to_string().contains("POOLS"), "{err}");
        }
    }

    #[test]
    fn a_form_this_does_not_implement_says_so_rather_than_answering_the_wrong_one() {
        // `SHOW POOLS EXTENDED` is a different question, and answering the
        // plain one would be a confident wrong answer.
        let err = parse("SHOW POOLS EXTENDED").unwrap_err();
        assert!(matches!(err, ShowError::Unknown { .. }), "{err:?}");
        assert!(err.to_string().contains("EXTENDED"), "{err}");
    }

    #[test]
    fn a_show_inside_a_string_is_not_a_command() {
        // The shared lexer's job. Text that is data must not be read as SQL,
        // here as everywhere else.
        assert_eq!(parse("SELECT 'SHOW POOLS'"), Err(ShowError::NotShow));
    }

    #[test]
    fn the_commands_pgbouncer_has_are_marked_as_such() {
        // Those must keep its column names and order. The rest are free,
        // because no existing dashboard can be reading them.
        for target in [
            ShowTarget::Pools,
            ShowTarget::Servers,
            ShowTarget::Clients,
            ShowTarget::Stats,
            ShowTarget::Config,
        ] {
            assert!(target.is_pgbouncer_compatible(), "{target}");
        }
        for target in [ShowTarget::Peers, ShowTarget::Quota, ShowTarget::Tenants] {
            assert!(
                !target.is_pgbouncer_compatible(),
                "{target} is not a PgBouncer command and is marked as one"
            );
        }
    }

    #[test]
    fn a_target_keyword_round_trips() {
        for target in ShowTarget::all() {
            assert_eq!(ShowTarget::parse(target.as_str()), Some(*target));
            assert_eq!(
                ShowTarget::parse(&target.as_str().to_uppercase()),
                Some(*target)
            );
            assert_eq!(target.to_string(), target.as_str());
        }
        assert_eq!(ShowTarget::parse("nonsense"), None);
    }

    #[test]
    fn parsing_never_panics_on_arbitrary_text() {
        // This reads statements arriving from a client socket.
        for sql in [
            "SHOW '",
            "SHOW $$",
            "SHOW /*",
            "show local local local",
            "\u{1f600}",
            "SHOW \0",
            &"SHOW ".repeat(500),
        ] {
            let _ = parse(sql);
        }
    }
}
