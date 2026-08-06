//! Pooled I/O buffers.
//!
//! At 100k connections per node, a 16 KiB read buffer and a 16 KiB write buffer
//! per connection is 3.2 GB of memory doing nothing, because most connections
//! are idle most of the time. That is the entire reason the proxy is worth
//! building, so per-connection buffers do not survive contact with the target.
//!
//! A connection instead borrows a buffer when its socket becomes readable and
//! returns it when quiescent, so an idle connection costs a socket and a small
//! state struct rather than 32 KiB.
//!
//! # Backpressure
//!
//! [`BufferSlab::try_borrow`] returns [`None`] when every buffer is out, rather
//! than allocating. That converts a memory spike under a synchronized burst
//! into latency, which is the correct direction to fail. Waiting for a buffer
//! is the caller's problem: this crate performs no I/O and knows nothing about
//! a runtime.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Default buffer size, chosen to hold a typical Postgres message without
/// growing while staying well under a page-multiple that would waste memory.
pub const DEFAULT_BUFFER_SIZE: usize = 16 * 1024;

/// A bounded pool of reusable byte buffers.
///
/// Sharding is deliberately not implemented yet. It is an optimization, and
/// `docs/internal/standards/testing.md` requires measurement before optimizing; the reference
/// workload that would justify it does not exist until M7.
pub struct BufferSlab {
    free: Mutex<Vec<Vec<u8>>>,
    buffer_size: usize,
    max_outstanding: usize,
    outstanding: AtomicUsize,
}

impl fmt::Debug for BufferSlab {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufferSlab")
            .field("buffer_size", &self.buffer_size)
            .field("max_outstanding", &self.max_outstanding)
            .field("outstanding", &self.outstanding.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl BufferSlab {
    /// Builds a slab handing out `max_outstanding` buffers of `buffer_size`.
    #[must_use]
    pub fn new(buffer_size: usize, max_outstanding: usize) -> Arc<Self> {
        Arc::new(Self {
            free: Mutex::new(Vec::new()),
            buffer_size,
            max_outstanding,
            outstanding: AtomicUsize::new(0),
        })
    }

    /// Takes a buffer, or [`None`] if the slab is exhausted.
    ///
    /// Reuses a returned buffer's allocation when one is available, so a warm
    /// slab does not allocate on the relay path.
    pub fn try_borrow(self: &Arc<Self>) -> Option<PooledBuf> {
        // Claim a slot first. Doing it the other way round would let two
        // callers both see room and both allocate past the bound.
        let claimed = self
            .outstanding
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.max_outstanding).then_some(n + 1)
            });
        if claimed.is_err() {
            return None;
        }

        let buf = self
            .lock_free_list()
            .pop()
            .unwrap_or_else(|| Vec::with_capacity(self.buffer_size));

        Some(PooledBuf {
            buf,
            slab: Arc::clone(self),
        })
    }

    /// How many buffers are currently checked out.
    #[must_use]
    pub fn outstanding(&self) -> usize {
        self.outstanding.load(Ordering::SeqCst)
    }

    /// How many buffers are sitting idle, available for reuse without
    /// allocating.
    #[must_use]
    pub fn idle(&self) -> usize {
        self.lock_free_list().len()
    }

    /// The most buffers this slab will hand out at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.max_outstanding
    }

    fn lock_free_list(&self) -> std::sync::MutexGuard<'_, Vec<Vec<u8>>> {
        // A poisoned lock means another thread panicked while holding it. The
        // free list is a plain Vec of byte buffers with no invariant that a
        // panic could break, so recovering is correct and is better than
        // propagating a panic into the relay path.
        self.free
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn give_back(&self, mut buf: Vec<u8>) {
        // Clearing is a security requirement, not hygiene. A buffer returned
        // with one tenant's bytes still in it and handed to another connection
        // would leak query data across tenants. `clear` sets the length to zero
        // while keeping the allocation, which is exactly what is wanted: the
        // bytes are unreachable through the safe API, and the next borrower
        // cannot read past its own written length.
        buf.clear();

        // Drop an over-grown buffer rather than keeping it forever. One large
        // result must not permanently inflate the slab's memory.
        if buf.capacity() <= self.buffer_size * 2 {
            self.lock_free_list().push(buf);
        }

        self.outstanding.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A buffer borrowed from a [`BufferSlab`], returned when dropped.
pub struct PooledBuf {
    buf: Vec<u8>,
    slab: Arc<BufferSlab>,
}

impl PooledBuf {
    /// The buffer's current contents.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    /// The buffer's contents, mutably.
    pub fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        &mut self.buf
    }
}

impl fmt::Debug for PooledBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print contents: this holds client traffic, which routinely
        // carries customer data in SQL literals.
        f.debug_struct("PooledBuf")
            .field("len", &self.buf.len())
            .field("capacity", &self.buf.capacity())
            .finish_non_exhaustive()
    }
}

impl Deref for PooledBuf {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl DerefMut for PooledBuf {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}

impl Drop for PooledBuf {
    fn drop(&mut self) {
        let buf = std::mem::take(&mut self.buf);
        self.slab.give_back(buf);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn the_default_buffer_size_is_sixteen_kibibytes() {
        // `16 * 1024` could become `16 / 1024`, which is zero, or `16 + 1024`.
        // Nothing asserted the value, only that buffers worked, and a slab of
        // zero-length buffers still satisfies "a buffer was handed out". The
        // constant's own doc says it is chosen to hold a typical Postgres
        // message without growing, which is a claim about this number.
        assert_eq!(DEFAULT_BUFFER_SIZE, 16_384);
    }

    #[test]
    fn a_dropped_buffer_returns_to_the_slab() {
        let slab = BufferSlab::new(64, 4);
        assert_eq!(slab.outstanding(), 0);

        let buf = slab.try_borrow().unwrap();
        assert_eq!(slab.outstanding(), 1);
        assert_eq!(slab.idle(), 0);

        drop(buf);
        assert_eq!(slab.outstanding(), 0);
        assert_eq!(slab.idle(), 1, "buffer was not returned for reuse");
    }

    #[test]
    fn the_slab_is_bounded_rather_than_allocating() {
        // The property that turns a memory spike into latency.
        let slab = BufferSlab::new(64, 2);
        let a = slab.try_borrow().unwrap();
        let b = slab.try_borrow().unwrap();
        assert!(slab.try_borrow().is_none(), "slab exceeded its bound");
        assert_eq!(slab.outstanding(), 2);

        drop(a);
        assert!(slab.try_borrow().is_some(), "slot did not free up");
        drop(b);
    }

    #[test]
    fn a_warm_slab_reuses_the_allocation() {
        // The relay loop must not allocate. Same pointer means same allocation.
        let slab = BufferSlab::new(64, 2);

        let mut first = slab.try_borrow().unwrap();
        first.extend_from_slice(b"hello");
        let first_ptr = first.as_ptr();
        drop(first);

        let second = slab.try_borrow().unwrap();
        assert_eq!(second.as_ptr(), first_ptr, "allocation was not reused");
        assert!(second.capacity() >= 64);
    }

    #[test]
    fn a_reused_buffer_carries_no_previous_contents() {
        // Security, not hygiene. A buffer handed on with a previous tenant's
        // bytes still readable would leak query data across tenants.
        let slab = BufferSlab::new(64, 1);

        let mut first = slab.try_borrow().unwrap();
        first.extend_from_slice(b"SELECT secret FROM tenant_a");
        drop(first);

        let second = slab.try_borrow().unwrap();
        assert!(second.is_empty(), "reused buffer still had a length");
        assert_eq!(second.as_slice(), b"", "previous contents were readable");
    }

    #[test]
    fn an_over_grown_buffer_is_dropped_rather_than_kept() {
        // One large result must not permanently inflate the slab.
        let slab = BufferSlab::new(64, 2);
        let mut buf = slab.try_borrow().unwrap();
        buf.extend_from_slice(&[0_u8; 64 * 8]);
        drop(buf);

        assert_eq!(slab.idle(), 0, "over-grown buffer was retained");
        assert_eq!(slab.outstanding(), 0, "slot was not released");
    }

    #[test]
    fn a_moderately_grown_buffer_is_kept() {
        let slab = BufferSlab::new(64, 2);
        let mut buf = slab.try_borrow().unwrap();
        buf.extend_from_slice(&[0_u8; 96]);
        drop(buf);
        assert_eq!(slab.idle(), 1, "usefully sized buffer was thrown away");
    }

    #[test]
    fn buffers_are_writable_through_deref() {
        let slab = BufferSlab::new(64, 1);
        let mut buf = slab.try_borrow().unwrap();
        buf.extend_from_slice(b"abc");
        assert_eq!(buf.len(), 3);
        assert_eq!(&buf[..], b"abc");
        buf.as_mut_vec().push(b'd');
        assert_eq!(buf.as_slice(), b"abcd");
    }

    #[test]
    fn debug_never_prints_contents() {
        // Buffers hold client traffic, which carries customer data in SQL
        // literals.
        let slab = BufferSlab::new(64, 1);
        let mut buf = slab.try_borrow().unwrap();
        buf.extend_from_slice(b"SELECT ssn FROM people");
        let rendered = format!("{buf:?}");
        assert!(!rendered.contains("ssn"), "leaked in {rendered}");
        assert!(rendered.contains("len"));

        let slab_rendered = format!("{slab:?}");
        assert!(slab_rendered.contains("outstanding"));
    }

    #[test]
    fn capacity_reports_the_bound() {
        let slab = BufferSlab::new(DEFAULT_BUFFER_SIZE, 7);
        assert_eq!(slab.capacity(), 7);
    }

    #[test]
    fn concurrent_borrowers_never_exceed_the_bound() {
        // The check-then-claim race: two threads both seeing room and both
        // taking the last slot.
        use std::sync::atomic::AtomicBool;
        use std::thread;

        const BOUND: usize = 8;
        let slab = BufferSlab::new(64, BOUND);
        let breached = Arc::new(AtomicBool::new(false));

        thread::scope(|scope| {
            for _ in 0..16 {
                let slab = Arc::clone(&slab);
                let breached = Arc::clone(&breached);
                scope.spawn(move || {
                    for _ in 0..500 {
                        if let Some(buf) = slab.try_borrow() {
                            if slab.outstanding() > BOUND {
                                breached.store(true, Ordering::SeqCst);
                            }
                            drop(buf);
                        }
                    }
                });
            }
        });

        assert!(!breached.load(Ordering::SeqCst), "slab exceeded its bound");
        assert_eq!(slab.outstanding(), 0, "slots leaked");
    }
}
