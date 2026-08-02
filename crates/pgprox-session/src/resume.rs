//! Making a borrowed connection look like this session's own.
//!
//! A session that is not pinned gets a different upstream connection from one
//! transaction to the next. Two things have to be carried across that gap, or
//! the client sees state that is not its own:
//!
//! - **Session parameters.** `search_path` above all, which decides which
//!   schema unqualified table names resolve to. A connection carrying another
//!   tenant's `search_path` silently answers from the wrong tables.
//! - **Prepared statements.** Every modern driver uses named `Parse`. The
//!   client's name is rewritten to a global one derived from the SQL, and the
//!   target connection may or may not already hold it.
//!
//! # Why both live here
//!
//! `pgprox-pool` owns the bookkeeping for each: what this session set, what
//! this connection holds. It deliberately does not know what to *send*,
//! because sending is frames and frames are `pgprox-proto`. This module is the
//! join, and it is the one the statements module's own docs point at.
//!
//! # The direction that is easy to forget
//!
//! Replay is not only about setting what this session wants. A connection
//! carrying a parameter from whoever used it last, which this session never
//! mentioned, has to be reset. Otherwise the leak runs the other way and the
//! previous tenant's setting becomes this one's default.

use pgprox_core::error::ClientError;
use pgprox_pool::params::SessionParams;
use pgprox_pool::statements::{
    ConnectionStatements, GlobalName, Preparation, SessionStatements, StatementConfig,
    deallocates_everything,
};

/// One thing to send before the client's own frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A simple query: a `SET` or a `RESET`.
    Run(String),
    /// A protocol `Close` of a statement, to make room under the cap.
    ///
    /// Not a SQL `DEALLOCATE`: these are protocol-level statements, and the
    /// SQL form cannot name one whose name came from a `Parse`.
    Close(GlobalName),
    /// A protocol `Parse` under the global name.
    Prepare {
        /// The name the server will know it by.
        global: GlobalName,
        /// The SQL to prepare.
        sql: String,
    },
}

/// What one upstream connection currently carries.
///
/// Held by the shell alongside the connection itself, and reset when the
/// connection is closed rather than when a session lets go of it.
#[derive(Debug)]
pub struct ConnectionMemory {
    /// Parameters set on it, by whoever used it last.
    pub params: SessionParams,
    /// Statements it holds.
    pub statements: ConnectionStatements,
}

impl ConnectionMemory {
    /// A freshly opened connection, carrying nothing.
    #[must_use]
    pub fn new(config: StatementConfig) -> Self {
        Self {
            params: SessionParams::new(),
            statements: ConnectionStatements::new(config),
        }
    }
}

impl Default for ConnectionMemory {
    fn default() -> Self {
        Self::new(StatementConfig::default())
    }
}

/// What this session expects any connection it borrows to look like.
#[derive(Debug, Default)]
pub struct SessionMemory {
    /// Parameters this session set.
    pub params: SessionParams,
    /// Statements this session prepared, by the name the client chose.
    pub statements: SessionStatements,
}

/// Brings a freshly acquired connection up to this session's parameters.
///
/// Returns the statements to run before the client's frame, or nothing at all
/// when the connection already matches, which is the common case for a warm
/// pool serving one tenant.
#[must_use]
pub fn on_acquire(session: &SessionMemory, connection: &ConnectionMemory) -> Vec<Step> {
    session
        .params
        .replay_onto(&connection.params)
        .into_iter()
        .map(Step::Run)
        .collect()
}

/// Records that the replay was sent, so the connection is not re-set next time.
///
/// Separate from [`on_acquire`] because the shell may fail to send it, and a
/// connection recorded as carrying settings it never received is worse than
/// one that gets them twice.
pub fn replayed(session: &SessionMemory, connection: &mut ConnectionMemory) {
    connection.params.clear_all();
    for (name, value) in session.params.iter() {
        connection.params.record(name, value);
    }
}

/// Records what a client statement did to state neither side can see alone.
///
/// Call it for every statement the client sends upstream, before the answer
/// comes back. Returns whether anything was forgotten, which is a metric worth
/// having: a client running `DISCARD ALL` between statements is re-preparing
/// its whole cache each time, and that is a workload nobody would guess at from
/// the outside.
///
/// # What this closes
///
/// `DISCARD ALL` deallocates every prepared statement on the connection, and
/// both maps went on believing otherwise. `SessionStatements::close_all` and
/// `ConnectionStatements::forget_all` have existed since M5 and neither had a
/// caller outside its own tests, so the first `Bind` after a `DISCARD ALL`
/// named a global the server had just dropped.
///
/// The parameter half was already handled: `SessionParams::observe_statement`
/// treats `DISCARD ALL` as `RESET ALL`. Only the statements were missed, which
/// is what made this hard to see. Half the state was being tracked correctly.
/// # The two maps rather than the two memory structs
///
/// `SessionMemory` and `ConnectionMemory` are how this module carries state
/// around, and taking them was the obvious signature. It was also why this
/// function sat uncalled for a milestone: the proxy holds a `SessionMemory` but
/// keeps the connection's statements on `Upstreamed`, which is the thing the
/// pool lends, so there was no call site where both structs were in scope at
/// once. `M16.7`.
///
/// The maps are what it needs, and asking for them is what lets the caller
/// exist.
pub fn observe_statement(
    session: &mut SessionStatements,
    connection: &mut ConnectionStatements,
    sql: &str,
) -> bool {
    if !deallocates_everything(sql) {
        return false;
    }

    // Both sides, and both for the same reason. The session map is the client's
    // names, and the client knows it ran a DISCARD ALL, so it will re-parse.
    // The connection map is what the server holds, and the server has just
    // dropped it.
    session.close_all();
    connection.forget_all();
    true
}

/// Records a protocol `Close` of a prepared statement.
///
/// Returns whether anything was forgotten, so a caller can count it the way
/// [`observe_statement`] counts a `DISCARD ALL`.
///
/// # The half that was missing
///
/// A client's `Close` is rewritten to this proxy's global name and forwarded,
/// so the server really does deallocate it. Nothing told either map, and the
/// connection's record went on claiming the statement was held. The next `Bind`
/// of that SQL therefore took the already-prepared path and named a statement
/// the server had dropped: `26000 prepared statement "pgprox_..." does not
/// exist`.
///
/// It outlives the session that caused it. The connection goes back to the pool
/// still mis-recorded, so the next session to bind that SQL on it fails too,
/// until the connection is reaped or the entry is evicted. That is the same
/// shape as `M15.3`'s `DISCARD ALL`, in the one path that fixed left open, and
/// `M16.7`'s note about under-clearing producing "does not exist on a
/// connection the proxy thought was warm" describes it exactly.
///
/// Both maps, for the reasons [`observe_statement`] gives: the client knows it
/// closed the statement and will re-parse, and the server has dropped it.
pub fn on_close(
    session: &mut SessionStatements,
    connection: &mut ConnectionStatements,
    client_name: &str,
) -> bool {
    let Some(closed) = session.close(client_name) else {
        return false;
    };
    connection.forget(&closed.global);
    true
}

/// Makes sure the target connection holds the statement a `Bind` names.
///
/// # Errors
///
/// Fails when the client binds a statement it never parsed. Postgres would
/// refuse it too, but not for the same reason: here the name cannot even be
/// translated, so forwarding it would send the client's private name to a
/// server that has never seen it.
pub fn before_bind(
    session: &SessionMemory,
    connection: &mut ConnectionMemory,
    client_name: &str,
) -> Result<Vec<Step>, ClientError> {
    let Some(statement) = session.statements.get(client_name) else {
        return Err(ClientError::ProtocolViolation(
            "bind names a statement this session never parsed",
        ));
    };

    Ok(match connection.statements.prepare_for(&statement.global) {
        Preparation::AlreadyHeld => Vec::new(),
        // Preparation is non_exhaustive. A variant added later must not take
        // the "already held" path by default, because that is the one that
        // sends a Bind for a statement the server does not have.
        Preparation::Replay { evict } => {
            // Evictions first, so the connection never holds more than its cap
            // even for the duration of one prepare. A server that refused the
            // Parse for want of memory is the failure this ordering avoids.
            let mut steps: Vec<Step> = evict.into_iter().map(Step::Close).collect();
            steps.push(Step::Prepare {
                global: statement.global.clone(),
                sql: statement.sql.clone(),
            });
            steps
        }
        _ => {
            return Err(ClientError::ProtocolViolation(
                "the statement map asked for something this session cannot do",
            ));
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use pgprox_pool::pin::Replayable;

    fn session_with(settings: &[(&str, &str)]) -> SessionMemory {
        let mut memory = SessionMemory::default();
        for (name, value) in settings {
            memory.params.record(name, value);
        }
        memory
    }

    fn connection_with(settings: &[(&str, &str)]) -> ConnectionMemory {
        let mut memory = ConnectionMemory::default();
        for (name, value) in settings {
            memory.params.record(name, value);
        }
        memory
    }

    fn capped(cap: usize) -> ConnectionMemory {
        ConnectionMemory {
            params: SessionParams::new(),
            statements: ConnectionStatements::new(StatementConfig {
                per_connection_cap: cap,
            }),
        }
    }

    #[test]
    fn a_session_setting_is_replayed_onto_a_connection_that_lacks_it() {
        let session = session_with(&[("search_path", "tenant_acme")]);
        let connection = ConnectionMemory::default();

        assert_eq!(
            on_acquire(&session, &connection),
            vec![Step::Run("SET search_path = tenant_acme".to_owned())]
        );
    }

    #[test]
    fn a_value_that_needs_quoting_gets_it() {
        // A search_path of "a, b" split across a naive replay would set a
        // path nobody asked for, and the second half would be a syntax error
        // only some of the time.
        let session = session_with(&[("search_path", "a, b")]);
        assert_eq!(
            on_acquire(&session, &ConnectionMemory::default()),
            vec![Step::Run("SET search_path = 'a, b'".to_owned())]
        );
    }

    #[test]
    fn a_connection_that_already_matches_costs_nothing() {
        // The common case in a warm pool serving one tenant, and the reason
        // replay is a difference rather than a full re-set.
        let session = session_with(&[("search_path", "tenant_acme")]);
        let connection = connection_with(&[("search_path", "tenant_acme")]);

        assert!(on_acquire(&session, &connection).is_empty());
    }

    #[test]
    fn a_setting_this_session_never_made_is_reset() {
        // The leak that runs the other way. A connection carrying another
        // tenant's search_path would answer this session from the wrong
        // schema, and every table name would still resolve.
        let session = SessionMemory::default();
        let connection = connection_with(&[("search_path", "tenant_globex")]);

        assert_eq!(
            on_acquire(&session, &connection),
            vec![Step::Run("RESET search_path".to_owned())]
        );
    }

    #[test]
    fn recording_the_replay_makes_the_next_acquire_free() {
        let session = session_with(&[("search_path", "tenant_acme")]);
        let mut connection = connection_with(&[("search_path", "tenant_globex")]);

        assert!(!on_acquire(&session, &connection).is_empty());
        replayed(&session, &mut connection);
        assert!(
            on_acquire(&session, &connection).is_empty(),
            "the same settings were replayed twice onto one connection"
        );
    }

    #[test]
    fn recording_the_replay_forgets_what_the_previous_session_left() {
        let session = SessionMemory::default();
        let mut connection = connection_with(&[("search_path", "tenant_globex")]);

        replayed(&session, &mut connection);
        assert_eq!(
            connection.params.get("search_path"),
            None,
            "a reset parameter stayed on the connection's books"
        );
    }

    #[test]
    fn a_statement_the_connection_lacks_is_parsed_before_the_bind() {
        let mut session = SessionMemory::default();
        session.statements.parse("S_1", "SELECT $1");
        let mut connection = capped(10);

        let steps = before_bind(&session, &mut connection, "S_1").unwrap();
        let global = GlobalName::for_sql("SELECT $1");
        assert_eq!(
            steps,
            vec![Step::Prepare {
                global,
                sql: "SELECT $1".to_owned(),
            }]
        );
    }

    #[test]
    fn a_statement_the_connection_already_holds_is_not_parsed_again() {
        // Without this the mapping would be pure overhead: every Bind would
        // carry a Parse and the round trip saved by preparing would be spent
        // preparing.
        let mut session = SessionMemory::default();
        session.statements.parse("S_1", "SELECT $1");
        let mut connection = capped(10);

        before_bind(&session, &mut connection, "S_1").unwrap();
        assert!(
            before_bind(&session, &mut connection, "S_1")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn two_clients_naming_the_same_sql_differently_share_one_server_statement() {
        // The property that makes this affordable at five thousand tenants
        // running the same application: the name comes from the SQL.
        let mut pgx = SessionMemory::default();
        pgx.statements.parse("stmtcache_1", "SELECT $1");
        let mut jdbc = SessionMemory::default();
        jdbc.statements.parse("S_1", "SELECT $1");
        let mut connection = capped(10);

        assert_eq!(
            before_bind(&pgx, &mut connection, "stmtcache_1")
                .unwrap()
                .len(),
            1
        );
        assert!(
            before_bind(&jdbc, &mut connection, "S_1")
                .unwrap()
                .is_empty(),
            "the same SQL was prepared twice under two client names"
        );
    }

    #[test]
    fn two_clients_reusing_one_name_for_different_sql_do_not_collide() {
        // The failure this mapping exists to prevent. Both sessions call it
        // S_1, and passing that name through would give one of them the
        // other's query.
        let mut first = SessionMemory::default();
        first.statements.parse("S_1", "SELECT 1");
        let mut second = SessionMemory::default();
        second.statements.parse("S_1", "SELECT 2");
        let mut connection = capped(10);

        let a = before_bind(&first, &mut connection, "S_1").unwrap();
        let b = before_bind(&second, &mut connection, "S_1").unwrap();
        assert_ne!(a, b, "two different queries mapped to one server statement");
        assert_eq!(b.len(), 1, "the second session's SQL was never prepared");
    }

    #[test]
    fn eviction_is_sent_before_the_parse_that_needed_the_room() {
        // A server that refused the Parse for want of memory is what this
        // ordering avoids: the connection never holds more than its cap, even
        // for the duration of one prepare.
        let mut session = SessionMemory::default();
        session.statements.parse("a", "SELECT 1");
        session.statements.parse("b", "SELECT 2");
        let mut connection = capped(1);

        before_bind(&session, &mut connection, "a").unwrap();
        let steps = before_bind(&session, &mut connection, "b").unwrap();

        assert!(
            matches!(steps.first(), Some(Step::Close(_))),
            "the parse came before the eviction that made room for it: {steps:?}"
        );
        assert!(matches!(steps.last(), Some(Step::Prepare { .. })));
    }

    #[test]
    fn binding_a_statement_that_was_never_parsed_is_refused() {
        // Not the same error Postgres would give: here the client's private
        // name cannot even be translated, so forwarding it would send a name
        // the server has never seen.
        let session = SessionMemory::default();
        let mut connection = capped(10);

        assert_eq!(
            before_bind(&session, &mut connection, "S_1").unwrap_err(),
            ClientError::ProtocolViolation("bind names a statement this session never parsed")
        );
    }

    #[test]
    fn a_closed_statement_can_no_longer_be_bound() {
        let mut session = SessionMemory::default();
        session.statements.parse("S_1", "SELECT $1");
        session.statements.close("S_1");
        let mut connection = capped(10);

        assert!(before_bind(&session, &mut connection, "S_1").is_err());
    }

    #[test]
    fn an_unreplayable_setting_is_never_recorded_for_replay() {
        // It pins instead. Recording it here as well would replay a setting
        // whose effect the allowlist says cannot be reproduced, which is worse
        // than not replaying it: the session would look correct and not be.
        let mut session = SessionMemory::default();
        session
            .params
            .observe_statement("SET work_mem = '64MB'", Replayable::DEFAULT);

        assert!(
            on_acquire(&session, &ConnectionMemory::default()).is_empty(),
            "a parameter outside the replayable allowlist was replayed"
        );
    }

    #[test]
    fn discard_all_makes_both_maps_forget() {
        // The desync this closes. `DISCARD ALL` deallocates every prepared
        // statement on the connection, and until this existed both maps went on
        // believing otherwise, so the next `Bind` named a global the server had
        // just dropped.
        let mut session = SessionMemory::default();
        let mut connection = capped(10);

        session.statements.parse("S_1", "SELECT $1");
        let steps = before_bind(&session, &mut connection, "S_1").unwrap();
        assert!(!steps.is_empty(), "the setup did not prepare anything");
        assert!(!connection.statements.is_empty());

        assert!(observe_statement(
            &mut session.statements,
            &mut connection.statements,
            "DISCARD ALL"
        ));

        assert!(
            connection.statements.is_empty(),
            "the connection still believes it holds statements the server dropped"
        );
        assert!(
            before_bind(&session, &mut connection, "S_1").is_err(),
            "a name the client must re-parse was still bindable"
        );
    }

    #[test]
    fn a_protocol_close_makes_both_maps_forget() {
        // The same desync as `DISCARD ALL`, through the door that fix left
        // open. The client's `Close` is rewritten to the global name and
        // forwarded, so the server deallocates it; a connection that went on
        // claiming to hold it answered the next `Bind` with `26000`.
        let mut session = SessionMemory::default();
        let mut connection = capped(10);

        session.statements.parse("S_1", "SELECT $1");
        before_bind(&session, &mut connection, "S_1").unwrap();
        assert!(!connection.statements.is_empty(), "nothing was prepared");

        assert!(on_close(
            &mut session.statements,
            &mut connection.statements,
            "S_1"
        ));

        assert!(
            connection.statements.is_empty(),
            "the connection still believes it holds a statement the server dropped"
        );

        // And the connection is not poisoned for whoever borrows it next: a
        // session that prepares the same SQL gets a real `Parse` rather than
        // the already-held path. This is the part that outlives the session
        // that closed it.
        let mut next = SessionMemory::default();
        next.statements.parse("other_name", "SELECT $1");
        assert!(
            matches!(
                before_bind(&next, &mut connection, "other_name")
                    .unwrap()
                    .last(),
                Some(Step::Prepare { .. })
            ),
            "the next session to borrow this connection would bind a dropped statement"
        );
    }

    #[test]
    fn closing_a_statement_this_session_never_parsed_forgets_nothing() {
        // A `Close` of an unknown name is not an error the proxy has to raise:
        // Postgres itself answers `CloseComplete` for one. What it must not do
        // is drop something else, and the return value says nothing happened.
        let mut session = SessionMemory::default();
        let mut connection = capped(10);
        session.statements.parse("S_1", "SELECT $1");
        before_bind(&session, &mut connection, "S_1").unwrap();

        assert!(!on_close(
            &mut session.statements,
            &mut connection.statements,
            "never_parsed"
        ));
        assert!(
            !connection.statements.is_empty(),
            "an unrelated Close dropped a statement the connection still holds"
        );
    }

    #[test]
    fn a_re_parse_after_discard_all_prepares_again_rather_than_assuming() {
        // The half that would still be wrong if only the session map were
        // cleared: the client re-parses, gets the same global name back because
        // the name is derived from the SQL, and the connection must not claim to
        // hold it already.
        let mut session = SessionMemory::default();
        let mut connection = capped(10);

        session.statements.parse("S_1", "SELECT $1");
        before_bind(&session, &mut connection, "S_1").unwrap();
        observe_statement(
            &mut session.statements,
            &mut connection.statements,
            "DISCARD ALL",
        );

        session.statements.parse("S_1", "SELECT $1");
        let steps = before_bind(&session, &mut connection, "S_1").unwrap();
        assert!(
            steps
                .iter()
                .any(|step| matches!(step, Step::Prepare { .. })),
            "the connection skipped the Parse for a statement the server no longer has: {steps:?}"
        );
    }

    #[test]
    fn an_ordinary_statement_forgets_nothing() {
        let mut session = SessionMemory::default();
        let mut connection = capped(10);
        session.statements.parse("S_1", "SELECT $1");
        before_bind(&session, &mut connection, "S_1").unwrap();

        for sql in ["SELECT 1", "DISCARD PLANS", "SET search_path = a"] {
            assert!(
                !observe_statement(&mut session.statements, &mut connection.statements, sql),
                "{sql:?} reported a deallocation"
            );
        }
        assert!(before_bind(&session, &mut connection, "S_1").is_ok());
    }
}
