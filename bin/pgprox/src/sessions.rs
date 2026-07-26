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
    /// Fired to ask this one session to leave.
    ///
    /// The registry holds the signal rather than the session, because a shed
    /// decision is taken by the node and a session is a task on a socket. The
    /// session watches it at the same place it watches a drain, so a client
    /// mid-transaction is not cut off by either.
    close: crate::run::Shutdown,
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
    /// When each tenant was last shed, most recent last.
    ///
    /// The window the per-tenant rate limit is measured over. Kept here
    /// because this is where a shed happens, and a limit counted anywhere else
    /// would be counting something other than what it limits.
    recent: Mutex<HashMap<TenantId, VecDeque<Instant>>>,
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

    /// Registers a client, until the returned value is dropped.
    pub fn register(
        &self,
        conn: ConnId,
        tenant: TenantId,
        node: NodeId,
        now: Instant,
        budget: u32,
        close: crate::run::Shutdown,
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
    pub fn set_pinned(&self, conn: ConnId, reason: &str) {
        if let Some(entry) = self.lock().get_mut(&conn) {
            entry.pinned = Some(reason.to_owned());
        }
        self.pins.fetch_add(1, Ordering::Relaxed);
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
        );
        assert_eq!(sessions.len(), 1);
        drop(held);

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
        );
        let _b = sessions.register(
            conn(2),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            signal(),
        );
        let _c = sessions.register(
            conn(3),
            TenantId::new("globex"),
            NodeId::new(1),
            now,
            16,
            signal(),
        );

        assert_eq!(
            sessions.per_tenant(),
            vec![(TenantId::new("acme"), 2), (TenantId::new("globex"), 1)]
        );
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
        );
        let _b = sessions.register(
            conn(1),
            TenantId::new("acme"),
            NodeId::new(1),
            now,
            16,
            signal(),
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
        );

        assert_eq!(sessions.recent_sheds(&acme, start), 0);
        sessions.shed(conn(1), start);
        assert_eq!(sessions.recent_sheds(&acme, start), 1);

        // Another tenant's sheds are not this one's.
        assert_eq!(sessions.recent_sheds(&TenantId::new("globex"), start), 0);

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
