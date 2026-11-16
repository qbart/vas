//! Where growing containers get their memory.
//!
//! A container with [`Dynamic<A>`](crate::Dynamic) storage asks an
//! [`Allocator`] for memory when it runs out of room. The allocator is
//! yours: implement the trait on top of whatever hands out memory in your
//! game (a frame allocator, a platform call, a tracked heap).
//!
//! Ready-made allocators:
//!
//! - [`Arena`](crate::Arena): bump allocation from one block.
//! - [`Global`] (with the `alloc` feature): the program's global allocator.
//! - `&A` for any allocator `A`, so several containers can share one.

#![allow(unsafe_code)]

use core::alloc::Layout;
use core::ptr::{self, NonNull};

use crate::Full;

/// A source of memory for containers with [`Dynamic`](crate::Dynamic)
/// storage.
///
/// Methods take `&self` so that one allocator can serve many containers;
/// keep state that changes in a `Cell` or an atomic.
///
/// ```
/// use core::alloc::Layout;
/// use core::ptr::NonNull;
/// use vas::allocator::Allocator;
/// use vas::{Arena, Dynamic, Full, Memory, Vector};
///
/// /// Hands out memory from an arena, but never more than `limit` bytes
/// /// per request.
/// struct Limited<'a> {
///     arena: Arena<'a>,
///     limit: usize,
/// }
///
/// // SAFETY: blocks come from the arena, which upholds the contract.
/// unsafe impl Allocator for Limited<'_> {
///     fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Full<()>> {
///         if layout.size() > self.limit {
///             return Err(Full(()));
///         }
///         self.arena.allocate(layout)
///     }
///
///     unsafe fn free(&self, block: NonNull<u8>, layout: Layout) {
///         // SAFETY: forwarded unchanged.
///         unsafe { self.arena.free(block, layout) }
///     }
/// }
///
/// let mut block = Memory::<u8, 1024>::new();
/// let allocator = Limited { arena: Arena::new(&mut block), limit: 64 };
/// let mut v = Vector::new(Dynamic(&allocator));
/// while v.push(0u8).is_ok() {}
/// assert!(v.len() >= 32 && v.len() <= 64);
/// ```
///
/// # Safety
///
/// Containers rely on the following, and implementations must uphold it:
///
/// - A block returned by [`allocate`](Self::allocate) or
///   [`grow`](Self::grow) is valid for reads and writes of `layout.size()`
///   bytes, aligned to `layout.align()`, and overlaps no other live block.
/// - The block stays valid, at the same address and with its contents
///   untouched, until it is passed to [`free`](Self::free) or successfully
///   to [`grow`](Self::grow). Moving the allocator value does not
///   invalidate it.
/// - A failed `grow` leaves the old block as it was.
pub unsafe trait Allocator {
    /// Hands out a block for `layout`, or fails when there is no memory.
    ///
    /// A request of zero bytes may return any non-null address aligned to
    /// `layout.align()`.
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Full<()>>;

    /// Takes a block back.
    ///
    /// # Safety
    ///
    /// `block` must come from `allocate` or `grow` of this allocator with
    /// exactly this `layout`, and must not be used afterwards.
    unsafe fn free(&self, block: NonNull<u8>, layout: Layout);

    /// Makes a block bigger, keeping its first `old.size()` bytes. Returns
    /// the new address, which may differ from the old one; the old block is
    /// then gone. On failure the old block is untouched and still owned by
    /// the caller.
    ///
    /// The default allocates a new block, copies and frees the old one.
    /// Override it when the allocator can extend a block in place.
    ///
    /// # Safety
    ///
    /// `block` must come from `allocate` or `grow` of this allocator with
    /// the layout `old`. `new` must have the same alignment as `old` and be
    /// at least as large.
    unsafe fn grow(
        &self,
        block: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Result<NonNull<u8>, Full<()>> {
        let grown = self.allocate(new)?;
        // SAFETY: both blocks are valid for `old.size()` bytes (the new one
        // is at least as large) and do not overlap, by the trait contract.
        // The old block is not used after it is freed.
        unsafe {
            ptr::copy_nonoverlapping(block.as_ptr(), grown.as_ptr(), old.size());
            self.free(block, old);
        }
        Ok(grown)
    }
}

// SAFETY: forwards every call unchanged to `A`, which upholds the contract.
unsafe impl<A: Allocator + ?Sized> Allocator for &A {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Full<()>> {
        (**self).allocate(layout)
    }

    #[inline]
    unsafe fn free(&self, block: NonNull<u8>, layout: Layout) {
        // SAFETY: the caller upholds the contract of `A::free`.
        unsafe { (**self).free(block, layout) }
    }

    #[inline]
    unsafe fn grow(
        &self,
        block: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Result<NonNull<u8>, Full<()>> {
        // SAFETY: the caller upholds the contract of `A::grow`.
        unsafe { (**self).grow(block, old, new) }
    }
}

/// The program's global allocator (`#[global_allocator]`, by default the
/// system heap). Available with the `alloc` feature.
///
/// It is the default allocator of [`Dynamic`](crate::Dynamic), so
/// `Vector<f32, Dynamic>` grows on the heap.
///
/// ```
/// use vas::{Dynamic, Vector};
///
/// let mut v = Vector::<u32, Dynamic>::default();
/// for i in 0..10_000 {
///     assert!(v.push(i).is_ok());
/// }
/// assert_eq!(v.len(), 10_000);
/// ```
#[cfg(feature = "alloc")]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Global;

/// An address aligned for `layout` that is never read or written; stands in
/// for blocks of zero bytes.
#[cfg(feature = "alloc")]
#[inline]
fn dangling(layout: Layout) -> NonNull<u8> {
    NonNull::<u8>::dangling().with_addr(
        core::num::NonZeroUsize::new(layout.align()).unwrap_or(core::num::NonZeroUsize::MIN),
    )
}

// SAFETY: blocks come from the global allocator, which hands out valid,
// non-overlapping blocks that stay put until freed or reallocated. Requests
// of zero bytes never reach it.
#[cfg(feature = "alloc")]
unsafe impl Allocator for Global {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Full<()>> {
        if layout.size() == 0 {
            return Ok(dangling(layout));
        }
        // SAFETY: the layout has a non-zero size.
        NonNull::new(unsafe { alloc::alloc::alloc(layout) }).ok_or(Full(()))
    }

    #[inline]
    unsafe fn free(&self, block: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            // SAFETY: by the caller's contract the block came from this
            // allocator with this layout; it is not zero-sized, so it came
            // from `alloc` or `realloc`.
            unsafe { alloc::alloc::dealloc(block.as_ptr(), layout) }
        }
    }

    #[inline]
    unsafe fn grow(
        &self,
        block: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Result<NonNull<u8>, Full<()>> {
        if old.size() == 0 {
            return self.allocate(new);
        }
        // SAFETY: the block came from this allocator with layout `old`,
        // which is not zero-sized. `new` has the same alignment and a size
        // that is non-zero and, being a valid layout, does not overflow
        // when rounded up to the alignment.
        NonNull::new(unsafe { alloc::alloc::realloc(block.as_ptr(), old, new.size()) })
            .ok_or(Full(()))
    }
}
