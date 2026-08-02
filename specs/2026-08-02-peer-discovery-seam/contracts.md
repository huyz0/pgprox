# Contracts

Everything here is new. Nothing existing in `pgprox-core` changes shape, but the
trait lands in `pgprox-core` and therefore falls under non-negotiable 6: the
trait, its fake, every implementation and an ADR arrive in one commit.
`scripts/check-core-contract.sh` enforces the two mechanical halves.

## The trait

`crates/pgprox-core/src/cluster.rs`, beside `ClusterCoordinator`.

Deliberately the same shape as `ConfigSource`, which has been proven here for
two milestones and already has a fake, a poll-loop implementation and a
composition root that starts it without knowing which it holds. A second
mechanism for "a thing that changes while a node runs" would be a second set of
mistakes.

```rust
/// Where a node learns which peers to gossip with.
///
/// Discovery, and deliberately not liveness. A source may cause this node to
/// gossip with more peers, or to treat one as draining sooner than gossip
/// would. It may never cause a node to be counted alive that gossip has not
/// heard from: `pgprox_cluster::membership` counts a peer alive from digests
/// that arrived, and that is what makes a one-way network failure safe.
///
/// See ADR 0004 and ADR 00XX.
#[async_trait::async_trait]
pub trait PeerSource: Send + Sync + fmt::Debug {
    /// The peers this node should gossip with, by node id, as `host:port`.
    ///
    /// Never includes this node. A node gossiping with itself is a peer that
    /// can never be down, which would make quorum unfalsifiable.
    fn peers(&self) -> Arc<BTreeMap<NodeId, String>>;

    /// Observes changes. The receiver always holds the latest table.
    fn watch(&self) -> watch::Receiver<Arc<BTreeMap<NodeId, String>>>;

    /// Whether the last attempt to read the peer table succeeded.
    ///
    /// Defaulted to true, because a source with no loop cannot fail between
    /// reads. A source that can go stale overrides it, for the reason
    /// `ConfigSource::is_healthy` exists: a node gossiping with a table from
    /// twenty minutes ago looks exactly like one gossiping with the current
    /// table.
    fn is_healthy(&self) -> bool {
        true
    }

    /// Runs whatever loop this source needs to notice a change, until dropped.
    ///
    /// Defaulted to never returning, because the static source has no loop.
    async fn run_loop(self: Arc<Self>) {
        std::future::pending::<()>().await
    }
}
```

The `Arc` forwarding impl is required and must forward `is_healthy` rather than
take the default, for the reason the `ConfigSource` one carries a comment about:
an `Arc` around a source that can go stale can go stale, and taking the default
reports every wrapped source as healthy forever. `M14.34` found that exact
mutant surviving.

## The static implementation

`crates/pgprox-core/src/cluster.rs`, since it is three fields and no I/O.

```rust
/// A fixed peer table, which is what `--peer` flags produce.
///
/// The default, and the behaviour the fleet has today.
#[derive(Debug)]
pub struct StaticPeers { /* watch::Sender<Arc<BTreeMap<NodeId, String>>> */ }

impl StaticPeers {
    #[must_use]
    pub fn new(peers: BTreeMap<NodeId, String>) -> Arc<Self>;
}
```

## The fake

Behind `test-fakes`, beside `FakeConfigSource`, and it must be able to publish:
the whole point of the seam is a table that changes, and a fake that cannot
change is a fake that tests only the static case.

```rust
#[derive(Debug)]
pub struct FakePeerSource { /* … */ }

impl FakePeerSource {
    pub fn new(initial: BTreeMap<NodeId, String>) -> Arc<Self>;
    /// Publishes a new table to every watcher.
    pub fn publish(&self, next: BTreeMap<NodeId, String>);
    /// Makes `is_healthy` report false, so the stale path has a driver.
    pub fn go_stale(&self);
}
```

## What changes in the binary

`bin/pgprox/src/run.rs`, `run_with_peers`:

```rust
// before
pub async fn run_with_peers(
    app: App,
    listeners: Listeners,
    peers: BTreeMap<NodeId, String>,
    shutdown: Shutdown,
) -> Result<(), StartupError>

// after
pub async fn run_with_peers(
    app: App,
    listeners: Listeners,
    peers: Arc<dyn PeerSource>,
    shutdown: Shutdown,
) -> Result<(), StartupError>
```

Three consumers stop taking a copy:

| consumer | today | after |
| --- | --- | --- |
| `GossipTransport::new` | `BTreeMap` moved in | holds the source, reads per request |
| `NodeObservatory::set_peers` | `OnceLock`, set once | holds the source; the `OnceLock` goes |
| `Context.peers` | `BTreeMap` field | holds the source, reads per cancel |

The `OnceLock` removal is the one behavioural change worth naming. Its doc says
"a second call would mean two answers to who is in the fleet", which was the
right guard when the answer could not change. It becomes wrong the moment the
answer can.

`entry.rs` builds a `StaticPeers` from the parsed `--peer` flags, so the flag
surface and the chart are untouched by this work.

## What does not change

- `ClusterCoordinator`, `MembershipView`, `ClusterDigest`: untouched.
- `MembershipConfig` and its three-and-ten-second windows: untouched.
- `CoordinatorConfig::fleet_size`: still configured, still the quota divisor.
- The gossip wire format: untouched. This is about who to talk to, not what is
  said.
