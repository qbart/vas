//! Fixed-capacity vector over user-provided memory.

#![allow(unsafe_code)]

use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::{fmt, ptr, slice};

use crate::{Full, InsertError};

/// A fixed-capacity, contiguous vector that lives in memory provided by the
/// caller.
///
/// The capacity is the length of the storage slice and never changes. The
/// contents are always one contiguous block, available through
/// [`as_slice`](Self::as_slice).
///
/// ```
/// use core::mem::MaybeUninit;
/// use vas::Vector;
///
/// let mut memory = [MaybeUninit::<u32>::uninit(); 4];
/// let mut v = Vector::new(&mut memory);
/// assert!(v.push(1).is_ok());
/// assert!(v.push(2).is_ok());
/// assert_eq!(v.as_slice(), &[1, 2]);
/// ```
pub struct Vector<'a, T> {
    // Invariant: `len <= storage.len()` and `storage[..len]` is initialised.
    storage: &'a mut [MaybeUninit<T>],
    len: usize,
}

impl<'a, T> Vector<'a, T> {
    /// Creates an empty vector using `storage` as its memory.
    #[inline]
    pub const fn new(storage: &'a mut [MaybeUninit<T>]) -> Self {
        Self { storage, len: 0 }
    }

    /// Number of elements in the vector.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Maximum number of elements the vector can hold.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.storage.len()
    }

    /// Number of elements that can still be pushed.
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.storage.len() - self.len
    }

    /// `true` when the vector holds no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` when no more elements can be pushed.
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.len == self.storage.len()
    }

    /// Appends `value`, or hands it back in [`Full`] when there is no room.
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), Full<T>> {
        match self.storage.get_mut(self.len) {
            Some(slot) => {
                slot.write(value);
                self.len += 1;
                Ok(())
            }
            None => Err(Full(value)),
        }
    }

    /// Removes and returns the last element, or `None` when empty.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: the slot at the old `len - 1` is in bounds and initialised
        // by the invariant; `len` was decremented first, so the value is
        // moved out exactly once.
        Some(unsafe { self.storage.get_unchecked(self.len).assume_init_read() })
    }

    /// Appends all of `values` with a single copy, or nothing when they do
    /// not fit.
    #[inline]
    pub fn extend_from_slice(&mut self, values: &[T]) -> Result<(), Full<()>>
    where
        T: Copy,
    {
        if values.len() > self.remaining() {
            return Err(Full(()));
        }
        // SAFETY: `len + values.len() <= capacity`, so the destination range
        // is inside the storage. `values` is a shared borrow and the storage
        // is exclusively borrowed, so the ranges cannot overlap.
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                self.as_mut_ptr().add(self.len),
                values.len(),
            );
        }
        self.len += values.len();
        Ok(())
    }

    /// Sets the length to `len`. New slots are filled with clones of `value`;
    /// surplus elements are dropped. Fails without changes when `len` exceeds
    /// the capacity.
    #[inline]
    pub fn resize(&mut self, len: usize, value: T) -> Result<(), Full<()>>
    where
        T: Clone,
    {
        self.resize_with(len, || value.clone())
    }

    /// Sets the length to `len`. New slots are filled by calling `fill` once
    /// per slot; surplus elements are dropped. Fails without changes when
    /// `len` exceeds the capacity.
    #[inline]
    pub fn resize_with(&mut self, len: usize, mut fill: impl FnMut() -> T) -> Result<(), Full<()>> {
        if len <= self.len {
            self.truncate(len);
            return Ok(());
        }
        let Some(slots) = self.storage.get_mut(self.len..len) else {
            return Err(Full(()));
        };
        for slot in slots {
            slot.write(fill());
            // Count each element as soon as it exists, so a panicking `fill`
            // leaves the vector consistent.
            self.len += 1;
        }
        Ok(())
    }

    /// Inserts `value` at `index`, shifting later elements up by one. O(n).
    ///
    /// Hands the value back when the vector is full or `index > len`.
    #[inline]
    pub fn insert(&mut self, index: usize, value: T) -> Result<(), InsertError<T>> {
        if index > self.len {
            return Err(InsertError::OutOfBounds(value));
        }
        if self.is_full() {
            return Err(InsertError::Full(value));
        }
        // SAFETY: `index <= len < capacity`, so `index..len + 1` is inside
        // the storage. The tail `index..len` is moved up by one slot
        // (overlapping copy), which leaves slot `index` free to be written
        // without dropping anything.
        unsafe {
            let slot = self.as_mut_ptr().add(index);
            ptr::copy(slot, slot.add(1), self.len - index);
            slot.write(value);
        }
        self.len += 1;
        Ok(())
    }

    /// Removes the element at `index`, shifting later elements down by one
    /// so order is preserved. O(n). Returns `None` when `index` is out of
    /// range.
    #[inline]
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }
        // SAFETY: `index < len`, so the slot is in bounds and initialised.
        // Its value is moved out, then the tail `index + 1..len` is moved
        // down by one slot (overlapping copy) and `len` shrinks, so the
        // duplicated last slot is no longer considered initialised.
        let value = unsafe {
            let slot = self.as_mut_ptr().add(index);
            let value = slot.read();
            ptr::copy(slot.add(1), slot, self.len - index - 1);
            value
        };
        self.len -= 1;
        Some(value)
    }

    /// Moves all elements into `storage` with a single copy and returns a
    /// vector over it, leaving the old storage free. This is how a vector
    /// "grows": get a bigger block, then move into it.
    ///
    /// Hands the vector back unchanged when `storage` is too small.
    #[inline]
    pub fn move_into<'b>(
        mut self,
        storage: &'b mut [MaybeUninit<T>],
    ) -> Result<Vector<'b, T>, Full<Self>> {
        let len = self.len;
        if storage.len() < len {
            return Err(Full(self));
        }
        // SAFETY: the source holds `len` initialised elements and the
        // destination has room for at least `len`. Both are exclusive
        // borrows, so they cannot overlap. Setting `self.len` to zero
        // afterwards transfers ownership of the elements: the old vector
        // drops nothing.
        unsafe {
            ptr::copy_nonoverlapping(self.as_mut_ptr(), storage.as_mut_ptr().cast::<T>(), len);
        }
        self.len = 0;
        Ok(Vector { storage, len })
    }

    /// Removes the element at `index` in O(1) by moving the last element
    /// into its place. Returns `None` when `index` is out of range.
    #[inline]
    pub fn swap_remove(&mut self, index: usize) -> Option<T> {
        if index >= self.len {
            return None;
        }
        let last = self.len - 1;
        let base = self.as_mut_ptr();
        // SAFETY: `index <= last < len`, so both slots are in bounds and
        // initialised. The value at `index` is moved out, then the last
        // element is moved into the hole (a no-op copy when they are the
        // same slot) and `len` shrinks so the last slot is no longer
        // considered initialised.
        let value = unsafe {
            let value = base.add(index).read();
            ptr::copy(base.add(last), base.add(index), 1);
            value
        };
        self.len = last;
        Some(value)
    }

    /// Shortens the vector to `len` elements, dropping the rest. Does nothing
    /// when `len` is not smaller than the current length.
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        if len >= self.len {
            return;
        }
        let count = self.len - len;
        // Shrink first so a panicking `Drop` cannot cause a double drop.
        self.len = len;
        // SAFETY: `len..len + count` was inside the initialised prefix and is
        // no longer reachable through `self`, so each element is dropped
        // exactly once.
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                self.as_mut_ptr().add(len),
                count,
            ));
        }
    }

    /// Removes and drops all elements.
    #[inline]
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// The contents as one contiguous slice.
    #[inline]
    pub const fn as_slice(&self) -> &[T] {
        // SAFETY: the first `len` slots are initialised by the invariant and
        // `MaybeUninit<T>` has the same layout as `T`.
        unsafe { slice::from_raw_parts(self.storage.as_ptr().cast::<T>(), self.len) }
    }

    /// The contents as one contiguous mutable slice.
    #[inline]
    pub const fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: as in `as_slice`, and `self` is borrowed exclusively.
        unsafe { slice::from_raw_parts_mut(self.storage.as_mut_ptr().cast::<T>(), self.len) }
    }

    #[inline]
    const fn as_mut_ptr(&mut self) -> *mut T {
        self.storage.as_mut_ptr().cast::<T>()
    }
}

impl<T> Drop for Vector<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T> Deref for Vector<'_, T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T> DerefMut for Vector<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: fmt::Debug> fmt::Debug for Vector<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}
