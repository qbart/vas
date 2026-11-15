//! Inline, uninitialised blocks of memory to back containers.

use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};

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
