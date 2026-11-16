//! Inline, uninitialised blocks of memory to back containers, and units for
//! writing sizes.

use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};

/// `count` kilobytes in bytes (1 KB = 1024 bytes).
///
/// Sizes in vas are plain `usize` byte counts; write them with [`KB`],
/// [`MB`] and [`GB`]. They are `const`, so they also work where a constant
/// is required:
///
/// ```
/// use vas::arena::StaticBlock;
/// use vas::{KB, MB};
///
/// static GAME_MEMORY: StaticBlock<{ MB(20) }> = StaticBlock::new();
/// assert_eq!(GAME_MEMORY.capacity(), KB(20 * 1024));
/// ```
///
/// A size too large for a `usize` becomes `usize::MAX`, which no block can
/// satisfy, so it is reported as [`Full`](crate::Full) rather than wrapping
/// to a small size.
// Named like the unit, so that sizes read as `MB(20)`.
#[allow(non_snake_case)]
#[inline]
pub const fn KB(count: usize) -> usize {
    count.saturating_mul(1 << 10)
}

/// `count` megabytes in bytes (1 MB = 1024 KB). See [`KB`].
#[cfg(not(target_pointer_width = "16"))]
#[allow(non_snake_case)]
#[inline]
pub const fn MB(count: usize) -> usize {
    count.saturating_mul(1 << 20)
}

/// `count` gigabytes in bytes (1 GB = 1024 MB). See [`KB`].
#[cfg(not(target_pointer_width = "16"))]
#[allow(non_snake_case)]
#[inline]
pub const fn GB(count: usize) -> usize {
    count.saturating_mul(1 << 30)
}

/// An uninitialised block with room for `N` values of `T`, stored inline
/// (on the stack, in a `static`, or inside another struct).
///
/// It is a convenience for `[MaybeUninit<T>; N]`: creating it costs nothing
/// and it holds no values, so dropping it drops nothing. Containers borrow it
/// as their storage.
///
/// ```
/// use vas::{Memory, Vector};
///
/// let mut memory = Memory::<f32, 1024>::new();
/// let mut v = Vector::new(&mut memory);
/// assert!(v.push(1.0).is_ok());
/// ```
#[repr(transparent)]
pub struct Memory<T, const N: usize> {
    slots: [MaybeUninit<T>; N],
}

impl<T, const N: usize> Memory<T, N> {
    /// Creates the block. Nothing is initialised.
    #[inline]
    pub const fn new() -> Self {
        Self {
            slots: [const { MaybeUninit::uninit() }; N],
        }
    }

    /// Number of values the block has room for.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }
}

impl<T, const N: usize> Default for Memory<T, N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Deref for Memory<T, N> {
    type Target = [MaybeUninit<T>];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.slots
    }
}

impl<T, const N: usize> DerefMut for Memory<T, N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.slots
    }
}
