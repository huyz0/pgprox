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
///
/// # The name is the identity, so its width is the guarantee
///
/// Nothing checks the SQL again once a connection holds a name.
/// [`ConnectionStatements::prepare_for`] is given the name and answers
/// `AlreadyHeld`, and the client's `Bind` then runs whatever that name is
/// attached to. Two different statements sharing a name is two clients running
/// each other's SQL, silently and with correct-looking results.
///
/// This was one 64-bit FNV-1a. FNV is not a cryptographic hash and its
/// finalizer here is a bijection, so colliding the name was colliding FNV-1a-64:
/// meet-in-the-middle work, around 2^32, on input a tenant writes. `M24.7`.
///
/// **The blast radius is one tenant's own pool**, and that is why this is 128
/// bits of a fast hash rather than a keyed or cryptographic one. `PoolKey`
/// carries the server, the database and the role, so the connections a name is
/// held on all belong to one tenant under one role. A tenant that constructs a
/// collision confuses itself. What 128 bits buys is that nobody does it by
/// accident and nobody does it cheaply; what it does not buy is a guarantee
/// against someone who wants it, and a guarantee is not what this needs.
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
        let [low, high] = wide_hash(sql.as_bytes());
        Self(format!("pgprox_{high:016x}{low:016x}"))
    }

    /// The name as it goes on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Longest identifier Postgres accepts, from `NAMEDATALEN - 1`.
///
/// A name past this is not an error: the server truncates it, silently, which
/// would turn a wide hash back into a narrow one at whatever width survived.
/// The constant is here so widening the hash again has something to fail.
pub const MAX_IDENTIFIER: usize = 63;

/// A stable 128-bit hash of the SQL, low half first.
///
/// Two independent 64-bit passes rather than one repeated, which is the
/// simplification this must not become: `(h as u128) << 64 | h` is 128 bits wide
/// and 64 bits strong. The second pass starts from a different offset basis and
/// walks the input backwards, so an input pair that collides under one has no
/// reason to collide under the other.
///
/// A pair rather than a `u128`, which is the same 16 bytes and not the same
/// alignment. `ConnectionStatements` holds one of these and sits inside the
/// session future, and a 16-byte alignment there cascaded into 152 bytes of
/// padding: `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds`
/// went from 5,048 to 5,200 against a 5,120 ceiling. The ceiling is a constant
/// and the layout was the thing that could move.
fn wide_hash(bytes: &[u8]) -> [u64; 2] {
    // The published FNV-1a offset basis, and a second one that is simply a
    // different constant. The basis only has to differ; nothing about the
    // original value is load-bearing.
    const SECOND_BASIS: u64 = 0x9E37_79B9_7F4A_7C15;

    [
        stable_hash(bytes),
        hash_from(SECOND_BASIS, bytes.iter().rev().copied()),
    ]
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

    hash_from(FNV_OFFSET, bytes.iter().copied())
}

/// FNV-1a from a given basis, with a `SplitMix64` finalizer.
///
/// The basis and the iteration order are the two things [`wide_hash`] varies
/// between its passes.
fn hash_from(basis: u64, bytes: impl Iterator<Item = u8>) -> u64 {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = basis;
    for byte in bytes {
        hash ^= u64::from(byte);
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

/// Whether a statement deallocates every prepared statement on the connection.
///
/// `DISCARD ALL` and `DEALLOCATE ALL` both do, and they are the only two forms
/// that matter here. The narrower ones cannot: `DISCARD PLANS` releases cached
/// plans and leaves the statements, and `DEALLOCATE name` names a statement the
/// client prepared with SQL `PREPARE`, which is not a name this proxy ever
/// hands out.
///
/// # Why the maps have to be told
///
/// The two sides share one namespace in Postgres, so `DISCARD ALL` drops the
/// statements this proxy prepared with a protocol `Parse` as surely as the
/// ones the client prepared with SQL. A map that did not hear about it would go
/// on believing a connection holds statements the server has dropped, and the
/// next `Bind` would name one of them.
///
/// # Read from the client's SQL rather than from the server's answer
///
/// pgbouncer does this on the `CommandComplete` tag, which is the server
/// reporting what it did rather than the client saying what it asked for. The
/// difference shows up when a `DEALLOCATE ALL` is rolled back: the tag came
/// back, the statements are still there, and both approaches over-clear. That
/// is the safe direction. Under-clearing is the one that produces "prepared
/// statement does not exist" on a connection the proxy thought was warm, and
/// reading the client's SQL cannot under-clear, because the statement either
/// ran or errored and an error leaves the maps clearable but correct.
#[must_use]
pub fn deallocates_everything(sql: &str) -> bool {
    use pgprox_core::sql::{Lexer, Token};

    // Through the lexer for every word, not only the first. Trivia was
    // previously skipped once up front and the rest read with
    // `split_whitespace`, which is comment-blind past that point:
    // `DISCARD /* c */ ALL` read `/*` as the second word and never reached
    // `ALL`, so a client tagging its own `DISCARD ALL` the way this crate's
    // own hint comments are written left both maps believing statements the
    // server had already dropped were still there.
    let mut lexer = Lexer::new(sql);
    let Some(Token::Word(verb)) = lexer.next() else {
        return false;
    };
    if !verb.eq_ignore_ascii_case("discard") && !verb.eq_ignore_ascii_case("deallocate") {
        return false;
    }

    // `DEALLOCATE PREPARE ALL` is legal and means the same thing; the keyword
    // is noise the grammar allows.
    let mut what = match lexer.next() {
        Some(Token::Word(w)) => w,
        _ => "",
    };
    if what.eq_ignore_ascii_case("prepare") {
        what = match lexer.next() {
            Some(Token::Word(w)) => w,
            _ => "",
        };
    }
    // A trailing semicolon is its own `Token::Semicolon`, not part of the
    // word, so nothing needs trimming off `what` the way a raw scan would.
    what.eq_ignore_ascii_case("all")
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
    /// A hash of the SQL this connection's unnamed statement currently holds,
    /// or zero for none.
    ///
    /// # Still 64 bits, and that is a measurement rather than an oversight
    ///
    /// A collision here answers `holds_unnamed` true for SQL the connection
    /// does not hold, so the `Parse` is skipped and the client's `Bind` runs
    /// whatever the previous session on this connection parsed unnamed. That is
    /// the same defect `M24.7` widened [`GlobalName`] for, with the same blast
    /// radius: one tenant, one role, one database, since that is what a
    /// `PoolKey` is.
    ///
    /// Widening [`GlobalName`] cost nothing, because it is a `String` either
    /// way. Widening this one costs 32 bytes of session future, measured:
    /// `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` goes
    /// from 5,112 to 5,144 against a ceiling of 5,120. The ceiling is a
    /// constant and non-negotiable 2 says it does not move, so the honest
    /// answer is that this half is not fixed and this is what it would take.
    ///
    /// Anything that buys back 32 bytes of that future unblocks it. The pair
    /// has to be `[u64; 2]` rather than `u128` when it happens: a 16-byte
    /// alignment here cascaded to 152 bytes rather than 32.
    ///
    /// Outside `held` on purpose: the unnamed statement has no name to be held
    /// under, the next `Parse` of it replaces it rather than adding to it, and
    /// it is not what `per_connection_cap` is counting. See `note_unnamed`.
    ///
    /// A hash rather than the SQL, and eight bytes rather than a `String`'s
    /// twenty-four, because this struct is inside `Upstreamed`, which the
    /// session holds across every await in the relay loop. The `String` cost 56
    /// bytes of session future and there were 16;
    /// `one_session_costs_less_than_the_slab_buffer_it_no_longer_holds` said so.
    ///
    /// The sentinel is safe in the direction it can be wrong. SQL that happens
    /// to hash to zero reads as "nothing here", which sends a `Parse` that was
    /// not needed, and a `Parse` of the unnamed statement is always legal
    /// because it replaces rather than collides.
    unnamed: u64,
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
            unnamed: 0,
            tick: 0,
        }
    }

    /// Records that this connection's unnamed statement is now this SQL.
    ///
    /// `M20.6`. The unnamed statement is not one of `held`, and must not be:
    /// it has no name to be held under, it is replaced by the next `Parse` of
    /// it rather than coexisting, and it does not survive being closed the way
    /// a named one does. Counting it against `per_connection_cap` would evict
    /// real statements to make room for something the server does not keep.
    pub fn note_unnamed(&mut self, sql: &str) {
        self.unnamed = stable_hash(sql.as_bytes());
    }

    /// Whether this connection's unnamed statement is already this SQL.
    ///
    /// False on a connection this session has not used for it yet, which is the
    /// case a `Bind` of the unnamed statement after a change of connection has
    /// to be told about.
    #[must_use]
    pub fn holds_unnamed(&self, sql: &str) -> bool {
        let hash = stable_hash(sql.as_bytes());
        hash != 0 && self.unnamed == hash
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
        // `DISCARD ALL` takes the unnamed statement with the rest, and a
        // connection that went on believing in it would skip the `Parse` the
        // next `Bind` of it needs.
        self.unnamed = 0;
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

    #[test]
    fn the_two_forms_that_deallocate_everything_are_recognised() {
        for sql in [
            "DISCARD ALL",
            "discard all",
            "DEALLOCATE ALL",
            "deallocate all;",
            "DEALLOCATE PREPARE ALL",
            "  DISCARD   ALL  ",
            "/* pgx */ DISCARD ALL",
            "-- reset the session\nDISCARD ALL",
        ] {
            assert!(deallocates_everything(sql), "{sql:?} was not recognised");
        }
    }

    #[test]
    fn a_comment_between_the_words_does_not_hide_the_deallocation() {
        // Trivia was previously skipped once, up front, and every later word
        // read with `split_whitespace`, which is comment-blind past that
        // point: `DISCARD /* c */ ALL` read `/*` as the second word and never
        // reached `ALL`, so both maps kept believing statements the server
        // had already dropped were still there.
        for sql in [
            "DISCARD /* c */ ALL",
            "DEALLOCATE /* c */ ALL",
            "DEALLOCATE PREPARE /* c */ ALL",
            "DEALLOCATE /* c */ PREPARE ALL",
            "DISCARD -- c\nALL",
        ] {
            assert!(deallocates_everything(sql), "{sql:?} was not recognised");
        }
    }

    #[test]
    fn the_narrower_forms_leave_the_statements_alone() {
        // `DISCARD PLANS` releases cached plans and keeps the statements, and
        // `DEALLOCATE name` names one the client prepared with SQL `PREPARE`,
        // which is never a name this proxy handed out. Clearing on either would
        // throw away a warm connection's whole map for nothing.
        for sql in [
            "DISCARD PLANS",
            "DISCARD SEQUENCES",
            "DISCARD TEMP",
            "DEALLOCATE my_statement",
            "DEALLOCATE PREPARE my_statement",
            "SELECT 1",
            "",
            "DISCARD",
            "DEALLOCATE",
            "discarding_is_not_a_verb ALL",
        ] {
            assert!(
                !deallocates_everything(sql),
                "{sql:?} cleared the statement maps"
            );
        }
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
        //
        // The low half is what this pinned before `M24.7` widened it, which is
        // visible here on purpose: the second pass is appended rather than
        // mixed in, so the first is still the published FNV-1a-64 and this
        // pinning still says what it said.
        assert_eq!(
            name("SELECT $1").as_str(),
            "pgprox_1079e9518a147011533e5fdc2f41216f"
        );
        assert!(
            name("SELECT $1").as_str().ends_with("533e5fdc2f41216f"),
            "the low half is no longer the hash this pinned before M24.7"
        );
    }

    #[test]
    fn a_global_name_fits_an_identifier_postgres_will_not_truncate() {
        // `M24.7`. A name past NAMEDATALEN - 1 is not an error: the server
        // truncates it, silently, which turns a wide hash back into a narrow
        // one at whatever width survived. Nothing checked this while the name
        // was 23 characters, and widening it is exactly when it starts to
        // matter.
        for sql in ["", "SELECT 1", &"x".repeat(4096)] {
            let name = GlobalName::for_sql(sql);
            assert!(
                name.as_str().len() <= MAX_IDENTIFIER,
                "{} characters, which Postgres truncates to {MAX_IDENTIFIER}",
                name.as_str().len()
            );
        }
    }

    #[test]
    fn the_two_halves_of_a_name_are_different_functions() {
        // The simplification this must not become. `(h as u128) << 64 | h` is
        // 128 bits wide and 64 bits strong, and every other test here would
        // pass for it: identical SQL would still share a name, different SQL
        // would still differ, and the pinned value would still be pinned.
        //
        // Asserted over a spread rather than one input, because two functions
        // agreeing once is a coincidence and agreeing every time is one
        // function.
        let mut agreements = 0;
        for i in 0..256 {
            let sql = format!("SELECT {i}");
            let text = GlobalName::for_sql(&sql);
            let (high, low) = text.as_str()["pgprox_".len()..].split_at(16);
            if high == low {
                agreements += 1;
            }
        }
        assert_eq!(
            agreements, 0,
            "the two halves of the name are the same 64 bits twice"
        );
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

    #[test]
    fn the_unnamed_statement_is_tracked_apart_from_the_ones_that_are_held() {
        // `M20.6`. The unnamed statement has no name to be held under, is
        // replaced by the next `Parse` of it rather than added to, and does not
        // survive a `Close`. Putting it in `held` would let it evict a real
        // statement to make room for something the server does not keep.
        let mut conn = ConnectionStatements::new(config(1));

        assert!(
            !conn.holds_unnamed("SELECT 1"),
            "a fresh connection has none"
        );
        conn.note_unnamed("SELECT 1");
        assert!(conn.holds_unnamed("SELECT 1"));
        assert!(
            !conn.holds_unnamed("SELECT 2"),
            "a different statement read as the same one"
        );

        // The cap is untouched: one named statement still fits beside it.
        assert!(conn.is_empty(), "the unnamed statement took a held slot");
        assert_eq!(conn.len(), 0);

        // Replaced rather than accumulated, which is the whole of what makes it
        // the unnamed statement.
        conn.note_unnamed("SELECT 2");
        assert!(!conn.holds_unnamed("SELECT 1"));
        assert!(conn.holds_unnamed("SELECT 2"));
    }

    #[test]
    fn discard_all_takes_the_unnamed_statement_with_the_rest() {
        // A connection that went on believing in it would skip the `Parse` the
        // next `Bind` of it needs, which is `M20.1`'s failure in a new place.
        let mut conn = ConnectionStatements::new(config(4));
        conn.note_unnamed("SELECT 1");
        conn.forget_all();
        assert!(!conn.holds_unnamed("SELECT 1"));
    }
}
