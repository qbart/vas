//! Containers shared between threads, for job systems and cross-thread
//! work.
//!
//! - [`Queue`]: a bounded multi-producer, multi-consumer FIFO queue.
//! - [`CachePadded`]: keeps a value on a cache line of its own.
//!
//! Like the rest of vas they never allocate and never block: memory comes
//! from the caller ([`Borrowed`](crate::Borrowed) or
//! [`Inline`](crate::Inline) storage, of a fixed size), and a full or empty
//! container answers at once instead of waiting. The containers need atomic
//! operations on pointer-sized integers (`target_has_atomic = "ptr"`);
//! [`CachePadded`] is always available.

#![allow(unsafe_code)]

mod padded;
#[cfg(target_has_atomic = "ptr")]
mod queue;

pub use padded::CachePadded;
#[cfg(target_has_atomic = "ptr")]
pub use queue::Queue;

#[cfg(target_has_atomic = "ptr")]
use core::cell::UnsafeCell;
#[cfg(target_has_atomic = "ptr")]
use core::mem::MaybeUninit;
#[cfg(target_has_atomic = "ptr")]
use core::sync::atomic::AtomicUsize;

/// One element's place in a concurrent container: the value and a stamp
/// saying which turn of the ring it belongs to and whether it is written.
///
/// Give a container `&mut [MaybeUninit<Slot<T>>]` (for example from
/// [`Arena::alloc`](crate::Arena::alloc)); the container sets the stamps.
#[cfg(target_has_atomic = "ptr")]
pub struct Slot<T> {
    stamp: AtomicUsize,
    value: UnsafeCell<MaybeUninit<T>>,
}

#[cfg(target_has_atomic = "ptr")]
impl<T> Slot<T> {
    /// An empty slot with the stamp `stamp`.
    #[inline]
    const fn new(stamp: usize) -> Self {
        Self {
            stamp: AtomicUsize::new(stamp),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }
}

/// The largest power of two that is at most `len`, or 0.
#[cfg(target_has_atomic = "ptr")]
#[inline]
const fn power_of_two_below(len: usize) -> usize {
    match len {
        0 => 0,
        _ => 1 << (usize::BITS - 1 - len.leading_zeros()),
    }
}

/// Writes fresh slots over the first `capacity` of `slots`, the one at
/// `index` stamped `index * step`.
#[cfg(target_has_atomic = "ptr")]
#[inline]
fn stamp_slots<T>(slots: &mut [MaybeUninit<Slot<T>>], capacity: usize, step: usize) {
    for (index, slot) in slots.iter_mut().take(capacity).enumerate() {
        slot.write(Slot::new(index.wrapping_mul(step)));
    }
}

/// `[MaybeUninit<Slot<T>>; N]` with the first `capacity` slots stamped as
/// [`stamp_slots`] does, for `const` constructors.
#[cfg(target_has_atomic = "ptr")]
#[inline]
const fn stamped_array<T, const N: usize>(
    capacity: usize,
    step: usize,
) -> [MaybeUninit<Slot<T>>; N] {
    let mut slots = [const { MaybeUninit::uninit() }; N];
    let mut index = 0;
    while index < capacity && index < N {
        slots[index] = MaybeUninit::new(Slot::new(index.wrapping_mul(step)));
        index += 1;
    }
    slots
}

/// Spins a little longer after each lost race, so that threads contending
/// for one position spread out instead of colliding again at once. Bounded:
/// at most `2^LIMIT` spins per wait.
#[cfg(target_has_atomic = "ptr")]
struct Backoff {
    step: u32,
}

#[cfg(target_has_atomic = "ptr")]
impl Backoff {
    const LIMIT: u32 = 6;

    #[inline]
    const fn new() -> Self {
        Self { step: 0 }
    }

    #[inline]
    fn wait(&mut self) {
        for _ in 0..1u32 << self.step {
            core::hint::spin_loop();
        }
        if self.step < Self::LIMIT {
            self.step += 1;
        }
    }
}
