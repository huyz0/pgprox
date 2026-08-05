//! What a spawned task costs beyond the future it holds. `M37.1`.
//!
//! # Why this exists
//!
//! `M36` measured an idle connection at roughly 15 KB and accounted for 5,048
//! of it as the session future. Three milestones ruled out everything else that
//! was proposed: not the read and write buffers, not the allocator arenas, not
//! the prepared statement map. What was left unweighed is the difference
//! between a future and a task.
//!
//! `size_of_val` on a future is the future. `tokio::spawn` puts it in a
//! heap-allocated task alongside a header holding the waker, the state, the
//! join handle's half of the channel and the scheduler's intrusive links, and
//! then the allocator rounds the pair up to a size it is willing to hand out.
//! The test that guards the session future's size measures exactly the part
//! that was already accounted for.
//!
//! # What this measures and what it does not
//!
//! `dhat` reports bytes requested from the allocator. It does not see what
//! glibc rounds a request up to, nor the arena bookkeeping around it, so a
//! figure here is a floor on the resident cost rather than the whole of it.
//! `M36`'s numbers are resident memory and are not directly comparable; what is
//! comparable is the shape.
//!
//! # Several sizes, not one
//!
//! The answer is a relationship. One point cannot tell a constant header from
//! one that grows with the future, and the two say different things about what
//! to do next: a constant is paid once per connection and a proportional one is
//! paid again every time the future grows.

#![allow(clippy::unwrap_used, clippy::panic, clippy::print_stdout)]

use std::hint::black_box;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// How many tasks each measurement spawns.
///
/// Enough that the per-task figure is not one allocation's rounding, few enough
/// that the runtime is not itself growing while the measurement runs.
const TASKS: usize = 256;

/// A future that parks at an await holding `N` bytes across it.
///
/// The `black_box` after the await was written to be what keeps the array in
/// the future, and it is not: moving it before the await, so nothing uses the
/// array afterwards, leaves the size unchanged. `rustc` keeps a local declared
/// before an await in the state machine whether or not anything reads it later.
///
/// So the assertion on `size_of_val` below is not a check that a trick worked.
/// It is the only thing here that knows the future is the size the test asked
/// for, and it is load-bearing on its own.
async fn parked<const N: usize>(gate: tokio::sync::oneshot::Receiver<()>) {
    let pad = [0_u8; N];
    let _ = gate.await;
    black_box(&pad);
}

/// Bytes the allocator was asked for while `body` ran, and did not get back.
fn held(body: impl FnOnce()) -> usize {
    let before = dhat::HeapStats::get().curr_bytes;
    body();
    dhat::HeapStats::get().curr_bytes - before
}

/// Spawns `TASKS` copies of a future of `N` bytes and returns the bytes held
/// per task, along with the future's own size.
fn cost_of_spawning<const N: usize>(runtime: &tokio::runtime::Runtime) -> (usize, usize) {
    let (_probe_tx, probe_rx) = tokio::sync::oneshot::channel();
    let future = parked::<N>(probe_rx);
    let future_size = std::mem::size_of_val(&future);
    drop(future);

    let mut gates = Vec::with_capacity(TASKS);
    let mut handles = Vec::with_capacity(TASKS);

    // The senders are kept out of the measurement: they are the test's, not the
    // task's. Allocated up front so the vector does not grow inside `held`.
    let mut senders = Vec::with_capacity(TASKS);
    for _ in 0..TASKS {
        let (tx, rx) = tokio::sync::oneshot::channel();
        senders.push(tx);
        gates.push(rx);
    }

    let bytes = held(|| {
        for gate in gates.drain(..) {
            handles.push(runtime.spawn(parked::<N>(gate)));
        }
        // Every task must reach its await before the measurement, or some of
        // them are still a future on this thread rather than a task on the
        // runtime. A yield is enough: the tasks park on a channel nobody has
        // sent to.
        std::thread::sleep(std::time::Duration::from_millis(50));
    });

    for tx in senders {
        let _ = tx.send(());
    }
    runtime.block_on(async {
        for handle in handles {
            let _ = handle.await;
        }
    });

    (future_size, bytes / TASKS)
}

/// One `#[test]`, several sections. `dhat`'s profiler is process-wide and
/// separate test functions run on separate threads in the same process, so they
/// would fight over it. Same reason the allocation budgets are one function.
#[test]
fn a_spawned_task_costs_its_future_plus_a_header() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    // Warm: the first spawn grows the runtime's own queues, and that is not a
    // per-task cost.
    let _ = cost_of_spawning::<64>(&runtime);

    let (small_size, small) = cost_of_spawning::<64>(&runtime);
    let (medium_size, medium) = cost_of_spawning::<1024>(&runtime);
    let (session_size, session) = cost_of_spawning::<4096>(&runtime);
    let (large_size, large) = cost_of_spawning::<16384>(&runtime);

    println!("future bytes -> held per task");
    for (size, cost) in [
        (small_size, small),
        (medium_size, medium),
        (session_size, session),
        (large_size, large),
    ] {
        println!(
            "  {size:>6} -> {cost:>6}   overhead {:>5}",
            cost.saturating_sub(size)
        );
    }

    // The arrays survived into the futures. Without this the sizes below are
    // whatever the optimiser left and every conclusion drawn from them is about
    // a different program.
    assert!(
        small_size >= 64 && medium_size >= 1024 && session_size >= 4096 && large_size >= 16384,
        "a padding array was optimised out of its future: {small_size}, {medium_size}, \
         {session_size}, {large_size}"
    );

    // A task costs at least its future. Anything else would mean the future is
    // not in the allocation, which is the thing being measured.
    assert!(
        session >= session_size,
        "a task holding a {session_size} byte future held only {session} bytes"
    );

    // And the overhead is a header rather than a second copy. The distinction
    // is the whole point: a constant is paid once per connection, and something
    // proportional would mean every byte added to the session future costs two.
    // `saturating_sub` rather than a signed difference: a task cannot hold less
    // than its future, the assertion above says so, and a subtraction that
    // could go negative would need a cast clippy refuses on a 64-bit target.
    let small_overhead = small.saturating_sub(small_size);
    let large_overhead = large.saturating_sub(large_size);
    assert!(
        small_overhead.abs_diff(large_overhead) < 512,
        "the overhead grew with the future, {small_overhead} to {large_overhead}, \
         so it is not a fixed header"
    );
}
