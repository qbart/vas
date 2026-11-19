//! A value on a cache line of its own.

use core::fmt;
use core::ops::{Deref, DerefMut};

/// Keeps `T` on a cache line of its own, so that threads writing different
/// values do not slow each other down by sharing one line ("false
/// sharing").
///
/// Aligned to 128 bytes on x86_64, aarch64 and powerpc64, whose CPUs fetch
/// cache lines in pairs, and to 64 bytes elsewhere. It derefs to `T`.
///
/// ```
/// use core::sync::atomic::{AtomicUsize, Ordering};
/// use vas::sync::CachePadded;
///
/// struct Counters {
///     produced: CachePadded<AtomicUsize>, // written by one thread
///     consumed: CachePadded<AtomicUsize>, // written by another
/// }
///
/// let counters = Counters {
///     produced: CachePadded::new(AtomicUsize::new(0)),
///     consumed: CachePadded::new(AtomicUsize::new(0)),
/// };
/// counters.produced.fetch_add(1, Ordering::Relaxed);
/// assert_eq!(counters.produced.load(Ordering::Relaxed), 1);
/// ```
#[cfg_attr(
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "powerpc64"
    ),
    repr(align(128))
)]
#[cfg_attr(
    not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "powerpc64"
    )),
    repr(align(64))
)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CachePadded<T> {
    value: T,
}

impl<T> CachePadded<T> {
    /// Pads `value`.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    /// The value, without the padding.
    #[inline]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T> Deref for CachePadded<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for CachePadded<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T> From<T> for CachePadded<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: fmt::Debug> fmt::Debug for CachePadded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CachePadded").field(&self.value).finish()
    }
}
