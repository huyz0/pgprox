# Async and concurrency

## Sans-I/O is the load-bearing rule

Business logic is a pure function of state and input events. It does not touch a
socket, a clock, or a syscall.

```rust
// Yes. Testable at memory speed, no runtime needed.
impl SessionState {
    pub fn on_frame(&mut self, frame: Frame<'_>) -> Action { ... }
}

// No. Now the pin-detection logic needs a Postgres to test.
impl Session {
    pub async fn handle(&mut self, sock: &mut TcpStream) -> Result<()> { ... }
}
```

This applies to the protocol codec, the session state machine, the pin
detector, the statement classifier, the quota arithmetic, and the routing
decision. Between them that is most of the logic in the project.

Two reasons, and the second is the one people underestimate. First, it is the
only way to hold 95% coverage in a two-minute test run, because tests that open
sockets are slow and flaky. Second, concurrency bugs in a state machine you can
drive deterministically are findable, and the same bugs behind a socket are not.

The I/O shell that wraps these state machines is generic over
`AsyncRead + AsyncWrite + Unpin`, so tests drive it with `tokio::io::duplex` and
never bind a port.

## Time is injected

Nothing calls `Instant::now()` or `tokio::time::sleep` directly. Take the
`Clock` trait from `pgprox-core`. Tests use `tokio::time::pause`, and the
cluster simulation uses a virtual clock that can advance a lease TTL in
microseconds.

A test that sleeps in wall-clock time is a bug. It is also how a two-minute
suite becomes a twenty-minute suite one test at a time.

## Blocking

Nothing blocks on the async runtime. No file I/O, no DNS, no CPU work longer
than a few microseconds on a task that also serves connections. Where blocking
is unavoidable, `tokio::task::spawn_blocking`, and say why in a comment.

The relay loop is the hottest path in the process. Nothing allocates there, and
nothing in it acquires a lock that another task can hold across an await.

## Locks

- Prefer message passing over shared state. `tokio::sync::mpsc` to an owner
  task beats a `Mutex` that every connection contends on at 100k connections.
- Never hold a `std::sync::Mutex` across an await. Clippy catches the obvious
  cases and not the subtle ones, so this is also a review item.
- Where shared state is genuinely right, prefer sharding by key over one lock.
  The pool map and the grant cache are both sharded.
- Document the lock order anywhere two locks can be held at once. Better still,
  restructure so they cannot.

## Cancellation safety

Every `async fn` that can appear in a `select!` branch must be cancellation
safe, meaning dropping the future mid-flight leaves no torn state. This is the
subtlest class of bug in the codebase, because it only shows up under load when
a client disconnects at exactly the wrong moment.

Concretely: a partially read frame must not leave the codec believing it has
consumed bytes it has not. An upstream connection released mid-transaction must
be closed, not returned to the pool. When a function is not cancellation safe,
say so in its doc comment and do not put it in a `select!`.

## Task structure

One task per client connection, one per upstream connection, plus the background
set (gossip, replica poller, idle reaper, config watcher). A Tokio task is cheap;
a per-connection buffer is not, which is why buffers are borrowed from the slab
on demand rather than owned for the connection's lifetime. See
[testing.md](testing.md) for the allocation budgets that enforce this.

Background tasks are spawned with a handle held by their owner and a shutdown
signal, never detached. A detached task that outlives its config reload is a
leak nobody notices until drain hangs.
