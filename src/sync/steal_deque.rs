//! Work-stealing deque: one owner, any number of thieves.

use core::cell::Cell;
use core::fmt;
use core::marker::PhantomData;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release, SeqCst};
use core::sync::atomic::{AtomicUsize, fence};

use super::{CachePadded, Slot, power_of_two_below, stamp_slots, stamped_array};
use crate::Full;
use crate::storage::{Buffer, Fixed, Inline, IntoBuffer, Storage};

/// A deque of jobs for one worker thread, which other threads can steal
/// from: Chase and Lev's work-stealing deque, of fixed capacity.
///
/// [`split`](Self::split) it into its two roles:
///
/// - a [`Worker`] for the owner thread, which pushes and pops at the back
///   (last in, first out: the job it just pushed is hot in its cache). It
///   can be moved to another thread but not shared or cloned, so there is
///   only ever one owner.
/// - a [`Stealer`] for every other thread, which takes from the front
///   (first in, first out: the oldest, usually biggest, jobs). It is
///   `Copy` and `Sync`.
///
/// Both borrow the deque, so no reference counting or allocation is
/// involved. Use scoped threads, or put the deque in `'static` memory
/// (for example [`Arena::alloc_value`](crate::Arena::alloc_value) on a
/// [`StaticBlock`](crate::arena::StaticBlock)) for handles that are
/// `'static`.
///
/// ```
/// use vas::Inline;
/// use vas::sync::{Steal, StealDeque};
///
/// let mut jobs = StealDeque::<u32, Inline<256>>::inline();
/// let (worker, stealer) = jobs.split();
/// for job in 0..100 {
///     assert!(worker.push(job).is_ok());
/// }
/// let mut stolen = 0;
/// std::thread::scope(|scope| {
///     let thief = scope.spawn(move || {
///         let mut count = 0;
///         loop {
///             match stealer.steal() {
///                 Steal::Success(_) => count += 1,
///                 Steal::Retry => continue,
///                 Steal::Empty => break count,
///             }
///         }
///     });
///     let mut mine = 0;
///     while worker.pop().is_some() {
///         mine += 1;
///     }
///     stolen = thief.join().unwrap_or(0) + mine;
/// });
/// assert_eq!(stolen, 100);
/// ```
///
/// Memory comes from the caller, as `Slot<T>`s: inline, or
/// [`Borrowed`](crate::Borrowed). Only the largest power of two of the
/// slots given is used, so give a power of two.
///
/// # Guarantees
///
/// - Every value pushed is taken exactly once, by the owner or by one
///   thief, or dropped with the deque.
/// - Lock-free; nothing blocks. A thief that loses a race to another thief
///   gets [`Steal::Retry`] instead of trying again itself.
/// - [`Worker::push`] returns [`Full`] when the deque is full, and also,
///   momentarily, when the slot it needs is still being read by a thief
///   that took the value one lap earlier.
/// - `len` and `is_empty` are snapshots that other threads may change at
///   once.
pub struct StealDeque<T, S: Fixed<Slot<T>>> {
    // Invariant: `capacity` is 0 or a power of two no larger than the
    // number of slots, and the first `capacity` slots are initialised.
    // Position `p` (counted with wrapping) lives in slot `p % capacity`.
    // Positions `top..bottom` hold the values in the deque; the owner
    // pushes and pops at `bottom`, thieves take at `top` by moving it up
    // one. A slot's stamp is `p` when it is free for a push of position
    // `p`: it becomes `p + capacity` once the value of position `p` has
    // been taken from the top and read.
    top: CachePadded<AtomicUsize>,
    bottom: CachePadded<AtomicUsize>,
    slots: <S as Storage<Slot<T>>>::Buffer,
    capacity: usize,
    marker: PhantomData<T>,
}

// SAFETY: values only move through the deque, so `T: Send` is enough; no
// `&T` is ever handed out. Only the one `Worker` (which `split` hands out
// from `&mut self`, and which is not `Sync` or `Clone`) writes slots and
// `bottom`. A value is read once, by whoever claimed its position: the
// owner alone for positions above `top`, or the winner of the
// compare-and-swap on `top` for the last one. The orderings that make the
// writes visible to those reads, and the reads finish before a slot is
// written again, are explained on each access and in `docs/steal_deque.md`.
// Through `&self` the buffer of fixed storage is only read.
unsafe impl<T: Send, S: Fixed<Slot<T>>> Sync for StealDeque<T, S> {}

impl<T, const N: usize> StealDeque<T, Inline<N>> {
    /// An empty deque holding its slots inline. `const`.
    #[inline]
    pub const fn inline() -> Self {
        let capacity = power_of_two_below(N);
        Self {
            top: CachePadded::new(AtomicUsize::new(0)),
            bottom: CachePadded::new(AtomicUsize::new(0)),
            slots: stamped_array(capacity, 1),
            capacity,
            marker: PhantomData,
        }
    }
}

impl<T, const N: usize> Default for StealDeque<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<T, S: Fixed<Slot<T>>> StealDeque<T, S> {
    /// An empty deque over `buffer`, for example
    /// `arena.alloc::<Slot<T>>(256)?`. O(capacity), to stamp the slots.
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Slot<T>, Storage = S>,
    {
        let mut slots = buffer.into_buffer();
        let capacity = power_of_two_below(slots.slots().len());
        stamp_slots(slots.slots_mut(), capacity, 1);
        Self {
            top: CachePadded::new(AtomicUsize::new(0)),
            bottom: CachePadded::new(AtomicUsize::new(0)),
            slots,
            capacity,
            marker: PhantomData,
        }
    }

    /// Splits the deque into the owner's handle and a thieves' handle.
    /// Copy the [`Stealer`] for as many threads as needed. The deque cannot
    /// be split again until both are gone, so there is one owner at a time.
    #[inline]
    pub fn split(&mut self) -> (Worker<'_, T, S>, Stealer<'_, T, S>) {
        let deque = &*self;
        (
            Worker {
                deque,
                marker: PhantomData,
            },
            Stealer { deque },
        )
    }

    /// How many values fit: the largest power of two of the slots given.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many values are in the deque. Only a snapshot: other threads
    /// may change it at any moment.
    #[inline]
    pub fn len(&self) -> usize {
        let top = self.top.load(Relaxed);
        let bottom = self.bottom.load(Relaxed);
        // During a pop `bottom` is briefly one below `top`.
        let len = bottom.wrapping_sub(top);
        if len.cast_signed() < 0 {
            0
        } else {
            len.min(self.capacity)
        }
    }

    /// `true` when the deque holds nothing. Only a snapshot, as
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

    /// The owner's push. Only [`Worker`] calls it.
    #[inline]
    fn push(&self, value: T) -> Result<(), Full<T>> {
        if self.capacity == 0 {
            return Err(Full(value));
        }
        // Only the owner writes `bottom`.
        let bottom = self.bottom.load(Relaxed);
        let slot = self.slot(bottom);
        // `Acquire`: when the stamp says the slot is free, the thief that
        // took its previous value has finished reading it. A stamp below
        // `bottom` means that value is still in the deque (full), or is
        // still being read.
        if slot.stamp.load(Acquire) != bottom {
            return Err(Full(value));
        }
        // SAFETY: only the owner writes slots, and nobody reads this one:
        // its position is not between `top` and `bottom` yet, and the
        // previous value's reader is done (above).
        unsafe { (*slot.value.get()).write(value) };
        // `Release`: a thief that sees the new `bottom` sees the value.
        self.bottom.store(bottom.wrapping_add(1), Release);
        Ok(())
    }

    /// The owner's pop. Only [`Worker`] calls it.
    #[inline]
    fn pop(&self) -> Option<T> {
        if self.capacity == 0 {
            return None;
        }
        let bottom = self.bottom.load(Relaxed).wrapping_sub(1);
        // Reserve the last value before looking at `top`. `Release`, like
        // every store of `bottom`: a thief that reads this value of
        // `bottom` also sees all the values pushed before it.
        self.bottom.store(bottom, Release);
        // `SeqCst` fence, paired with the one in `steal`: either the thief
        // sees the lowered `bottom`, or this load sees the thief's move of
        // `top`. Without it both could take the last value.
        fence(SeqCst);
        let top = self.top.load(Relaxed);
        let left = bottom.wrapping_sub(top).cast_signed();
        if left < 0 {
            // It was empty: undo the reservation.
            self.bottom.store(bottom.wrapping_add(1), Release);
            return None;
        }
        let slot = self.slot(bottom);
        if left > 0 {
            // More than one value: thieves cannot reach this one.
            // SAFETY: the position is below `bottom` and above `top`, so
            // its value is written and only the owner can take it.
            return Some(unsafe { (*slot.value.get()).assume_init_read() });
        }
        // The last value: race the thieves for it on `top`.
        let won = self
            .top
            .compare_exchange(top, top.wrapping_add(1), SeqCst, Relaxed)
            .is_ok();
        self.bottom.store(bottom.wrapping_add(1), Release);
        if !won {
            return None;
        }
        // SAFETY: winning the compare-and-swap made this the only reader
        // of the position, which the owner itself wrote.
        let value = unsafe { (*slot.value.get()).assume_init_read() };
        // Taken from the top: free the slot for the next lap. Only the
        // owner reads this stamp, so `Relaxed` is enough.
        slot.stamp
            .store(bottom.wrapping_add(self.capacity), Relaxed);
        Some(value)
    }

    /// A thief's steal. Only [`Stealer`] calls it.
    #[inline]
    fn steal(&self) -> Steal<T> {
        if self.capacity == 0 {
            return Steal::Empty;
        }
        let top = self.top.load(Acquire);
        // `SeqCst` fence, paired with the one in `pop` (see there).
        fence(SeqCst);
        // `Acquire`: the values below this `bottom` are visible.
        let bottom = self.bottom.load(Acquire);
        if bottom.wrapping_sub(top).cast_signed() <= 0 {
            return Steal::Empty;
        }
        // Claim the position before reading it, so that no one else reads
        // or overwrites it meanwhile.
        if self
            .top
            .compare_exchange(top, top.wrapping_add(1), SeqCst, Relaxed)
            .is_err()
        {
            return Steal::Retry;
        }
        let slot = self.slot(top);
        // SAFETY: the claim made this thread the only reader of the
        // position, whose value is written and visible (`bottom` above it
        // was read with `Acquire`). The owner does not write the slot
        // again until the stamp below frees it.
        let value = unsafe { (*slot.value.get()).assume_init_read() };
        // `Release`: the owner's next write to the slot comes after this
        // read.
        slot.stamp.store(top.wrapping_add(self.capacity), Release);
        Steal::Success(value)
    }
}

impl<T, S: Fixed<Slot<T>>> Drop for StealDeque<T, S> {
    fn drop(&mut self) {
        if !core::mem::needs_drop::<T>() || self.capacity == 0 {
            return;
        }
        let top = *self.top.get_mut();
        let bottom = *self.bottom.get_mut();
        let mask = self.capacity - 1;
        let slots = self.slots.slots_mut();
        let mut position = top;
        // With `&mut self` no handle exists, so nothing is under way and
        // every position from `top` to `bottom` holds a value.
        while position != bottom {
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

impl<T, S: Fixed<Slot<T>>> fmt::Debug for StealDeque<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StealDeque")
            .field("len", &self.len())
            .field("capacity", &self.capacity)
            .finish()
    }
}

/// The owner's handle of a [`StealDeque`]: pushes and pops at the back.
///
/// It can be moved to another thread (`Send`), but not shared (`!Sync`) or
/// cloned, so only one thread at a time pushes and pops:
///
/// ```compile_fail
/// use vas::Inline;
/// use vas::sync::StealDeque;
///
/// let mut deque = StealDeque::<u32, Inline<8>>::inline();
/// let (worker, _) = deque.split();
/// std::thread::scope(|scope| {
///     scope.spawn(|| worker.push(1)); // `&Worker` is not `Send`
///     let _ = worker.pop();
/// });
/// ```
pub struct Worker<'d, T, S: Fixed<Slot<T>>> {
    deque: &'d StealDeque<T, S>,
    // `Cell` is `Send` but not `Sync`: so is the worker.
    marker: PhantomData<Cell<()>>,
}

impl<'d, T, S: Fixed<Slot<T>>> Worker<'d, T, S> {
    /// Adds `value` at the back, or hands it back when the deque is full.
    /// O(1).
    #[inline]
    pub fn push(&self, value: T) -> Result<(), Full<T>> {
        self.deque.push(value)
    }

    /// Takes the value at the back, the one pushed last, or `None` when
    /// the deque is empty. O(1).
    #[inline]
    pub fn pop(&self) -> Option<T> {
        self.deque.pop()
    }

    /// A thieves' handle for the same deque.
    #[inline]
    pub fn stealer(&self) -> Stealer<'d, T, S> {
        Stealer { deque: self.deque }
    }

    /// How many values fit.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.deque.capacity
    }

    /// How many values are in the deque. A snapshot: thieves may take
    /// some at any moment.
    #[inline]
    pub fn len(&self) -> usize {
        self.deque.len()
    }

    /// `true` when the deque holds nothing. A snapshot, as
    /// [`len`](Self::len).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.deque.is_empty()
    }
}

impl<T, S: Fixed<Slot<T>>> fmt::Debug for Worker<'_, T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Worker")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}

/// A thief's handle of a [`StealDeque`]: takes from the front. `Copy`,
/// `Send` and `Sync`, so any number of threads can steal.
pub struct Stealer<'d, T, S: Fixed<Slot<T>>> {
    deque: &'d StealDeque<T, S>,
}

impl<T, S: Fixed<Slot<T>>> Clone for Stealer<'_, T, S> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, S: Fixed<Slot<T>>> Copy for Stealer<'_, T, S> {}

impl<T, S: Fixed<Slot<T>>> Stealer<'_, T, S> {
    /// Takes the value at the front, the oldest. Never waits: returns
    /// [`Steal::Retry`] when another thread took that value first, and
    /// [`Steal::Empty`] when there is nothing to take. O(1).
    #[inline]
    pub fn steal(&self) -> Steal<T> {
        self.deque.steal()
    }

    /// How many values fit.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.deque.capacity
    }

    /// How many values are in the deque. A snapshot.
    #[inline]
    pub fn len(&self) -> usize {
        self.deque.len()
    }

    /// `true` when the deque holds nothing. A snapshot.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.deque.is_empty()
    }
}

impl<T, S: Fixed<Slot<T>>> fmt::Debug for Stealer<'_, T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stealer")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .finish()
    }
}

/// What [`Stealer::steal`] got.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Steal<T> {
    /// The deque was empty.
    Empty,
    /// The value at the front.
    Success(T),
    /// Another thread took the value first; the deque may not be empty.
    Retry,
}

impl<T> Steal<T> {
    /// The value, if one was stolen.
    #[inline]
    pub fn success(self) -> Option<T> {
        match self {
            Self::Success(value) => Some(value),
            _ => None,
        }
    }

    /// `true` when the deque was empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// `true` when a value was stolen.
    #[inline]
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    /// `true` when another thread won the race.
    #[inline]
    pub const fn is_retry(&self) -> bool {
        matches!(self, Self::Retry)
    }
}
