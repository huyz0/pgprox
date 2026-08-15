//! What this node is serving right now.
//!
//! A node knows only its own clients. Everything else the admin surfaces
//! report comes from gossip, which carries totals rather than connections,
//! because gossiping one entry per client would put a hundred thousand rows on
//! the wire every second.
//!
//! # Why it holds no session
//!
//! Only what a report needs: who, what state, since when. The session itself
//! is a task with a socket, and a registry that owned one would decide when it
//! ended. Registering and deregistering is the session's own job, which is why
//! [`Registration`] deregisters on drop: a session that panicked mid-query
//! would otherwise be listed forever.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Instant;

use pgprox_core::admin::{ClientState, ClientView};
use pgprox_core::ids::{ConnId, NodeId, TenantId};
use pgprox_session::cancel::Registry;

/// One client, as a report sees it.
#[derive(Debug, Clone)]
struct Entry {
    tenant: TenantId,
    node: NodeId,
    state: ClientState,
    since: Instant,
    pinned: Option<String>,
    /// How many upstream connections this tenant's grant allows.
    ///
    /// Kept per session because it arrives with the grant and the registry is
    /// the only thing that outlives one. A shed decision weighs the home
    /// node's usage against the tenant's share of this, so a made-up number
    /// here is a client bounced to a node with no room for it.
    budget: u32,
    /// Fired to ask this one session to leave, for tenant affinity or for
    /// having been idle past the configured timeout.
    ///
    /// The registry holds the signal rather than the session, because both
    /// decisions are the node's and a session is a task on a socket. The
    /// session watches it at the same place it watches a drain, so a client
    /// mid-transaction is not cut off by any of the three.
    ///
    /// One signal for both reasons rather than two: `M74.0` tried two, each
    /// watched by its own `select!` branch, and measured what that cost. A
    /// `tokio::sync::watch::Receiver`'s `changed()` future held across a
    /// relay loop's awaits is not a cheap type, and a second one added 240
    /// bytes to the per-connection future against a budget of 72 remaining,
    /// worse than a `tokio::time::Sleep` held directly would have been. See
    /// `idle` below and ADR 0030.
    close: crate::run::Shutdown,
    /// Which of the two reasons `close` was fired for.
    ///
    /// A plain flag read synchronously when `close` fires, rather than a
    /// second signal awaited alongside it: the session already has to wake up
    /// and decide what to tell the client, so asking "which" costs one atomic
    /// load rather than one more future in the `select!`'s union.
    idle: Arc<std::sync::atomic::AtomicBool>,
}

/// Every client this node is serving.
#[derive(Debug, Default)]
pub struct Sessions {
    live: Mutex<HashMap<ConnId, Entry>>,
    /// A handle to this registry's own `Arc`, so a [`Registration`] can
    /// deregister without the caller having to remember to.
    me: Mutex<Weak<Self>>,
    transactions: AtomicU64,
    pins: AtomicU64,
    sheds: AtomicU64,
    idle_timeouts: AtomicU64,
    /// When each tenant was last shed, most recent last.
    ///
    /// The window the per-tenant rate limit is measured over. Kept here
    /// because this is where a shed happens, and a limit counted anywhere else
    /// would be counting something other than what it limits.
    recent: Mutex<HashMap<TenantId, VecDeque<Instant>>>,
    /// The cancel-key registry, wired in once by [`Sessions::wire_cancels`].
    ///
    /// `M90.5`. `Registration` already deregisters its client on drop so a
    /// session that panicked mid-query is not listed forever — see the
    /// module doc. A cancel key wants the identical guarantee and gets it for
    /// free by reusing this same drop: one `Arc` on this shared, once-per-node
    /// registry costs nothing on the per-connection future `Registration`
    /// lives inside, where a field of its own would have. `Option` rather
    /// than required at construction because `Sessions::new()` has no
    /// argument list to keep test code — the great majority of the crate's
    /// own tests — from having to build a `Registry` it does not otherwise
    /// need.
    cancels: Mutex<Option<Arc<Registry>>>,
}

/// How far back the shed rate limit looks.
///
/// A minute, matching `ShedConfig::max_per_tenant_per_minute`. The two are one
/// rule in two crates, and a window that disagreed with the limit would
/// enforce a number nobody configured.
pub const SHED_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

/// A client's place in the registry, removed when dropped.
///
/// Drop rather than an explicit call, because the explicit call is the one
/// that gets skipped on the path where a session ended badly, which is exactly
/// when a stale row is most confusing.
#[derive(Debug)]
pub struct Registration {
    sessions: Weak<Sessions>,
    conn: ConnId,
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(sessions) = self.sessions.upgrade() {
            sessions.lock().remove(&self.conn);
            // `M90.5`. The connection this client held, if any, may still be
            // in the cancel registry: the clean transaction-boundary release
            // in `serve.rs` only reaches it between transactions, and a
            // session that disconnects mid-transaction never gets there. This
            // is the same guard this drop already is for the row above,
            // applied to the second registry a session can go stale in.
            if let Some(cancels) = sessions
                .cancels
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                cancels.release(self.conn);
            }
        }
    }
}

impl Sessions {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Arc<Self> {
        let sessions = Arc::new(Self::default());
        *sessions.me.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(&sessions);
        sessions
    }

    /// Wires the cancel-key registry a [`Registration`] releases from on
    /// drop.
    ///
    /// `M90.5`. Separate from [`Sessions::new`] rather than a constructor
    /// argument so the many tests that need a `Sessions` but never touch a
    /// cancel key do not also need a `Registry`. Not wiring it is safe:
    /// [`Registration::drop`] simply finds nothing to release, the same as
    /// before this existed.
    pub fn wire_cancels(&self, cancels: Arc<Registry>) {
        *self.cancels.lock().unwrap_or_else(PoisonError::into_inner) = Some(cancels);
    }

    /// Registers a client, until the returned value is dropped.
    ///
    /// `idle` is the flag `close` firing asks the session to read; the caller
    /// holds its own clone to pass into the session alongside `close` itself,
    /// the same way it already holds its own clone of `close` to watch. See
    /// the field's own docs for why this is a flag next to one signal rather
    /// than a second signal.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        &self,
        conn: ConnId,
        tenant: TenantId,
        node: NodeId,
        now: Instant,
        budget: u32,
        close: crate::run::Shutdown,
        idle: Arc<std::sync::atomic::AtomicBool>,
    ) -> Registration {
        self.lock().insert(
            conn,
            Entry {
                tenant,
                node,
                state: ClientState::Idle,
                since: now,
                pinned: None,
                budget,
                close,
                idle,
            },
        );
        Registration {
            sessions: self
                .me
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            conn,
        }
    }

    /// Records what a client is doing now.
    ///
    /// The timestamp moves only when the state does, because `SHOW CLIENTS`
    /// reports how long a client has been in its current state and an operator
    /// looking for a stuck session is reading exactly that column.
    pub fn set_state(&self, conn: ConnId, state: ClientState, now: Instant) {
        if let Some(entry) = self.lock().get_mut(&conn)
            && entry.state != state
        {
            entry.state = state;
            entry.since = now;
        }
    }

    /// Records that a client became pinned, and why.
    ///
    /// Counts only when there is a client to record it against. A session that
    /// ended between the decision and this call is normal, and counting it
    /// would report a pin nobody could find the client for. `shed` has said
    /// exactly that since it was written; this said the opposite until
    /// `M17.4`, so `pgprox_pin_total` could climb while `SHOW CLIENTS` showed
    /// nothing pinned.
    pub fn set_pinned(&self, conn: ConnId, reason: &str) {
        if let Some(entry) = self.lock().get_mut(&conn) {
            entry.pinned = Some(reason.to_owned());
            self.pins.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// What each client's tenant is allowed, for a shed decision.
    ///
    /// The largest a session claimed, because two sessions of one tenant carry
    /// the same grant and the largest is the least likely to be a stale one.
    #[must_use]
    pub fn budget_for(&self, tenant: &TenantId) -> u32 {
        self.lock()
            .values()
            .filter(|entry| &entry.tenant == tenant)
            .map(|entry| entry.budget)
            .max()
            .unwrap_or(0)
    }

    /// Asks one client to leave, and counts it as shed.
    ///
    /// Returns whether there was such a client. The session closes itself at
    /// its next boundary: this is a request, not a socket being cut, which is
    /// what makes a shed invisible to an application that reconnects.
    pub fn shed(&self, conn: ConnId, now: Instant) -> bool {
        let Some(entry) = self.lock().get(&conn).cloned() else {
            return false;
        };
        entry.close.fire();
        self.sheds.fetch_add(1, Ordering::Relaxed);
        self.recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(entry.tenant)
            .or_default()
            .push_back(now);
        true
    }

    /// Closes one client for having been idle past the configured timeout.
    ///
    /// Fires `idle_signal` rather than `close`, so the session reports
    /// `ClientError::IdleTimeout` rather than `ClientError::Shed`: the two are
    /// different facts about why the connection ended, and an operator
    /// reading `pgprox_shed_total` next to a client log full of idle closures
    /// would draw the wrong conclusion from either being folded into the
    /// other.
    pub fn close_idle(&self, conn: ConnId) -> bool {
        let Some(entry) = self.lock().get(&conn).cloned() else {
            return false;
        };
        // Set before firing, so a session waking on `close` never reads it
        // before it is there. `Release`/`Acquire` is what makes that ordering
        // guarantee cross threads rather than merely happening to work here.
        entry.idle.store(true, Ordering::Release);
        entry.close.fire();
        self.idle_timeouts.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Whether `conn`'s `close` signal, if it has fired, was for having been
    /// idle rather than for tenant affinity.
    ///
    /// A lookup at the moment `close` wakes a session, rather than a
    /// reference the session holds across every await in its loop: `conn` and
    /// the registry are already part of the relay loop's captured state, so
    /// this adds nothing to it. See `M74.0`.
    #[must_use]
    pub fn was_idle_timeout(&self, conn: ConnId) -> bool {
        self.lock()
            .get(&conn)
            .is_some_and(|entry| entry.idle.load(Ordering::Acquire))
    }

    /// Clients closed for having been idle past the configured timeout, since
    /// start.
    #[must_use]
    pub fn idle_timeouts(&self) -> u64 {
        self.idle_timeouts.load(Ordering::Relaxed)
    }

    /// How many times this tenant has been shed in the last minute.
    ///
    /// What feeds the rate limit M3.7 named. Without it the limit is handed a
    /// zero and can never refuse, which turns "move this client once" into
    /// "move this client every time the tick runs".
    #[must_use]
    pub fn recent_sheds(&self, tenant: &TenantId, now: Instant) -> u32 {
        let mut recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(times) = recent.get_mut(tenant) else {
            return 0;
        };

        // Trimmed on read rather than on a timer: the map is touched only when
        // a shed happens or a decision is taken, and a timer would be a second
        // thing to get wrong.
        while times
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > SHED_WINDOW)
        {
            times.pop_front();
        }
        u32::try_from(times.len()).unwrap_or(u32::MAX)
    }

    /// Counts one completed transaction.
    pub fn count_transaction(&self) {
        self.transactions.fetch_add(1, Ordering::Relaxed);
    }

    /// How many clients this node is serving.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.lock().len()).unwrap_or(u32::MAX)
    }

    /// Whether it is serving none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Transactions served since start.
    #[must_use]
    pub fn transactions(&self) -> u64 {
        self.transactions.load(Ordering::Relaxed)
    }

    /// Sessions pinned since start.
    #[must_use]
    pub fn pins(&self) -> u64 {
        self.pins.load(Ordering::Relaxed)
    }

    /// Clients shed since start.
    #[must_use]
    pub fn sheds(&self) -> u64 {
        self.sheds.load(Ordering::Relaxed)
    }

    /// Client counts per tenant.
    #[must_use]
    pub fn per_tenant(&self) -> Vec<(TenantId, u32)> {
        let mut counts: std::collections::BTreeMap<TenantId, u32> =
            std::collections::BTreeMap::new();
        for entry in self.lock().values() {
            *counts.entry(entry.tenant.clone()).or_default() += 1;
        }
        counts.into_iter().collect()
    }

    /// Upstream connections held per tenant, not client connections.
    ///
    /// `ClientState::Active` is documented as "holding an upstream
    /// connection" and nothing else is, so counting only that state gives
    /// exactly one per connection actually held rather than one per client
    /// waiting on or multiplexed behind it. `M90`, cycle 6: `report_tenants`
    /// was fed [`Self::per_tenant`] instead, which in a proxy built to
    /// multiplex many clients onto few connections reported a number
    /// dominated by idle clients rather than by the upstream budget it was
    /// compared against — `ClusterDigest::tenant_usage`'s own doc says
    /// "upstream connections it holds per tenant".
    ///
    /// Callers still need to restrict this to tenants the node homes before
    /// gossiping it, which is `tenant_usage`'s other documented restriction;
    /// this crate does not know about cluster membership, so that filter
    /// stays at the call site, the same as `per_tenant`'s callers already
    /// filter for their own purposes.
    #[must_use]
    pub fn per_tenant_upstream(&self) -> Vec<(TenantId, u32)> {
        let mut counts: std::collections::BTreeMap<TenantId, u32> =
            std::collections::BTreeMap::new();
        for entry in self.lock().values() {
            if entry.state == ClientState::Active {
                *counts.entry(entry.tenant.clone()).or_default() += 1;
            }
        }
        counts.into_iter().collect()
    }

    /// Every client, as `SHOW CLIENTS` and `GET /v1/clients` render them.
    #[must_use]
    pub fn views(&self, now: Instant) -> Vec<ClientView> {
        let mut views: Vec<ClientView> = self
            .lock()
            .iter()
            .map(|(conn, entry)| ClientView {
                conn: *conn,
                tenant: entry.tenant.clone(),
                node: entry.node,
                state: entry.state,
                since: now.saturating_duration_since(entry.since),
                pinned: entry.pinned.clone(),
            })
            .collect();
        // Sorted so two reads of an unchanged node agree, which is what makes
        // diffing two of them worth anything.
        views.sort_by_key(|view| view.conn);
        views
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ConnId, Entry>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn conn(n: u64) -> ConnId {
        ConnId::new(NodeId::new(1), n)
    }

    #[test]
    fn a_registered_client_is_reported() {
        let sessions = Sessions::new();
        let now = Instant::now();
        let _held = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        let views = sessions.views(now);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].tenant, TenantId::new("acme"));
        assert_eq!(views[0].state, ClientState::Idle);
    }

    #[test]
    fn a_client_disappears_when_its_registration_drops() {
        // Drop rather than an explicit call, because the explicit call is the
        // one skipped on the path where a session ended badly, and that is
        // when a stale row is most misleading.
        let sessions = Sessions::new();
        let now = Instant::now();

        let held = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        assert_eq!(sessions.len(), 1);

        // A second, so that the count is a count rather than a constant.
        // `M17.4`: `len` returning 1 survived every test, because one was the
        // only number anything asked for, and a node reporting one client
        // while serving thousands is the number the shed decision divides.
        let also = sessions.register(
            conn(2),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        assert_eq!(sessions.len(), 2);
        drop(also);
        assert_eq!(sessions.len(), 1);

        drop(held);
        assert_eq!(sessions.len(), 0);
        assert!(sessions.is_empty(), "a finished session was still listed");
    }

    #[test]
    fn the_state_timestamp_moves_only_when_the_state_does() {
        // SHOW CLIENTS reports how long a client has been in its current
        // state, which is the column an operator reads to find a stuck
        // session. Resetting it on every touch would hide exactly that.
        let sessions = Sessions::new();
        let start = Instant::now();
        let _held = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            start,
            16,
            crate::run::Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        let later = start + Duration::from_secs(10);
        sessions.set_state(conn(1), ClientState::Active, later);
        sessions.set_state(conn(1), ClientState::Active, later + Duration::from_secs(5));

        let views = sessions.views(later + Duration::from_secs(5));
        assert_eq!(views[0].since, Duration::from_secs(5));
    }

    #[test]
    fn a_pinned_client_says_why() {
        let sessions = Sessions::new();
        let now = Instant::now();
        let _held = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            crate::run::Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        sessions.set_pinned(conn(1), "listen");

        assert_eq!(sessions.views(now)[0].pinned.as_deref(), Some("listen"));
        assert_eq!(sessions.pins(), 1);
    }

    #[test]
    fn updates_for_a_client_that_has_gone_are_ignored() {
        // A session that ended between a decision and its report is normal,
        // and resurrecting its row would list a client nobody could find.
        let sessions = Sessions::new();
        let now = Instant::now();

        sessions.set_state(conn(9), ClientState::Active, now);
        sessions.set_pinned(conn(9), "listen");

        assert!(sessions.is_empty());
        // And the counter says so too. `M17.4`: `pins` returning a constant 1
        // survived because no test ever saw it hold zero, and under it lived
        // the defect that the increment ran whether or not there was a client
        // to pin. `sheds` is asserted to stay at zero three lines up in its
        // own test; this asks the same question of the same shape.
        assert_eq!(sessions.pins(), 0, "a client that is gone was counted");
    }

    #[test]
    fn clients_are_counted_per_tenant() {
        let sessions = Sessions::new();
        let now = Instant::now();
        let signal = crate::run::Shutdown::new;
        let _a = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let _b = sessions.register(
            conn(2),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let _c = sessions.register(
            conn(3),
            TenantId::new("globex"),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        assert_eq!(
            sessions.per_tenant(),
            vec![(TenantId::new("acme"), 2), (TenantId::new("globex"), 1)]
        );
    }

    #[test]
    fn only_connection_holding_clients_count_toward_upstream_usage() {
        // `M90`, cycle 6. `ClusterDigest::tenant_usage` is documented as
        // upstream connections, and `ClientState::Active` is the one state
        // documented as holding one. Idle and waiting clients are the
        // multiplexing this proxy exists for and must not inflate a number a
        // peer compares against an upstream budget.
        let sessions = Sessions::new();
        let now = Instant::now();
        let signal = crate::run::Shutdown::new;
        let acme = TenantId::new("acme");
        let globex = TenantId::new("globex");

        let _active = sessions.register(
            conn(1),
            acme.clone(),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        sessions.set_state(conn(1), ClientState::Active, now);

        let _idle = sessions.register(
            conn(2),
            acme.clone(),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        sessions.set_state(conn(2), ClientState::Idle, now);

        let _waiting = sessions.register(
            conn(3),
            globex.clone(),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        sessions.set_state(conn(3), ClientState::Waiting, now);

        // Every client counts here, which is `per_tenant`'s own contract and
        // is unaffected by this fix.
        assert_eq!(sessions.per_tenant(), vec![(acme.clone(), 2), (globex, 1)]);

        // Only the one actually holding a connection counts here: globex's
        // waiting client has none yet, and acme's idle one gave hers back.
        assert_eq!(sessions.per_tenant_upstream(), vec![(acme, 1)]);
    }

    #[test]
    fn two_reads_of_an_unchanged_node_agree() {
        // Which is what makes diffing two of them worth anything.
        let sessions = Sessions::new();
        let now = Instant::now();
        let signal = crate::run::Shutdown::new;
        let _a = sessions.register(
            conn(3),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let _b = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            signal(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        assert_eq!(sessions.views(now), sessions.views(now));
        assert_eq!(sessions.views(now)[0].conn, conn(1));
    }

    #[test]
    fn shedding_a_client_asks_it_to_leave_and_counts_it() {
        // A request rather than a socket being cut: the session closes itself
        // at its next boundary, which is what makes a shed invisible to an
        // application that reconnects.
        let sessions = Sessions::new();
        let close = crate::run::Shutdown::new();
        let _held = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            Instant::now(),
            16,
            close.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        assert!(sessions.shed(conn(1), Instant::now()));
        assert!(close.fired(), "the session was never asked to leave");
        assert_eq!(sessions.sheds(), 1);
    }

    #[test]
    fn shedding_a_client_that_has_gone_does_nothing() {
        // A session that ended between the decision and the request is normal,
        // and counting it would report a shed that never happened.
        let sessions = Sessions::new();

        assert!(!sessions.shed(conn(9), Instant::now()));
        assert_eq!(sessions.sheds(), 0);
    }

    #[test]
    fn closing_an_idle_client_fires_the_same_signal_shed_does() {
        // One signal for both reasons, which is `M74.0`'s whole point: the
        // session watches one `select!` branch rather than two.
        let sessions = Sessions::new();
        let close = crate::run::Shutdown::new();
        let _held = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            Instant::now(),
            16,
            close.clone(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        assert!(sessions.close_idle(conn(1)));
        assert!(close.fired(), "the session was never asked to leave");
        assert_eq!(sessions.idle_timeouts(), 1);
        assert_eq!(sessions.sheds(), 0, "an idle close was counted as a shed");
    }

    #[test]
    fn was_idle_timeout_tells_the_two_reasons_apart() {
        let sessions = Sessions::new();
        let shed_close = crate::run::Shutdown::new();
        let idle_close = crate::run::Shutdown::new();
        let _shed_session = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            Instant::now(),
            16,
            shed_close,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let _idle_session = sessions.register(
            conn(2),
            TenantId::new("acme"),
            NodeId::new(1),
            Instant::now(),
            16,
            idle_close,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        sessions.shed(conn(1), Instant::now());
        sessions.close_idle(conn(2));

        assert!(
            !sessions.was_idle_timeout(conn(1)),
            "a shed read back as an idle timeout"
        );
        assert!(
            sessions.was_idle_timeout(conn(2)),
            "an idle timeout read back as a shed"
        );
    }

    #[test]
    fn was_idle_timeout_of_a_client_that_has_gone_is_false_rather_than_a_panic() {
        let sessions = Sessions::new();
        assert!(!sessions.was_idle_timeout(conn(9)));
    }

    #[test]
    fn closing_an_idle_client_that_has_gone_does_nothing() {
        let sessions = Sessions::new();
        assert!(!sessions.close_idle(conn(9)));
        assert_eq!(sessions.idle_timeouts(), 0);
    }

    #[test]
    fn recent_sheds_are_counted_per_tenant_and_expire() {
        // The rate limit's only input. A tenant shed once must not be shed
        // again immediately, and a tenant shed a minute ago is not recent.
        let sessions = Sessions::new();
        let start = Instant::now();
        let acme = TenantId::new("acme");
        let _held = sessions.register(
            conn(1),
            acme.clone(),
            NodeId::new(1),
            start,
            16,
            crate::run::Shutdown::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        assert_eq!(sessions.recent_sheds(&acme, start), 0);
        sessions.shed(conn(1), start);
        assert_eq!(sessions.recent_sheds(&acme, start), 1);

        // Another tenant's sheds are not this one's.
        assert_eq!(sessions.recent_sheds(&TenantId::new("globex"), start), 0);

        // The window's own edge. `M17.4`: `>` and `>=` were interchangeable
        // here, because the only two times asked about were the instant of
        // the shed and a full second past the window. A shed exactly
        // `SHED_WINDOW` old is still inside a window that long.
        assert_eq!(sessions.recent_sheds(&acme, start + SHED_WINDOW), 1);

        // And the window moves.
        assert_eq!(
            sessions.recent_sheds(&acme, start + SHED_WINDOW + Duration::from_secs(1)),
            0
        );
    }

    #[test]
    fn the_counters_count() {
        let sessions = Sessions::new();
        sessions.count_transaction();
        sessions.count_transaction();

        assert_eq!(sessions.transactions(), 2);
        assert_eq!(sessions.sheds(), 0, "nothing was shed");
    }
}
