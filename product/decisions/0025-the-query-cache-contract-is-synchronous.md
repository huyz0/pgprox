# 0025. The query cache contract is synchronous

Status: accepted

Amends ADR [0021](0021-the-query-cache-promises-bounded-staleness.md), which
this rests on rather than changes: the promise is still bounded staleness and
the cache is still one node's own.

## Context

`QueryCache` has been an `#[async_trait]` since M0, when it was a contract with
nothing behind it. `pgprox-cache` implemented it in M9, and that implementation
holds a `std::sync::Mutex` and a `HashMap`. Its module documentation says
"Nothing here waits", and it is right: no method on the store awaits anything,
and the one lock it takes is a `std` mutex precisely because nothing suspends
while holding it.

So the trait described a capability no implementation had, and `async_trait`
charged for it. The macro rewrites every method to return a boxed future, which
is a heap allocation per call. `pgprox-core` also implements the trait for
`Arc<T>`, so a caller holding an `Arc<dyn QueryCache>` — which is what the
composition root holds — boxed twice: once for the forwarding call, once for the
real one.

`M26.2` added an allocation budget to `pgprox-cache`, the first the crate had
had. It found this, and reading had not:

| path | heap blocks, before |
| --- | --- |
| a miss, which hashes a key and returns `None` | 2 |
| the same call reaching the store directly | 1 |
| a hit | 2, plus the recency order's own |

Two allocations per statement, on a feature whose entire argument is that a hit
is cheaper than a round trip to the database.

## Decision

`QueryCache::get`, `put` and `invalidate_tenant` are synchronous. The trait
carries no `#[async_trait]`, the `Arc<T>` forwarding impl becomes ordinary
method calls the compiler can inline, and a lookup allocates nothing.

Nothing about the behaviour changes. An `async fn` that never yields and a `fn`
do the same work in the same order; what goes is the box the future was placed
in. The callers were already inside async functions and remain so, and they were
already blocking on a `std::sync::Mutex` for the duration of the call, because
that is what the store has always done.

## What this forecloses, and why that is acceptable

An implementation that reaches the network. A memcached or Redis backend, which
is what pgpool-II offers through `memqcache_method`, could not implement this
contract without blocking a runtime worker.

ADR 0021 already decided that question in the other direction. "One node, not
the fleet. Each node caches for itself and invalidates for itself." The
reasoning there was about staleness under partition and it lands here too: a
shared cache is a network dependency on the statement path, and the TTL bounds
staleness whether the entries are shared or not.

If a remote backend is ever wanted, this is not the contract it should
implement. It would need timeouts, a failure mode for an unreachable store, and
a decision about whether a statement waits for it — three things this trait has
no vocabulary for and should not acquire on the chance. Adding `async` back for
an implementation that does not exist is speculative generality, which is what
this ADR is undoing.

## Alternatives rejected

**Keep the trait async and fix the call sites.** Calling `(**cache).get(..)`
rather than `cache.get(..)` skips the forwarding impl and halves the cost, two
blocks to one. It leaves the remaining block, it leaves the footgun for the next
caller who writes the natural thing, and it makes the fast path depend on
punctuation.

**Remove the `Arc<T>` impl instead.** Also halves it, and takes away something
callers legitimately use. The impl is not the problem; boxing is, and the impl
only doubled it.

**Keep `async` and use a runtime-aware lock.** Backwards. A `tokio::Mutex`
would make the store genuinely async and slower, to justify a signature chosen
before the implementation existed.

## Consequences

A statement's cache lookup allocates nothing, where it allocated twice. The
instruction counts in `product/perf/baseline.json` fall with it, and the
allocation budget in `crates/pgprox-cache/tests/budgets.rs` asserts zero for a
miss rather than describing where two blocks came from.

Every implementation, every fake and every call site moved in one commit, per
non-negotiable 6. The change is mechanical: `async fn` to `fn`, and `.await`
deleted. Two `block_on` helpers written to poll a future that never yields — one
in `bin/pgprox`'s tests, one in the cache's bench and budget — are deleted
outright, which is the clearest evidence that the async was never doing
anything.

The recency order still allocates on a hit. That is a `BTreeMap` splitting and
merging nodes rather than anything to do with this contract, and it is `M26.4`.
