//! Where a container keeps its elements.
//!
//! A container such as [`Vector<T, S>`](crate::Vector) takes a storage
//! parameter `S` that decides where its memory lives:
//!
//! - [`Borrowed<'a>`]: memory you provide and the container borrows. This
//!   covers a [`Memory`] block on the stack, a piece of an
//!   [`Arena`](crate::Arena), or any `&mut [MaybeUninit<T>]`.
//! - [`Inline<N>`]: room for `N` elements inside the container itself. No
//!   lifetime, nothing to borrow; the container can be a plain struct field.
//! - [`Dynamic<A>`]: memory taken from an [`Allocator`] as the container
//!   fills up. The only kind that grows.
//!
//! The kind is chosen by what is passed to the container's `new`: a
//! reference borrows, a value is stored inline, `Dynamic(allocator)` grows.
//!
//! ```
//! use vas::{Borrowed, Inline, Memory, Vector};
//!
//! let mut memory = Memory::<f32, 16>::new();
//! let borrowed: Vector<f32, Borrowed<'_>> = Vector::new(&mut memory);
//! let inline: Vector<f32, Inline<16>> = Vector::new(Memory::new());
//! assert_eq!(borrowed.capacity(), inline.capacity());
//! ```

#![allow(unsafe_code)]

use core::alloc::Layout;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;
use core::slice;

use crate::Memory;
use crate::allocator::Allocator;
#[cfg(feature = "alloc")]
use crate::allocator::Global;

/// A kind of storage: decides which buffer holds the elements of a
/// container of `T`.
///
/// Use it as a bound to accept a container over any storage:
///
/// ```
/// use vas::storage::Storage;
/// use vas::{Inline, Vector};
///
/// fn total<S: Storage<u32>>(values: &Vector<u32, S>) -> u32 {
///     values.iter().sum()
/// }
///
/// let mut v = Vector::<u32, Inline<4>>::inline();
/// assert!(v.extend_from_slice(&[1, 2, 3]).is_ok());
/// assert_eq!(total(&v), 6);
/// ```
pub trait Storage<T> {
    /// The buffer holding the elements.
    type Buffer: Buffer<T>;
}

/// A run of slots for values of `T`.
///
/// # Safety
///
/// Containers rely on the following, and implementations must uphold it:
///
/// - `slots` and `slots_mut` return the same slots, in the same order and
///   with the same length, until [`grow`](Self::grow) returns `true`.
/// - What a container writes into a slot stays there until the container
///   overwrites it. Moving the buffer value moves the contents with it.
/// - After `grow(min)` returns `true` there are at least `min` slots, and
///   each slot that existed before holds the same bytes at the same index.
///   When it returns `false` nothing has changed.
/// - `replace(min, transfer)` either returns `false` without calling
///   `transfer` and without changing anything, or calls
///   `transfer(old, new)` exactly once, with all current slots as `old` and
///   at least `min` other slots as `new`. When `transfer` returns `true`
///   so does `replace`, and the buffer's slots are then exactly `new`, as
///   `transfer` left them. When `transfer` returns `false` so does
///   `replace`, and the buffer still has its old slots, untouched.
/// - The slots are not reachable in any other way while the buffer exists.
pub unsafe trait Buffer<T> {
    /// All slots.
    fn slots(&self) -> &[MaybeUninit<T>];

    /// All slots, mutably.
    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>];

    /// `true` when [`grow`](Self::grow) and [`replace`](Self::replace) can
    /// succeed at all. Containers use it to skip work that only matters
    /// for buffers that grow.
    const GROWS: bool = false;

    /// Tries to make room for at least `min` slots, keeping the contents.
    /// Buffers of a fixed size return `false`, which is the default.
    #[inline]
    fn grow(&mut self, min: usize) -> bool {
        let _ = min;
        false
    }

    /// Tries to switch to a different run of at least `min` slots. The
    /// container moves its contents across in `transfer(old, new)` and
    /// returns `true`; the old slots are then given up without being read
    /// again. Returning `false` instead cancels the switch. Used by
    /// containers whose layout depends on the capacity, such as hash maps.
    /// Buffers of a fixed size return `false`, which is the default.
    #[inline]
    fn replace(
        &mut self,
        min: usize,
        transfer: impl FnOnce(&[MaybeUninit<T>], &mut [MaybeUninit<T>]) -> bool,
    ) -> bool {
        let _ = (min, transfer);
        false
    }
}

/// Something a container can be built on. Says which kind of [`Storage`]
/// the result uses.
///
/// | Passed to `new` | Storage |
/// |---|---|
/// | `&mut [MaybeUninit<T>]`, `&mut [MaybeUninit<T>; N]`, `&mut Memory<T, N>` | [`Borrowed`] |
/// | `Memory<T, N>`, `[MaybeUninit<T>; N]` | [`Inline<N>`] |
/// | `Dynamic(allocator)` | [`Dynamic<A>`] |
pub trait IntoBuffer<T> {
    /// The kind of storage this turns into.
    type Storage: Storage<T>;

    /// Converts into the buffer of that storage.
    fn into_buffer(self) -> <Self::Storage as Storage<T>>::Buffer;
}

/// Storage borrowed for `'a` from the caller: a [`Memory`] block, a piece of
/// an [`Arena`](crate::Arena), or any `&mut [MaybeUninit<T>]`.
///
/// With `Borrowed<'static>` (memory from a
/// [`StaticBlock`](crate::arena::StaticBlock)) a container needs no lifetime
/// parameter.
pub struct Borrowed<'a>(PhantomData<&'a mut ()>);

/// Storage for `N` elements inside the container itself.
///
/// The container owns its memory, so it has no lifetime and can be a plain
/// struct field or a `static`. Moving the container copies all `N` slots,
/// so keep large ones in [`Borrowed`] storage instead.
pub struct Inline<const N: usize>;

/// Storage that grows: memory is taken from the allocator `A` when the
/// container runs out of room, and given back when it is dropped.
///
/// A container starts without any memory; inserting never fails for lack
/// of room unless the allocator refuses. Growing moves the elements, so it
/// happens in steps (the capacity at least doubles each time).
///
/// Pass `Dynamic(allocator)` to a container's `new`. Use `&allocator` to
/// share one allocator between containers.
///
/// ```
/// use vas::{Arena, Dynamic, Memory, Vector};
///
/// let mut block = Memory::<u8, 1024>::new();
/// let arena = Arena::new(&mut block);
/// let mut v = Vector::new(Dynamic(&arena));
/// for i in 0..100u32 {
///     assert!(v.push(i).is_ok());
/// }
/// ```
///
/// With the `alloc` feature the allocator defaults to
/// [`Global`], so `Vector<f32, Dynamic>` grows on the heap.
#[cfg(feature = "alloc")]
#[derive(Clone, Copy, Default, Debug)]
pub struct Dynamic<A = Global>(pub A);

/// Storage that grows: memory is taken from the allocator `A` when the
/// container runs out of room, and given back when it is dropped.
///
/// A container starts without any memory; inserting never fails for lack
/// of room unless the allocator refuses. Growing moves the elements, so it
/// happens in steps (the capacity at least doubles each time).
///
/// Pass `Dynamic(allocator)` to a container's `new`. Use `&allocator` to
/// share one allocator between containers.
///
/// ```
/// use vas::{Arena, Dynamic, Memory, Vector};
///
/// let mut block = Memory::<u8, 1024>::new();
/// let arena = Arena::new(&mut block);
/// let mut v = Vector::new(Dynamic(&arena));
/// for i in 0..100u32 {
///     assert!(v.push(i).is_ok());
/// }
/// ```
///
/// With the `alloc` feature the allocator defaults to the global one, so
/// `Vector<f32, Dynamic>` grows on the heap.
#[cfg(not(feature = "alloc"))]
#[derive(Clone, Copy, Default, Debug)]
pub struct Dynamic<A>(pub A);

impl<T, A: Allocator> Storage<T> for Dynamic<A> {
    type Buffer = Allocation<T, A>;
}

impl<'a, T> Storage<T> for Borrowed<'a> {
    type Buffer = Slots<'a, T>;
}

impl<T, const N: usize> Storage<T> for Inline<N> {
    type Buffer = [MaybeUninit<T>; N];
}

/// The buffer of [`Borrowed`] storage: a `&'a mut [MaybeUninit<T>]`.
///
/// It is kept as a pointer and a length so that naming the type does not
/// require `T: 'a`; generic code over `Vector<T, Borrowed<'a>>` then needs
/// no extra bounds.
pub struct Slots<'a, T> {
    // Invariant: `start..start + len` is a slice of `MaybeUninit<T>`
    // exclusively borrowed for `'a`.
    start: NonNull<MaybeUninit<T>>,
    len: usize,
    marker: PhantomData<&'a mut ()>,
}

// SAFETY: `Slots` is an exclusive borrow of a slice of `T`, so it is `Send`
// and `Sync` exactly when `&mut [T]` is.
unsafe impl<T: Send> Send for Slots<'_, T> {}
// SAFETY: as above.
unsafe impl<T: Sync> Sync for Slots<'_, T> {}

impl<'a, T> Slots<'a, T> {
    #[inline]
    fn new(slots: &'a mut [MaybeUninit<T>]) -> Self {
        Self {
            len: slots.len(),
            start: NonNull::from(slots).cast(),
            marker: PhantomData,
        }
    }
}

// SAFETY: the slots are one exclusively borrowed slice, returned unchanged
// by both methods.
unsafe impl<T> Buffer<T> for Slots<'_, T> {
    #[inline]
    fn slots(&self) -> &[MaybeUninit<T>] {
        // SAFETY: by the invariant this is a live slice, and `&self` keeps
        // it from being written through this buffer meanwhile.
        unsafe { slice::from_raw_parts(self.start.as_ptr(), self.len) }
    }

    #[inline]
    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>] {
        // SAFETY: by the invariant this is a live, exclusively borrowed
        // slice, and `&mut self` makes this the only access to it.
        unsafe { slice::from_raw_parts_mut(self.start.as_ptr(), self.len) }
    }
}

/// The buffer of [`Dynamic`] storage: a block of slots taken from the
/// allocator `A`, replaced by a bigger one on demand and freed on drop.
pub struct Allocation<T, A: Allocator> {
    // Invariant: `start..start + len` is a slice of `MaybeUninit<T>`. When
    // `len > 0` and `T` is not zero-sized it is a block from `allocator`
    // with the layout of `[T; len]`, owned by this value. Otherwise `start`
    // is dangling and nothing is allocated.
    start: NonNull<MaybeUninit<T>>,
    len: usize,
    allocator: A,
}

// SAFETY: `Allocation` owns its slots like a `Box<[MaybeUninit<T>]>` and
// its allocator, so it can move to another thread when both can.
unsafe impl<T: Send, A: Allocator + Send> Send for Allocation<T, A> {}
// SAFETY: shared access only reads the slots and the allocator.
unsafe impl<T: Sync, A: Allocator + Sync> Sync for Allocation<T, A> {}

impl<T, A: Allocator> Allocation<T, A> {
    /// Slots allocated the first time the buffer grows, unless more are
    /// asked for.
    const FIRST: usize = if size_of::<T>() <= 1024 { 4 } else { 1 };

    /// Creates an empty buffer; nothing is allocated until it grows.
    #[inline]
    pub const fn new(allocator: A) -> Self {
        Self {
            start: NonNull::dangling(),
            len: 0,
            allocator,
        }
    }

    /// Replaces the block by one of exactly `len` slots, keeping the
    /// contents. `len` is larger than the current length.
    #[cold]
    fn grow_to(&mut self, len: usize) -> bool {
        if size_of::<T>() == 0 {
            // Slots of zero bytes need no memory.
            self.len = len;
            return true;
        }
        let Ok(new) = Layout::array::<T>(len) else {
            return false;
        };
        let block = if self.len == 0 {
            self.allocator.allocate(new)
        } else {
            let Ok(old) = Layout::array::<T>(self.len) else {
                return false;
            };
            // SAFETY: by the invariant the block came from this allocator
            // with the layout `old`. `new` has the same alignment and is
            // larger, as `len > self.len`.
            unsafe { self.allocator.grow(self.start.cast(), old, new) }
        };
        match block {
            Ok(block) => {
                self.start = block.cast();
                self.len = len;
                true
            }
            Err(_) => false,
        }
    }
}

impl<T, A: Allocator> Drop for Allocation<T, A> {
    #[inline]
    fn drop(&mut self) {
        if self.len == 0 || size_of::<T>() == 0 {
            return;
        }
        if let Ok(layout) = Layout::array::<T>(self.len) {
            // SAFETY: by the invariant the block came from this allocator
            // with this layout, and it is not used again.
            unsafe { self.allocator.free(self.start.cast(), layout) }
        }
    }
}

// SAFETY: the slots are one block owned by this value, returned unchanged
// by both methods. `grow` succeeds only through `grow_to`, which keeps the
// old slots' bytes (the allocator's `grow` contract) and sets the length
// to at least `min`; on failure nothing is changed. `replace` hands
// `transfer` the current block and a new one of at least `min` slots, then
// adopts the new block.
unsafe impl<T, A: Allocator> Buffer<T> for Allocation<T, A> {
    const GROWS: bool = true;

    #[inline]
    fn slots(&self) -> &[MaybeUninit<T>] {
        // SAFETY: by the invariant this is a live slice (dangling and empty,
        // or zero-sized slots, before anything is allocated).
        unsafe { slice::from_raw_parts(self.start.as_ptr(), self.len) }
    }

    #[inline]
    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>] {
        // SAFETY: as in `slots`, and `&mut self` makes this the only access.
        unsafe { slice::from_raw_parts_mut(self.start.as_ptr(), self.len) }
    }

    #[inline]
    fn grow(&mut self, min: usize) -> bool {
        if min <= self.len {
            return true;
        }
        // At least double, so that a run of pushes copies each element a
        // constant number of times on average.
        let doubled = self.len.saturating_mul(2).max(Self::FIRST);
        // When there is no memory for the doubled size, settle for `min`.
        self.grow_to(min.max(doubled)) || (min < doubled && self.grow_to(min))
    }

    fn replace(
        &mut self,
        min: usize,
        transfer: impl FnOnce(&[MaybeUninit<T>], &mut [MaybeUninit<T>]) -> bool,
    ) -> bool {
        if size_of::<T>() == 0 {
            return false;
        }
        let doubled = self.len.saturating_mul(2).max(Self::FIRST);
        // As in `grow`: the doubled size first, then exactly `min`.
        let block = [min.max(doubled), min].into_iter().find_map(|len| {
            let layout = Layout::array::<T>(len).ok()?;
            let block = self.allocator.allocate(layout).ok()?;
            Some((block.cast::<MaybeUninit<T>>(), len))
        });
        let Some((start, len)) = block else {
            return false;
        };
        // SAFETY: the allocator returned a block for `len` slots that
        // overlaps no other live block, in particular not the current one.
        // `MaybeUninit` slots need no initialisation.
        let new = unsafe { slice::from_raw_parts_mut(start.as_ptr(), len) };
        // Should `transfer` panic, the new block is leaked and the buffer
        // still holds the old one, untouched.
        if !transfer(self.slots(), new) {
            // Cancelled: give the new block back and keep the old one.
            drop(Allocation::<T, &A> {
                start,
                len,
                allocator: &self.allocator,
            });
            return false;
        }
        let old = Allocation::<T, &A> {
            start: core::mem::replace(&mut self.start, start),
            len: core::mem::replace(&mut self.len, len),
            allocator: &self.allocator,
        };
        // Frees the old block through the same allocator.
        drop(old);
        true
    }
}

// SAFETY: an array owns its slots; both methods return all of them.
unsafe impl<T, const N: usize> Buffer<T> for [MaybeUninit<T>; N] {
    #[inline]
    fn slots(&self) -> &[MaybeUninit<T>] {
        self
    }

    #[inline]
    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>] {
        self
    }
}

impl<'a, T> IntoBuffer<T> for &'a mut [MaybeUninit<T>] {
    type Storage = Borrowed<'a>;

    #[inline]
    fn into_buffer(self) -> Slots<'a, T> {
        Slots::new(self)
    }
}

impl<'a, T, const N: usize> IntoBuffer<T> for &'a mut [MaybeUninit<T>; N] {
    type Storage = Borrowed<'a>;

    #[inline]
    fn into_buffer(self) -> Slots<'a, T> {
        Slots::new(self)
    }
}

impl<'a, T, const N: usize> IntoBuffer<T> for &'a mut Memory<T, N> {
    type Storage = Borrowed<'a>;

    #[inline]
    fn into_buffer(self) -> Slots<'a, T> {
        Slots::new(self)
    }
}

impl<T, A: Allocator> IntoBuffer<T> for Dynamic<A> {
    type Storage = Dynamic<A>;

    #[inline]
    fn into_buffer(self) -> Allocation<T, A> {
        Allocation::new(self.0)
    }
}

impl<T, const N: usize> IntoBuffer<T> for [MaybeUninit<T>; N] {
    type Storage = Inline<N>;

    #[inline]
    fn into_buffer(self) -> Self {
        self
    }
}

impl<T, const N: usize> IntoBuffer<T> for Memory<T, N> {
    type Storage = Inline<N>;

    #[inline]
    fn into_buffer(self) -> [MaybeUninit<T>; N] {
        self.into_slots()
    }
}

/// With the `alloc` feature, a `Vec` of slots can be borrowed directly; its
/// current length is the capacity.
#[cfg(feature = "alloc")]
impl<'a, T> IntoBuffer<T> for &'a mut alloc::vec::Vec<MaybeUninit<T>> {
    type Storage = Borrowed<'a>;

    #[inline]
    fn into_buffer(self) -> Slots<'a, T> {
        Slots::new(self)
    }
}

/// With the `alloc` feature, a boxed slice of slots can be borrowed
/// directly.
#[cfg(feature = "alloc")]
impl<'a, T> IntoBuffer<T> for &'a mut alloc::boxed::Box<[MaybeUninit<T>]> {
    type Storage = Borrowed<'a>;

    #[inline]
    fn into_buffer(self) -> Slots<'a, T> {
        Slots::new(self)
    }
}
