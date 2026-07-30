//! Cancelling a query, on whichever node the request lands on.
//!
//! A `CancelRequest` arrives on a fresh connection carrying nothing but a key.
//! It is unauthenticated by design, and the load balancer will not send it to
//! the node holding the query. Both facts shape everything here.
//!
//! # The key is a bearer token
//!
//! Anyone holding it can cancel that query. So the secret half of a [`ConnId`]
//! is filled from a CSPRNG rather than from a counter: with a counter, adding
//! one to your own key gives you your neighbour's, and "cancel your own query"
//! quietly becomes "cancel anyone's". `pgprox-core` cannot enforce that, having
//! no entropy source, so the obligation lands here and
//! [`Registry::issue`] is where it is met.
//!
//! # A cancel is only valid while the connection is held
//!
//! The proxy multiplexes, so the upstream connection a client was using at
//! `t` may belong to somebody else at `t+1`. Sending the server a
//! `CancelRequest` for a connection that has gone back to the pool cancels
//! whatever the next session is running, which is worse than not cancelling at
//! all: the client that asked sees its query finish normally, and an unrelated
//! tenant sees theirs fail.
//!
//! So the mapping exists only between acquire and release. Outside that window
//! a cancel resolves to nothing and is refused.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use pgprox_core::ids::{ConnId, NodeId, PoolKey, ServerId};
use pgprox_proto::encode_frontend;

/// Where a cancel request has to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routing {
    /// This node owns it: send a `CancelRequest` upstream with these details.
    Local(Box<Cancellation>),
    /// Another node owns it. Forward, and let it decide.
    Peer(NodeId),
    /// This node owns the key and has no such query running.
    ///
    /// Refused rather than ignored: a cancel for a query that already finished
    /// is normal and cheap to answer, and treating an unknown key as "probably
    /// fine" is how a key that leaked stays useful.
    Unknown,
}

/// What is needed to cancel one query upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancellation {
    /// The server holding the query.
    pub server: ServerId,
    /// The pool the connection came from, for dialling it again.
    pub key: PoolKey,
    /// The server's own cancel key, from its `BackendKeyData`.
    ///
    /// Not the one the proxy handed the client. Postgres will only cancel a
    /// query for the key it issued itself.
    pub backend_key: (i32, i32),
}

/// A source of unpredictable bits.
///
/// A trait so tests can be deterministic, and so this crate does not have to
/// choose a random number generator on behalf of the composition root.
pub trait Entropy: Send + Sync + fmt::Debug {
    /// The next value, or `None` when there is no entropy to be had.
    ///
    /// Only the low 48 bits are used. `None` rather than a fallback value,
    /// because every fallback available is either predictable or a panic on a
    /// connection path: a cancel key is a bearer token, and one drawn from a
    /// counter lets a tenant cancel its neighbour's queries by trying numbers
    /// near its own. A source that cannot produce bits refuses the connection
    /// instead. See `M1F.36` and `M6.30`.
    fn next(&self) -> Option<u64>;
}

/// Every query this node could be asked to cancel.
pub struct Registry {
    node: NodeId,
    entropy: Box<dyn Entropy>,
    live: Mutex<HashMap<ConnId, Cancellation>>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // No entries: each one is a cancel key, and this type will end up in a
        // log line eventually.
        f.debug_struct("Registry")
            .field("node", &self.node)
            .field("live", &self.len())
            .finish_non_exhaustive()
    }
}

impl Registry {
    /// A registry for connections this node owns.
    #[must_use]
    pub fn new(node: NodeId, entropy: Box<dyn Entropy>) -> Self {
        Self {
            node,
            entropy,
            live: Mutex::new(HashMap::new()),
        }
    }

    /// A fresh connection identifier, with a random secret.
    ///
    /// The randomness is the point. See the module docs.
    ///
    /// Returns `None` when the entropy source has none, which the caller turns
    /// into a refused connection. A key issued anyway would be guessable, and
    /// the client would have no way to know that.
    pub fn issue(&self) -> Option<ConnId> {
        Some(ConnId::new(self.node, self.entropy.next()?))
    }

    /// How many queries are currently cancellable.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether none are.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Records that a client's session now holds an upstream connection.
    ///
    /// Called on acquire. Until this, and after [`Registry::release`], a cancel
    /// for this client resolves to nothing.
    pub fn hold(&self, conn: ConnId, cancellation: Cancellation) {
        self.lock().insert(conn, cancellation);
    }

    /// Records that the connection went back to the pool.
    ///
    /// Called on release, and on disconnect. Forgetting to call it is the bug
    /// that cancels a stranger's query.
    pub fn release(&self, conn: ConnId) {
        self.lock().remove(&conn);
    }

    /// Where a cancel request for this key has to go.
    #[must_use]
    pub fn route(&self, conn: ConnId) -> Routing {
        if conn.node() != self.node {
            return Routing::Peer(conn.node());
        }
        self.lock()
            .get(&conn)
            .cloned()
            .map_or(Routing::Unknown, |found| Routing::Local(Box::new(found)))
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ConnId, Cancellation>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Sends a `CancelRequest` on an already-dialled connection.
///
/// The caller opens a fresh socket to the server holding the query. Postgres
/// requires the request on its own connection: it carries no startup packet
/// and gets no answer, and the server closes the socket whether or not it
/// cancelled anything.
///
/// # Errors
///
/// Fails when the socket does. A cancel that could not be sent is worth
/// logging and nothing more: the query will finish on its own.
pub async fn send<S>(mut stream: S, backend_key: (i32, i32)) -> std::io::Result<()>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    let mut out = Vec::new();
    encode_frontend::cancel_request(&mut out, backend_key.0, backend_key.1);
    stream.write_all(&out).await?;
    stream.flush().await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Entropy that is not: a counter, so tests are deterministic.
    ///
    /// Deliberately the thing production must never use, which is why the
    /// unpredictability test below supplies a different source rather than
    /// this one.
    #[derive(Debug, Default)]
    struct Counter(AtomicU64);

    impl Entropy for Counter {
        fn next(&self) -> Option<u64> {
            Some(self.0.fetch_add(1, Ordering::SeqCst) + 1)
        }
    }

    /// A source that has nothing, which is what a broken machine looks like.
    #[derive(Debug, Default)]
    struct Dry;

    impl Entropy for Dry {
        fn next(&self) -> Option<u64> {
            None
        }
    }

    /// A source with real spread, for the property that matters.
    #[derive(Debug, Default)]
    struct SplitMix(AtomicU64);

    impl Entropy for SplitMix {
        fn next(&self) -> Option<u64> {
            // Not for production either, but it has the property under test:
            // consecutive outputs are not consecutive numbers.
            let mut z = self
                .0
                .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::SeqCst)
                .wrapping_add(0x9E37_79B9_7F4A_7C15);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            Some(z ^ (z >> 31))
        }
    }

    fn registry() -> Registry {
        Registry::new(NodeId::new(1), Box::new(Counter::default()))
    }

    fn cancellation() -> Cancellation {
        Cancellation {
            server: ServerId::new("db-1", 5432),
            key: PoolKey::new(ServerId::new("db-1", 5432), "acme", "acme_app"),
            backend_key: (4242, 0x0bad_beef),
        }
    }

    #[test]
    fn a_source_with_no_entropy_issues_nothing() {
        // Rather than a fallback. Every fallback available is guessable, and a
        // client handed a guessable cancel key has no way to know it.
        let registry = Registry::new(NodeId::new(1), Box::new(Dry));

        assert!(registry.issue().is_none());
    }

    #[test]
    fn the_registry_counts_what_it_holds_and_prints_none_of_it() {
        // Two things nothing asserted. The count is what an operator reads to
        // know whether a node still has cancellable queries, so it has to
        // move; and the Debug has to say the count without saying the keys,
        // each of which is a bearer token for cancelling somebody's query.
        // Printing nothing satisfies the second half and fails the first.
        let registry = registry();
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());

        let conn = registry.issue().unwrap();
        registry.hold(conn, cancellation());
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());

        let shown = format!("{registry:?}");
        assert!(shown.contains("live: 1"), "{shown}");
        assert!(!shown.contains("4242"), "{shown}");
        assert!(!shown.contains(&0x0bad_beef_i32.to_string()), "{shown}");

        registry.release(conn);
        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn a_key_another_node_issued_is_forwarded_to_it() {
        // The whole reason the node is encoded in the key. Without this,
        // cancellation silently breaks the moment there is a second pod.
        let registry = registry();
        let elsewhere = ConnId::new(NodeId::new(7), 99);

        assert_eq!(registry.route(elsewhere), Routing::Peer(NodeId::new(7)));
    }

    #[test]
    fn a_key_this_node_owns_and_does_not_hold_is_refused() {
        // A cancel for a query that already finished is normal. Treating an
        // unknown key as probably fine is how a leaked key stays useful.
        let registry = registry();
        let conn = registry.issue().unwrap();

        assert_eq!(registry.route(conn), Routing::Unknown);
    }

    #[test]
    fn a_released_connection_can_no_longer_be_cancelled() {
        // The property this module exists for. The proxy multiplexes, so the
        // connection a client used a moment ago may belong to somebody else
        // now, and cancelling it would fail an unrelated tenant's query while
        // the client that asked watches its own finish normally.
        let registry = registry();
        let conn = registry.issue().unwrap();

        registry.hold(conn, cancellation());
        registry.release(conn);

        assert_eq!(
            registry.route(conn),
            Routing::Unknown,
            "a cancel outlived the connection it named"
        );
    }

    #[test]
    fn holding_a_second_connection_replaces_the_first() {
        // A session that released and acquired again is on a different
        // upstream connection, with a different server-side key.
        let registry = registry();
        let conn = registry.issue().unwrap();
        registry.hold(conn, cancellation());

        let moved = Cancellation {
            backend_key: (9999, 1),
            ..cancellation()
        };
        registry.hold(conn, moved.clone());

        assert_eq!(registry.route(conn), Routing::Local(Box::new(moved)));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn every_issued_key_belongs_to_this_node() {
        let registry = Registry::new(NodeId::new(5), Box::new(SplitMix::default()));
        for _ in 0..100 {
            assert_eq!(registry.issue().unwrap().node(), NodeId::new(5));
        }
    }

    #[test]
    fn issued_keys_are_not_derivable_from_one_another() {
        // The security property. With a counter, adding one to your own key
        // gives you your neighbour's, and "cancel your own query" becomes
        // "cancel anyone's".
        let registry = Registry::new(NodeId::new(1), Box::new(SplitMix::default()));
        let issued: Vec<u64> = (0..64)
            .map(|_| registry.issue().unwrap().secret())
            .collect();

        let sequential = issued.windows(2).filter(|w| w[1] == w[0] + 1).count();
        assert_eq!(
            sequential, 0,
            "issued keys are consecutive, so one client can derive another's"
        );

        let unique: std::collections::BTreeSet<u64> = issued.iter().copied().collect();
        assert_eq!(unique.len(), issued.len(), "two clients got the same key");
    }

    #[test]
    fn a_counter_source_fails_the_property_the_random_one_passes() {
        // Proves the test above is measuring something. A counter is exactly
        // what the module docs forbid, and it must not pass.
        let registry = registry();
        let issued: Vec<u64> = (0..8).map(|_| registry.issue().unwrap().secret()).collect();

        assert!(
            issued.windows(2).all(|w| w[1] == w[0] + 1),
            "the counter source stopped counting, so the property test proves nothing"
        );
    }

    #[test]
    fn a_registry_prints_no_cancel_keys() {
        // Each entry is a bearer token, and this will reach a log line.
        let registry = registry();
        let conn = registry.issue().unwrap();
        registry.hold(conn, cancellation());

        let rendered = format!("{registry:?}");
        assert!(!rendered.contains("beef"), "{rendered}");
        assert!(!rendered.contains("4242"), "{rendered}");
    }

    #[tokio::test]
    async fn a_cancel_request_carries_the_servers_own_key() {
        // Postgres cancels only for the key it issued itself, so sending the
        // one the proxy handed its client cancels nothing at all, silently.
        let (mut ours, mut theirs) = tokio::io::duplex(64);
        send(&mut ours, (4242, 0x0bad_beef)).await.unwrap();

        let mut buf = [0_u8; 16];
        tokio::io::AsyncReadExt::read_exact(&mut theirs, &mut buf)
            .await
            .unwrap();

        let pgprox_proto::frame::Decoded::Frame(frame, _) =
            pgprox_proto::frame::decode_untagged(&buf, 1024).unwrap()
        else {
            panic!("the cancel request did not decode");
        };
        let decoded = pgprox_proto::startup::decode(frame.body()).unwrap();
        let pgprox_proto::startup::Startup::CancelRequest { conn } = decoded else {
            panic!("that was not a cancel request: {decoded:?}");
        };
        assert_eq!(
            pgprox_proto::backend::key_from_conn_id(conn),
            (4242, 0x0bad_beef)
        );
    }

    #[tokio::test]
    async fn a_cancel_to_a_closed_socket_is_an_error_rather_than_a_panic() {
        let (ours, theirs) = tokio::io::duplex(64);
        drop(theirs);

        assert!(send(ours, (1, 2)).await.is_err());
    }
}
