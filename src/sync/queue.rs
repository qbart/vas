//! Bounded multi-producer, multi-consumer FIFO queue.

use core::fmt;
use core::marker::PhantomData;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};

use super::{Backoff, CachePadded, Slot, power_of_two_below, stamp_slots, stamped_array};
use crate::Full;
use crate::storage::{Buffer, Fixed, Inline, IntoBuffer, Storage};

/// A first-in, first-out queue of fixed capacity that any number of threads
/// push to and pop from at the same time, through `&self`.
///
/// For jobs injected from any thread, IO completions and work deferred to
/// another thread. It never blocks and never waits: [`push`](Self::push)
/// on a full queue hands the value back in [`Full`], and
/// [`pop`](Self::pop) on an empty one returns `None`, at once.
///
/// ```
/// use vas::Inline;
/// use vas::sync::Queue;
///
/// static JOBS: Queue<u32, Inline<256>> = Queue::inline();
///
/// std::thread::scope(|scope| {
///     for worker in 0..4 {
///         scope.spawn(move || JOBS.push(worker));
///     }
/// });
/// let mut done = Vec::new();
/// while let Some(job) = JOBS.pop() {
///     done.push(job);
/// }
/// done.sort();
/// assert_eq!(done, [0, 1, 2, 3]);
/// ```
///
/// Memory comes from the caller, as `Slot<T>`s: inline in the queue (which
/// can then be a `static`), or [`Borrowed`](crate::Borrowed), for example
/// from an [`Arena`](crate::Arena). Only the largest power of two of the
/// slots given is used, so give a power of two.
///
/// # Guarantees
///
/// - Every value pushed is popped exactly once, or dropped with the queue.
/// - Values from one thread come out in the order that thread pushed them.
/// - Lock-free: a thread that has to retry does so only because another
///   thread's push or pop succeeded meanwhile.
/// - A `pop` that meets a slot another thread has claimed but not finished
///   writing returns `None`, even when values pushed later are ready;
///   likewise a `push` meeting a slot still being read returns [`Full`].
///   Such answers are momentary: retry later.
/// - [`len`](Self::len) and [`is_empty`](Self::is_empty) are snapshots
///   that other threads may change at once.
pub struct Queue<T, S: Fixed<Slot<T>>> {
    // Invariant: `capacity` is 0 or a power of two no larger than the
    // number of slots, and the first `capacity` slots are initialised.
    // Position `p` (counted with wrapping) lives in slot `p % capacity`.
    // Its stamp, also wrapping, is `2p` while the slot is free for the
    // push of position `p`, `2p + 1` once that value is written, and
    // `2(p + capacity)` once it is taken, which frees the slot for the
    // next lap. Pushes claim positions in order on `tail`, pops on `head`.
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    slots: <S as Storage<Slot<T>>>::Buffer,
    capacity: usize,
    marker: PhantomData<T>,
}

// SAFETY: values only move through the queue, so `T: Send` is enough; no
// `&T` is ever handed out. Each value is accessed by one thread at a time:
// a push writes the slot only after claiming its position on `tail`, a pop
// reads it only after claiming it on `head`, and the stamps (stored with
// `Release`, loaded with `Acquire`) make each write happen before the read
// of the same position and each read happen before the next lap's write.
// Through `&self` the buffer of fixed storage is only read.
unsafe impl<T: Send, S: Fixed<Slot<T>>> Sync for Queue<T, S> {}

impl<T, const N: usize> Queue<T, Inline<N>> {
    /// An empty queue holding its slots inline. `const`, so it can
    /// initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        let capacity = power_of_two_below(N);
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            slots: stamped_array(capacity, 2),
            capacity,
            marker: PhantomData,
        }
    }
}

impl<T, const N: usize> Default for Queue<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<T, S: Fixed<Slot<T>>> Queue<T, S> {
    /// An empty queue over `buffer`, for example
    /// `arena.alloc::<Slot<T>>(1024)?`. O(capacity), to stamp the slots.
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Slot<T>, Storage = S>,
    {
        let mut slots = buffer.into_buffer();
        let capacity = power_of_two_below(slots.slots().len());
        stamp_slots(slots.slots_mut(), capacity, 2);
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            slots,
            capacity,
            marker: PhantomData,
        }
    }

    /// How many values fit: the largest power of two of the slots given.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many values are in the queue. Only a snapshot: other threads
    /// may change it at any moment.
    #[inline]
    pub fn len(&self) -> usize {
        let tail = self.tail.load(Relaxed);
        let head = self.head.load(Relaxed);
        // `head` may have moved past the `tail` read before it.
        let len = tail.wrapping_sub(head);
        if len.cast_signed() < 0 {
            0
        } else {
            len.min(self.capacity)
        }
    }

    /// `true` when the queue holds nothing. Only a snapshot, as
    /// [`len`](Self::len).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The slot of `position`. The capacity must not be zero.
    #[inline]
    fn slot(&self, position: usize) -> &Slot<T> {
        let index = position & (self.capacity - 1);
        // SAFETY: `index < capacity`, and the first `capacity` slots are
        // initialised (the invariant).
        unsafe { self.slots.slots().get_unchecked(index).assume_init_ref() }
    }

    /// Adds `value` at the back, or hands it back when the queue is full.
    pub fn push(&self, value: T) -> Result<(), Full<T>> {
        if self.capacity == 0 {
            return Err(Full(value));
        }
        let mut tail = self.tail.load(Relaxed);
        let mut backoff = Backoff::new();
        loop {
            let slot = self.slot(tail);
            // `Acquire`: when the slot is free, the pop that freed it has
            // finished reading the previous value.
            let stamp = slot.stamp.load(Acquire);
            let ahead = stamp.wrapping_sub(tail.wrapping_mul(2)).cast_signed();
            if ahead == 0 {
                // Free for this position: claim it. The stamp carries the
                // ordering, so the claim itself can be `Relaxed`.
                match self
                    .tail
                    .compare_exchange_weak(tail, tail.wrapping_add(1), Relaxed, Relaxed)
                {
                    Ok(_) => {
                        // SAFETY: claiming `tail` made this thread the only
                        // one to touch the slot until it stamps it, and
                        // the previous value was moved out (see above).
                        unsafe { (*slot.value.get()).write(value) };
                        // `Release`: the pop that sees the stamp sees the
                        // value.
                        slot.stamp
                            .store(tail.wrapping_mul(2).wrapping_add(1), Release);
                        return Ok(());
                    }
                    Err(current) => {
                        tail = current;
                        backoff.wait();
                    }
                }
            } else if ahead < 0 {
                // The slot still holds, or is still giving up, the value
                // from one lap ago: the queue is full.
                return Err(Full(value));
            } else {
                // Another push took this position: catch up.
                tail = self.tail.load(Relaxed);
            }
        }
    }

    /// Takes the value at the front, or `None` when the queue is empty.
    pub fn pop(&self) -> Option<T> {
        if self.capacity == 0 {
            return None;
        }
        let mut head = self.head.load(Relaxed);
        let mut backoff = Backoff::new();
        loop {
            let slot = self.slot(head);
            // `Acquire`: when the slot is written, its value is visible.
            let stamp = slot.stamp.load(Acquire);
            let ahead = stamp
                .wrapping_sub(head.wrapping_mul(2).wrapping_add(1))
                .cast_signed();
            if ahead == 0 {
                match self
                    .head
                    .compare_exchange_weak(head, head.wrapping_add(1), Relaxed, Relaxed)
                {
                    Ok(_) => {
                        // SAFETY: the stamp says the value of this position
                        // is written, and claiming `head` made this thread
                        // the only one to read it.
                        let value = unsafe { (*slot.value.get()).assume_init_read() };
                        // `Release`: the next lap's push writes only after
                        // this read.
                        slot.stamp
                            .store(head.wrapping_add(self.capacity).wrapping_mul(2), Release);
                        return Some(value);
                    }
                    Err(current) => {
                        head = current;
                        backoff.wait();
                    }
                }
            } else if ahead < 0 {
                // Not written yet: the queue is empty, or a push is still
                // writing this position.
                return None;
            } else {
                // Another pop took this position: catch up.
                head = self.head.load(Relaxed);
            }
        }
    }
}

impl<T, S: Fixed<Slot<T>>> Drop for Queue<T, S> {
    fn drop(&mut self) {
        if !core::mem::needs_drop::<T>() || self.capacity == 0 {
            return;
        }
        let head = *self.head.get_mut();
        let tail = *self.tail.get_mut();
        let mask = self.capacity - 1;
        let slots = self.slots.slots_mut();
        let mut position = head;
        // With `&mut self` no push or pop is under way, so every position
        // from `head` to `tail` holds a written value.
        while position != tail {
            if let Some(slot) = slots.get_mut(position & mask) {
                // SAFETY: the slot is one of the first `capacity`, so it is
                // initialised, and its value is written (see above) and
                // dropped only here.
                unsafe { slot.assume_init_mut().value.get_mut().assume_init_drop() };
            }
            position = position.wrapping_add(1);
        }
    }
}

impl<T, S: Fixed<Slot<T>>> fmt::Debug for Queue<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Queue")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}
