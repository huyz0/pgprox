//! Mapping client statement names onto server ones.
//!
//! # Why this is mandatory rather than an optimisation
//!
//! Every modern driver uses named `Parse`. pgx, asyncpg, JDBC and npgsql all
//! prepare by default, so a proxy that pinned on a named statement would pin
//! nearly every real session, and transaction pooling would quietly become
//! session pooling. The ratio this whole design exists for depends on this
//! module working. See ADR 0011.
//!
//! # The shape
//!
//! A client names its statements whatever it likes, and two clients routinely
//! choose the same name for different SQL: `pgx` uses `stmtcache_1`, JDBC uses
//! `S_1`. Those names cannot be passed through, because two sessions sharing an
//! upstream connection would collide.
//!
//! So the proxy rewrites. The global name is derived from a hash of the SQL, so
//! identical SQL from any session maps to one name. Each connection tracks
//! which global names it holds, and on acquire the proxy replays any `Parse`
//! the target does not already have.
//!
//! Deriving the name from the SQL rather than allocating a counter is what
//! makes two clients preparing the same query share one server-side statement,
//! which at five thousand tenants running the same application is most of them.
//!
//! # Why the map is bounded
//!
//! Postgres holds prepared statements in backend memory, so an unbounded map is
//! a slow leak multiplied by every connection. Eviction is LRU at a configured
//! cap.
//!
//! That cap is a correctness knob, not a performance one. Set too low it causes
//! constant re-preparation; set too high it grows backend memory across
//! thousands of connections until the server starts refusing work.
//!
//! # What this module does not do
//!
//! It does not touch bytes. Rewriting a name inside a `Parse` or `Bind` message
//! is `pgprox-proto`'s job, and joining the two is `pgprox-session`'s. This is
//! the bookkeeping: which name, and does this connection already hold it. See
//! the layering note in the M5 backlog.

use std::collections::HashMap;

/// How the statement map is tuned.
#[derive(Clone, Copy, Debug)]
pub struct StatementConfig {
    /// How many prepared statements one upstream connection may hold.
    ///
    /// A correctness knob, not a performance one. See the module docs.
    pub per_connection_cap: usize,
}

impl Default for StatementConfig {
    fn default() -> Self {
        // Enough for an application's whole query set several times over, and
        // small enough that a thousand connections holding a full map is still
        // a bounded amount of backend memory.
        Self {
            per_connection_cap: 128,
        }
    }
}

/// The global name a piece of SQL is prepared under.
///
/// Derived from the SQL, so identical SQL always yields the same name and two
/// sessions preparing it share one server-side statement.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct GlobalName(String);

impl GlobalName {
    /// The name for a piece of SQL.
    ///
    /// ```
    /// use pgprox_pool::statements::GlobalName;
    ///
    /// let a = GlobalName::for_sql("SELECT $1");
    /// let b = GlobalName::for_sql("SELECT $1");
    /// assert_eq!(a, b, "identical SQL must share a name");
    /// assert_ne!(a, GlobalName::for_sql("SELECT $2"));
    /// ```
    #[must_use]
    pub fn for_sql(sql: &str) -> Self {
        // A fixed prefix so a proxy-created statement is recognisable in
        // `pg_prepared_statements`, which is where an operator looks when
        // backend memory is climbing.
        Self(format!("pgprox_{:016x}", stable_hash(sql.as_bytes())))
    }

    /// The name as it goes on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable 64-bit hash of the SQL.
///
/// FNV-1a with a `SplitMix64` finalizer, the same construction as the
/// rendezvous hash in `pgprox-core` and for the same reason: `DefaultHasher` is
/// explicitly not stable across Rust releases, and a name that changed between
/// compiler versions would mean two nodes in a rolling upgrade preparing the
/// same SQL under different names. That is not a correctness bug, since each
/// connection tracks its own names, but it would silently double the
/// server-side statement count during every deploy.
fn stable_hash(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    let mut z = hash.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// What a session has prepared, by the names it chose.
///
/// One per client session. Maps the client's own statement names onto global
/// ones, so a `Bind` naming `S_1` can be rewritten to whatever the SQL of that
/// `Parse` hashed to.
#[derive(Clone, Debug, Default)]
pub struct SessionStatements {
    /// Client name to the SQL it was prepared with.
    prepared: HashMap<String, PreparedStatement>,
}

/// One statement a session has prepared.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PreparedStatement {
    /// The SQL, kept so it can be replayed onto another connection.
    pub sql: String,
    /// The name it is prepared under upstream.
    pub global: GlobalName,
}

impl SessionStatements {
    /// A session that has prepared nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a `Parse`, returning the name to rewrite it to.
    ///
    /// Re-preparing under a name the session already used replaces it, which is
    /// what a driver rotating its statement cache does.
    pub fn parse(&mut self, client_name: &str, sql: &str) -> GlobalName {
        let global = GlobalName::for_sql(sql);
        self.prepared.insert(
            client_name.to_owned(),
            PreparedStatement {
                sql: sql.to_owned(),
                global: global.clone(),
            },
        );
        global
    }

    /// The statement a client name refers to.
    ///
    /// [`None`] for a name the session never prepared, which the caller must
    /// pass through untouched rather than inventing a name for: the server's
    /// own error is the correct answer, and a rewritten name would turn a
    /// clear "prepared statement does not exist" into a confusing one.
    #[must_use]
    pub fn get(&self, client_name: &str) -> Option<&PreparedStatement> {
        self.prepared.get(client_name)
    }

    /// Forgets a statement, as `Close` does.
    pub fn close(&mut self, client_name: &str) -> Option<PreparedStatement> {
        self.prepared.remove(client_name)
    }

    /// Forgets everything, as `DEALLOCATE ALL` does.
    pub fn close_all(&mut self) {
        self.prepared.clear();
    }

    /// How many statements this session has prepared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.prepared.len()
    }

    /// Whether the session has prepared nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.prepared.is_empty()
    }

    /// Every statement this session holds, for replay onto a connection.
    pub fn iter(&self) -> impl Iterator<Item = &PreparedStatement> {
        self.prepared.values()
    }
}

/// What one upstream connection has prepared.
///
/// Must stay in step with the server's actual prepared statement set. A desync
/// produces errors that look like a driver bug, which is why eviction here
/// means telling the server to deallocate rather than merely forgetting.
#[derive(Clone, Debug)]
pub struct ConnectionStatements {
    config: StatementConfig,
    /// Global name to the tick it was last used at.
    held: HashMap<GlobalName, u64>,
    /// A logical clock, incremented on every use.
    ///
    /// Not a timestamp: LRU only needs an order, and a counter cannot go
    /// backwards the way a clock can.
    tick: u64,
}

/// What a caller must do before a `Bind` can be sent.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Preparation {
    /// The connection already holds it. Send the `Bind` as it is.
    AlreadyHeld,
    /// Send a `Parse` first, then the `Bind`.
    Replay {
        /// Statements to deallocate to make room, in eviction order.
        ///
        /// Emptied before the `Parse` is sent, so the connection never holds
        /// more than its cap even momentarily.
        evict: Vec<GlobalName>,
    },
}

impl ConnectionStatements {
    /// A freshly opened connection, holding nothing.
    #[must_use]
    pub fn new(config: StatementConfig) -> Self {
        Self {
            config,
            held: HashMap::new(),
            tick: 0,
        }
    }

    /// Whether this connection holds a statement.
    #[must_use]
    pub fn holds(&self, global: &GlobalName) -> bool {
        self.held.contains_key(global)
    }

    /// How many statements are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// What must happen before this statement can be bound.
    ///
    /// Touching the statement counts as a use whether or not it was held, so a
    /// statement being prepared right now is the most recently used and cannot
    /// be evicted by the very call that adds it.
    pub fn prepare_for(&mut self, global: &GlobalName) -> Preparation {
        self.tick += 1;

        if let Some(used_at) = self.held.get_mut(global) {
            *used_at = self.tick;
            return Preparation::AlreadyHeld;
        }

        // Room for one more, evicting least-recently-used first.
        let mut evict = Vec::new();
        while self.held.len() + 1 > self.config.per_connection_cap {
            let Some(victim) = self.least_recently_used() else {
                // A cap of zero. Nothing can be held, so nothing is evicted and
                // every statement is prepared afresh: slow, and not a desync.
                break;
            };
            self.held.remove(&victim);
            evict.push(victim);
        }

        if self.config.per_connection_cap > 0 {
            self.held.insert(global.clone(), self.tick);
        }
        Preparation::Replay { evict }
    }

    /// The least recently used statement.
    ///
    /// Ties break on the name so eviction is deterministic, which matters when
    /// a fresh connection prepares several statements at the same tick.
    fn least_recently_used(&self) -> Option<GlobalName> {
        self.held
            .iter()
            .min_by(|(a_name, a_tick), (b_name, b_tick)| {
                a_tick.cmp(b_tick).then_with(|| a_name.cmp(b_name))
            })
            .map(|(name, _)| name.clone())
    }

    /// Forgets a statement, after the server has been told to deallocate it.
    pub fn forget(&mut self, global: &GlobalName) {
        self.held.remove(global);
    }

    /// Forgets everything, as `DISCARD ALL` on this connection does.
    pub fn forget_all(&mut self) {
        self.held.clear();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// `M14.22`. Three mutants survived in this file. Two are accessors nobody
    /// asserted; the third is the one that matters.
    #[test]
    fn use_order_decides_eviction_rather_than_name_order() {
        // `prepare_for` opens with `self.tick += 1`, and `tick` starts at zero,
        // so `*=` freezes it there. Every held statement then carries the same
        // use time, and `least_recently_used` breaks ties on the name: the
        // eviction policy silently becomes "alphabetically first" instead of
        // "least recently used". The cache still works, still respects its cap,
        // and evicts the wrong statement, which shows up as a re-prepare storm
        // on whatever the busiest statement happens to hash to.
        //
        // Names are hashes of the SQL, so the deciding case has to be found
        // rather than written: the statement that is touched again must sort
        // *before* the one that should be evicted. Then use order and name
        // order disagree, and only use order gives the right victim.
        let (keep, evict_me) = (0..200)
            .flat_map(|i| (0..200).map(move |j| (i, j)))
            .map(|(i, j)| {
                (
                    GlobalName::for_sql(&format!("SELECT {i}")),
                    GlobalName::for_sql(&format!("SELECT {j}, 2")),
                )
            })
            .find(|(a, b)| a.as_str() < b.as_str())
            .unwrap();

        let mut held = ConnectionStatements::new(config(2));
        let third = GlobalName::for_sql("SELECT 'third'");

        assert!(matches!(
            held.prepare_for(&keep),
            Preparation::Replay { .. }
        ));
        assert!(matches!(
            held.prepare_for(&evict_me),
            Preparation::Replay { .. }
        ));

        // Touch the one that sorts first, so it is the most recently used while
        // still being alphabetically first. This is the whole test.
        assert!(matches!(held.prepare_for(&keep), Preparation::AlreadyHeld));

        let outcome = held.prepare_for(&third);
        assert_eq!(
            outcome,
            Preparation::Replay {
                evict: vec![evict_me]
            },
            "eviction followed name order rather than use order"
        );
        assert!(
            held.holds(&keep),
            "the most recently used statement was evicted"
        );
        assert!(held.holds(&third));
    }

    #[test]
    fn a_session_reports_how_many_statements_it_has_prepared() {
        // `SessionStatements::len` could return `1` unconditionally. Nothing
        // asked it for any other number, so a constant matched every case that
        // was tested and none that mattered.
        let mut session = SessionStatements::new();
        assert_eq!(session.len(), 0);
        assert!(session.is_empty());

        session.parse("s1", "SELECT 1");
        assert_eq!(session.len(), 1);
        assert!(!session.is_empty());

        session.parse("s2", "SELECT 2");
        session.parse("s3", "SELECT 3");
        assert_eq!(
            session.len(),
            3,
            "three distinct client names, three statements"
        );

        // Re-preparing an existing client name replaces rather than adds, which
        // is what a driver rotating its cache does.
        session.parse("s1", "SELECT 99");
        assert_eq!(session.len(), 3);
    }

    #[test]
    fn the_held_set_reports_its_own_size() {
        // `is_empty` could return `true` unconditionally, and the only test
        // that touched it asked an empty set.
        let mut held = ConnectionStatements::new(config(4));
        assert!(held.is_empty());

        held.prepare_for(&GlobalName::for_sql("SELECT one"));
        assert!(
            !held.is_empty(),
            "a set holding a statement called itself empty"
        );

        held.prepare_for(&GlobalName::for_sql("SELECT two"));
        assert!(!held.is_empty());

        held.forget_all();
        assert!(held.is_empty());
    }

    fn config(cap: usize) -> StatementConfig {
        StatementConfig {
            per_connection_cap: cap,
        }
    }

    fn name(sql: &str) -> GlobalName {
        GlobalName::for_sql(sql)
    }

    #[test]
    fn identical_sql_shares_one_global_name() {
        // The saving that matters at five thousand tenants running the same
        // application: one server-side statement rather than five thousand.
        assert_eq!(name("SELECT $1"), name("SELECT $1"));
        assert_eq!(
            name("SELECT * FROM orders WHERE id = $1"),
            name("SELECT * FROM orders WHERE id = $1")
        );
    }

    #[test]
    fn different_sql_gets_different_names() {
        let names = [
            name("SELECT $1"),
            name("SELECT $2"),
            name("SELECT 1"),
            name(""),
            name("select $1"),
        ];
        let mut unique: Vec<&GlobalName> = names.iter().collect();
        unique.sort();
        let count = unique.len();
        unique.dedup();
        assert_eq!(unique.len(), count, "two different statements collided");
    }

    #[test]
    fn whitespace_and_case_are_not_normalised_away() {
        // Deliberate. `SELECT $1` and `select $1` are the same query to a
        // reader, but normalising would mean the proxy deciding two pieces of
        // SQL are equivalent, and it does not parse SQL well enough to promise
        // that. Two names is wasteful; a wrong equivalence is a wrong answer.
        assert_ne!(name("SELECT $1"), name("select $1"));
        assert_ne!(name("SELECT $1"), name("SELECT  $1"));
    }

    #[test]
    fn a_global_name_is_recognisable_as_the_proxys() {
        // An operator watching backend memory climb looks in
        // pg_prepared_statements, and needs to tell ours from the driver's.
        let global = name("SELECT $1");
        assert!(
            global.as_str().starts_with("pgprox_"),
            "{}",
            global.as_str()
        );
        assert!(
            global
                .as_str()
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "a name needing quoting would have to be escaped on the wire"
        );
    }

    #[test]
    fn the_name_is_stable_across_builds() {
        // Pinned, not merely computed. DefaultHasher is not stable across Rust
        // releases, and a name that changed between compiler versions would
        // double the server-side statement count during every rolling upgrade.
        assert_eq!(name("SELECT $1").as_str(), "pgprox_533e5fdc2f41216f");
    }

    #[test]
    fn a_session_maps_its_own_names_onto_global_ones() {
        // Two clients routinely choose the same name for different SQL: pgx
        // uses stmtcache_1, JDBC uses S_1. Passing those through would collide
        // the moment they share a connection.
        let mut session = SessionStatements::new();
        let global = session.parse("S_1", "SELECT $1");

        assert_eq!(global, name("SELECT $1"));
        assert_eq!(session.get("S_1").unwrap().global, global);
        assert_eq!(session.get("S_1").unwrap().sql, "SELECT $1");
        assert_eq!(session.len(), 1);
        assert!(!session.is_empty());
    }

    #[test]
    fn two_sessions_using_the_same_client_name_do_not_collide() {
        let mut a = SessionStatements::new();
        let mut b = SessionStatements::new();
        let from_a = a.parse("S_1", "SELECT * FROM orders");
        let from_b = b.parse("S_1", "DELETE FROM orders");

        assert_ne!(
            from_a, from_b,
            "two sessions' statements collided on one name"
        );
    }

    #[test]
    fn re_preparing_a_name_replaces_it() {
        // What a driver rotating its statement cache does.
        let mut session = SessionStatements::new();
        session.parse("S_1", "SELECT $1");
        session.parse("S_1", "SELECT $2");

        assert_eq!(session.len(), 1);
        assert_eq!(session.get("S_1").unwrap().global, name("SELECT $2"));
    }

    #[test]
    fn an_unknown_client_name_is_reported_rather_than_invented() {
        // The server's own "prepared statement does not exist" is the correct
        // answer. A rewritten name would turn it into a confusing one.
        let session = SessionStatements::new();
        assert_eq!(session.get("never_prepared"), None);
    }

    #[test]
    fn closing_forgets_one_and_closing_all_forgets_everything() {
        let mut session = SessionStatements::new();
        session.parse("S_1", "SELECT $1");
        session.parse("S_2", "SELECT $2");

        let closed = session.close("S_1").unwrap();
        assert_eq!(closed.sql, "SELECT $1");
        assert_eq!(session.len(), 1);
        assert_eq!(session.close("S_1"), None);

        session.close_all();
        assert!(session.is_empty());
    }

    #[test]
    fn a_sessions_statements_can_be_listed_for_replay() {
        let mut session = SessionStatements::new();
        session.parse("S_1", "SELECT $1");
        session.parse("S_2", "SELECT $2");

        let mut sql: Vec<&str> = session.iter().map(|s| s.sql.as_str()).collect();
        sql.sort_unstable();
        assert_eq!(sql, vec!["SELECT $1", "SELECT $2"]);
    }

    #[test]
    fn a_fresh_connection_must_prepare_and_a_warm_one_need_not() {
        let mut conn = ConnectionStatements::new(config(8));
        let global = name("SELECT $1");
        assert!(conn.is_empty());

        assert_eq!(
            conn.prepare_for(&global),
            Preparation::Replay { evict: Vec::new() }
        );
        assert!(conn.holds(&global));
        assert_eq!(conn.len(), 1);

        assert_eq!(conn.prepare_for(&global), Preparation::AlreadyHeld);
        assert_eq!(conn.len(), 1, "a repeat prepare added a second entry");
    }

    #[test]
    fn the_cap_is_never_exceeded_even_momentarily() {
        // The connection must not hold cap+1 while the Parse is in flight, or
        // backend memory exceeds what the cap promised.
        let mut conn = ConnectionStatements::new(config(3));
        for i in 0..10 {
            let global = name(&format!("SELECT {i}"));
            conn.prepare_for(&global);
            assert!(conn.len() <= 3, "held {} with a cap of 3", conn.len());
        }
        assert_eq!(conn.len(), 3);
    }

    #[test]
    fn eviction_takes_the_least_recently_used() {
        let mut conn = ConnectionStatements::new(config(2));
        let a = name("SELECT 1");
        let b = name("SELECT 2");
        let c = name("SELECT 3");

        conn.prepare_for(&a);
        conn.prepare_for(&b);
        // Using `a` again makes `b` the oldest.
        assert_eq!(conn.prepare_for(&a), Preparation::AlreadyHeld);

        let outcome = conn.prepare_for(&c);
        assert_eq!(
            outcome,
            Preparation::Replay {
                evict: vec![b.clone()]
            },
            "the wrong statement was evicted"
        );
        assert!(conn.holds(&a));
        assert!(!conn.holds(&b));
        assert!(conn.holds(&c));
    }

    #[test]
    fn an_evicted_statement_is_never_reported_as_held() {
        // A desync here produces errors that look like a driver bug, so the
        // bookkeeping must not claim a statement the server no longer has.
        let mut conn = ConnectionStatements::new(config(1));
        let a = name("SELECT 1");
        let b = name("SELECT 2");

        conn.prepare_for(&a);
        let outcome = conn.prepare_for(&b);
        assert_eq!(
            outcome,
            Preparation::Replay {
                evict: vec![a.clone()]
            }
        );
        assert!(!conn.holds(&a), "an evicted statement was still held");
        assert_eq!(conn.prepare_for(&a), Preparation::Replay { evict: vec![b] });
    }

    #[test]
    fn the_statement_being_prepared_cannot_evict_itself() {
        // At a cap of one, adding a statement must evict the other one rather
        // than the one being added, which would hold nothing and re-prepare
        // forever.
        let mut conn = ConnectionStatements::new(config(1));
        let a = name("SELECT 1");
        conn.prepare_for(&a);
        assert!(conn.holds(&a));
        assert_eq!(conn.len(), 1);
    }

    #[test]
    fn a_cap_of_zero_prepares_every_time_rather_than_desyncing() {
        // Pathological configuration. It must be slow, not wrong: nothing is
        // held, so nothing is ever wrongly claimed to be held.
        let mut conn = ConnectionStatements::new(config(0));
        let global = name("SELECT 1");

        for _ in 0..3 {
            assert_eq!(
                conn.prepare_for(&global),
                Preparation::Replay { evict: Vec::new() }
            );
            assert!(conn.is_empty());
            assert!(!conn.holds(&global));
        }
    }

    #[test]
    fn eviction_is_deterministic_when_ticks_tie() {
        // A fresh connection preparing several statements can produce equal
        // ticks, and two nodes must evict the same one.
        let mut first = ConnectionStatements::new(config(2));
        let mut second = ConnectionStatements::new(config(2));
        let names: Vec<GlobalName> = (0..5).map(|i| name(&format!("SELECT {i}"))).collect();

        let a: Vec<Preparation> = names.iter().map(|n| first.prepare_for(n)).collect();
        let b: Vec<Preparation> = names.iter().map(|n| second.prepare_for(n)).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn forgetting_matches_what_the_server_was_told() {
        let mut conn = ConnectionStatements::new(config(8));
        let a = name("SELECT 1");
        let b = name("SELECT 2");
        conn.prepare_for(&a);
        conn.prepare_for(&b);

        conn.forget(&a);
        assert!(!conn.holds(&a));
        assert!(conn.holds(&b));

        conn.forget_all();
        assert!(conn.is_empty());
    }

    #[test]
    fn the_default_cap_is_bounded_and_useful() {
        // Large enough for an application's whole query set several times over,
        // small enough that a thousand connections at full map is bounded.
        let default = StatementConfig::default();
        assert!(default.per_connection_cap >= 64);
        assert!(default.per_connection_cap <= 1_024);
    }

    #[test]
    fn a_session_and_a_connection_agree_on_the_name() {
        // The join the session layer makes at M6: the session says which global
        // name a client's Bind means, and the connection says whether a Parse
        // has to be replayed first.
        let mut session = SessionStatements::new();
        let mut conn = ConnectionStatements::new(config(8));

        let global = session.parse("S_1", "SELECT $1");
        assert_eq!(
            conn.prepare_for(&global),
            Preparation::Replay { evict: Vec::new() }
        );

        // The same session, moved to the same connection later, finds it warm.
        let bound = session.get("S_1").unwrap().global.clone();
        assert_eq!(conn.prepare_for(&bound), Preparation::AlreadyHeld);
    }
}
