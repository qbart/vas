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
//!
//! The kind is chosen by what is passed to the container's `new`: a
//! reference borrows, a value is stored inline.
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

use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr::NonNull;
use core::slice;

use crate::Memory;

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

/// A fixed run of slots for values of `T`.
///
/// # Safety
///
/// Containers rely on the following, and implementations must uphold it:
///
/// - `slots` and `slots_mut` always return the same slots, in the same
///   order and with the same length, for as long as the buffer exists.
/// - What a container writes into a slot stays there until the container
///   overwrites it. Moving the buffer value moves the contents with it.
/// - The slots are not reachable in any other way while the buffer exists.
pub unsafe trait Buffer<T> {
    /// All slots.
    fn slots(&self) -> &[MaybeUninit<T>];

    /// All slots, mutably.
    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>];
}

/// Something a container can be built on. Says which kind of [`Storage`]
/// the result uses.
///
/// | Passed to `new` | Storage |
/// |---|---|
/// | `&mut [MaybeUninit<T>]`, `&mut [MaybeUninit<T>; N]`, `&mut Memory<T, N>` | [`Borrowed`] |
/// | `Memory<T, N>`, `[MaybeUninit<T>; N]` | [`Inline<N>`] |
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
