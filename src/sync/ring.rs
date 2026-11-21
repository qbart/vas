//! Single-producer, single-consumer ring buffer.

use core::cell::Cell;
use core::fmt;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};

use super::{CachePadded, power_of_two_below};
use crate::Full;
use crate::storage::{Buffer, Fixed, Inline, IntoBuffer, Storage};

/// A first-in, first-out ring buffer between exactly two threads: one that
/// pushes and one that pops.
///
/// For audio buffers, IO completions, or commands from the game thread to
/// the render thread: with a single writer and a single reader each side
/// owns its index, so a push or pop is a plain write and one atomic store,
/// with no compare-and-swap. Cheaper than [`Queue`](super::Queue), which
/// any number of threads can share.
///
/// [`split`](Self::split) it into a [`Producer`] and a [`Consumer`]. Each
/// can be moved to another thread but not shared or cloned, so there is
/// only ever one of each. Both borrow the ring: use scoped threads, or put
/// the ring in `'static` memory (for example
/// [`Arena::alloc_value`](crate::Arena::alloc_value) on a
/// [`StaticBlock`](crate::arena::StaticBlock)) for handles that are
/// `'static`.
///
/// ```
/// use vas::Inline;
/// use vas::sync::Ring;
///
/// let mut commands = Ring::<u32, Inline<64>>::inline();
/// let (producer, consumer) = commands.split();
/// std::thread::scope(|scope| {
///     scope.spawn(move || {
///         for command in 0..100 {
///             let mut command = command;
///             while let Err(vas::Full(back)) = producer.push(command) {
///                 command = back; // full: the reader is behind
///             }
///         }
///     });
///     let mut received = 0;
///     while received < 100 {
///         if let Some(command) = consumer.pop() {
///             assert_eq!(command, received);
///             received += 1;
///         }
///     }
/// });
/// ```
///
/// Memory comes from the caller, as plain `T`s: inline in the ring, or
/// [`Borrowed`](crate::Borrowed), for example `arena.alloc::<T>(1024)?`.
/// Only the largest power of two of the slots given is used.
///
/// The handles cannot be shared between threads:
///
/// ```compile_fail
/// use vas::Inline;
/// use vas::sync::Ring;
///
/// let mut ring = Ring::<u32, Inline<4>>::inline();
/// let (producer, _) = ring.split();
/// std::thread::scope(|scope| {
///     scope.spawn(|| producer.push(1)); // `&Producer` is not `Send`
///     scope.spawn(|| producer.push(2));
/// });
/// ```
///
/// # Guarantees
///
/// - Values come out in the order they went in, each exactly once, or are
///   dropped with the ring.
/// - Wait-free: `push` and `pop` finish in a bounded number of steps,
///   whatever the other thread does. A full or empty ring answers at once.
/// - `len` and `is_empty` on the handles are snapshots that the other side
///   may change at once.
pub struct Ring<T, S: Fixed<T>> {
    // Invariant: `capacity` is 0 or a power of two no larger than the
    // number of slots. Positions count up forever (wrapping); position `p`
    // lives in slot `p % capacity`. Positions `head..tail` hold written
    // values, and `tail - head <= capacity`. Only the producer moves
    // `tail`, only the consumer moves `head`.
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    slots: <S as Storage<T>>::Buffer,
    capacity: usize,
    marker: PhantomData<T>,
}

impl<T, const N: usize> Ring<T, Inline<N>> {
    /// An empty ring holding its slots inline. `const`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            slots: [const { MaybeUninit::uninit() }; N],
            capacity: power_of_two_below(N),
            marker: PhantomData,
        }
    }
}

impl<T, const N: usize> Default for Ring<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<T, S: Fixed<T>> Ring<T, S> {
    /// An empty ring over `buffer`, for example `arena.alloc::<T>(1024)?`.
    /// O(1).
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<T, Storage = S>,
    {
        let slots = buffer.into_buffer();
        let capacity = power_of_two_below(slots.slots().len());
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            slots,
            capacity,
            marker: PhantomData,
        }
    }

    /// The two ends: a [`Producer`] that pushes and a [`Consumer`] that
    /// pops. Values left from earlier handles stay in the ring.
    #[inline]
    pub fn split(&mut self) -> (Producer<'_, T>, Consumer<'_, T>) {
        let head = *self.head.get_mut();
        let tail = *self.tail.get_mut();
        let capacity = self.capacity;
        // Taken once here, while the ring is borrowed exclusively; the
        // handles reach the slots only through it. The borrow of the
        // buffer ends at once, and `head` and `tail` below are other
        // fields.
        let slots = NonNull::from(self.slots.slots_mut()).cast::<MaybeUninit<T>>();
        let producer = Producer {
            head: &self.head,
            tail: &self.tail,
            slots,
            capacity,
            tail_own: Cell::new(tail),
            head_seen: Cell::new(head),
            marker: PhantomData,
        };
        let consumer = Consumer {
            head: &self.head,
            tail: &self.tail,
            slots,
            capacity,
            head_own: Cell::new(head),
            tail_seen: Cell::new(tail),
            marker: PhantomData,
        };
        (producer, consumer)
    }

    /// How many values fit: the largest power of two of the slots given.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many values are in the ring.
    #[inline]
    pub fn len(&self) -> usize {
        // No handle exists while `&self` does, so this is exact.
        self.tail
            .load(Relaxed)
            .wrapping_sub(self.head.load(Relaxed))
    }

    /// `true` when the ring holds nothing.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T, S: Fixed<T>> Drop for Ring<T, S> {
    fn drop(&mut self) {
        if !core::mem::needs_drop::<T>() || self.capacity == 0 {
            return;
        }
        let head = *self.head.get_mut();
        let tail = *self.tail.get_mut();
        let mask = self.capacity - 1;
        let slots = self.slots.slots_mut();
        let mut position = head;
        while position != tail {
            if let Some(slot) = slots.get_mut(position & mask) {
                // SAFETY: positions `head..tail` hold written values (the
                // invariant), each dropped only here.
                unsafe { slot.assume_init_drop() };
            }
            position = position.wrapping_add(1);
        }
    }
}

impl<T, S: Fixed<T>> fmt::Debug for Ring<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ring")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}

/// The pushing end of a [`Ring`]. `Send`, not `Sync`, not `Clone`: one
/// producer, which may move to another thread.
pub struct Producer<'r, T> {
    head: &'r AtomicUsize,
    tail: &'r AtomicUsize,
    // The ring's slots, of which the first `capacity` are used.
    slots: NonNull<MaybeUninit<T>>,
    capacity: usize,
    // `tail`, which only this handle changes, and the last `head` read.
    tail_own: Cell<usize>,
    head_seen: Cell<usize>,
    marker: PhantomData<&'r mut [MaybeUninit<T>]>,
}

// SAFETY: the producer moves values of `T` into slots the consumer moves
// them out of, so `T: Send` is enough. It is not `Sync` (`Cell`), so one
// thread at a time uses it.
unsafe impl<T: Send> Send for Producer<'_, T> {}

impl<T> Producer<'_, T> {
    /// Adds `value` at the back, or hands it back when the ring is full.
    #[inline]
    pub fn push(&self, value: T) -> Result<(), Full<T>> {
        let tail = self.tail_own.get();
        if tail.wrapping_sub(self.head_seen.get()) >= self.capacity {
            // Full as far as this side knows: read `head` again. `Acquire`:
            // once a slot is seen free, the consumer has finished reading
            // its previous value.
            let head = self.head.load(Acquire);
            self.head_seen.set(head);
            if tail.wrapping_sub(head) >= self.capacity {
                return Err(Full(value));
            }
        }
        // SAFETY: `tail - head < capacity`, so `capacity` is not zero and
        // the slot of `tail` (within the first `capacity`) holds no value
        // the consumer may still read; only this handle writes slots.
        unsafe {
            self.slots
                .add(tail & (self.capacity - 1))
                .as_ptr()
                .write(MaybeUninit::new(value));
        }
        let tail = tail.wrapping_add(1);
        self.tail_own.set(tail);
        // `Release`: the consumer that sees the new `tail` sees the value.
        self.tail.store(tail, Release);
        Ok(())
    }

    /// How many values fit.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many values are in the ring. Only a snapshot: the consumer may
    /// take some at once.
    #[inline]
    pub fn len(&self) -> usize {
        self.tail_own.get().wrapping_sub(self.head.load(Relaxed))
    }

    /// `true` when the ring holds nothing. A snapshot, as
    /// [`len`](Self::len).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> fmt::Debug for Producer<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Producer")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}

/// The popping end of a [`Ring`]. `Send`, not `Sync`, not `Clone`: one
/// consumer, which may move to another thread.
pub struct Consumer<'r, T> {
    head: &'r AtomicUsize,
    tail: &'r AtomicUsize,
    // As in `Producer`.
    slots: NonNull<MaybeUninit<T>>,
    capacity: usize,
    // `head`, which only this handle changes, and the last `tail` read.
    head_own: Cell<usize>,
    tail_seen: Cell<usize>,
    marker: PhantomData<&'r mut [MaybeUninit<T>]>,
}

// SAFETY: as for `Producer`.
unsafe impl<T: Send> Send for Consumer<'_, T> {}

impl<T> Consumer<'_, T> {
    /// Takes the value at the front, or `None` when the ring is empty.
    #[inline]
    pub fn pop(&self) -> Option<T> {
        let head = self.head_own.get();
        if head == self.tail_seen.get() {
            // Empty as far as this side knows: read `tail` again.
            // `Acquire`: the values below it are visible.
            let tail = self.tail.load(Acquire);
            self.tail_seen.set(tail);
            if head == tail {
                return None;
            }
        }
        // SAFETY: `head != tail`, so `capacity` is not zero and position
        // `head` holds a written value (the invariant), visible through
        // the `Acquire` above; only this handle reads slots and the
        // producer does not write this one until `head` moves past it.
        let value = unsafe {
            self.slots
                .add(head & (self.capacity - 1))
                .as_ptr()
                .read()
                .assume_init()
        };
        let head = head.wrapping_add(1);
        self.head_own.set(head);
        // `Release`: the producer that sees the new `head` writes the slot
        // only after this read.
        self.head.store(head, Release);
        Some(value)
    }

    /// How many values fit.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many values are in the ring. Only a snapshot: the producer may
    /// add some at once.
    #[inline]
    pub fn len(&self) -> usize {
        self.tail.load(Relaxed).wrapping_sub(self.head_own.get())
    }

    /// `true` when the ring holds nothing. A snapshot, as
    /// [`len`](Self::len).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> fmt::Debug for Consumer<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Consumer")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}
