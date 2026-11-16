//! Double-ended queue (ring buffer) over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::iter::Chain;
use core::mem::MaybeUninit;
use core::{fmt, ptr, slice};

use crate::Full;
use crate::allocator::Allocator;
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};

/// Iterator over the elements of a [`Deque`], front to back.
pub type Iter<'d, T> = Chain<slice::Iter<'d, T>, slice::Iter<'d, T>>;

/// Mutable iterator over the elements of a [`Deque`], front to back.
pub type IterMut<'d, T> = Chain<slice::IterMut<'d, T>, slice::IterMut<'d, T>>;

/// A fixed-capacity double-ended queue (ring buffer) that lives in memory
/// provided by the caller.
///
/// Pushing and popping at either end is O(1) and never moves other elements.
/// The contents wrap around the end of the block, so they are exposed as up
/// to two slices rather than one.
///
/// ```
/// use vas::{Deque, Memory};
///
/// let mut memory = Memory::<u32, 8>::new();
/// let mut queue = Deque::new(&mut memory);
///
/// assert!(queue.push_back(1).is_ok());
/// assert!(queue.push_back(2).is_ok());
/// assert_eq!(queue.pop_front(), Some(1));
/// ```
///
/// With [`Dynamic`] storage the capacity is not fixed: the
/// memory comes from an allocator and the container grows when it runs out.
pub struct Deque<T, S: Storage<T>> {
    // Invariant: `head < capacity` (or `head == 0` when the capacity is
    // zero), `len <= capacity`, and the `len` slots starting at `head`,
    // wrapping around the end of the storage, are initialised.
    buffer: S::Buffer,
    head: usize,
    len: usize,
}

impl<T, const N: usize> Deque<T, Inline<N>> {
    /// Creates an empty deque with room for `N` values inside itself.
    ///
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            buffer: [const { MaybeUninit::uninit() }; N],
            head: 0,
            len: 0,
        }
    }
}

impl<T, const N: usize> Default for Deque<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<T, A: Allocator + Default> Default for Deque<T, Dynamic<A>> {
    /// Creates an empty deque that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<T, S: Storage<T>> Deque<T, S> {
    /// Creates an empty deque on `buffer`: a reference to memory borrows it,
    /// a `Memory` passed by value is stored inside the deque.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<T, Storage = S>,
    {
        Self {
            buffer: buffer.into_buffer(),
            head: 0,
            len: 0,
        }
    }

    /// Number of elements in the deque.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Number of elements the deque can hold without growing. For borrowed
    /// and inline storage this is the maximum.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buffer.slots().len()
    }

    /// Number of elements that can still be pushed without growing.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.buffer.slots().len() - self.len
    }

    /// `true` when the deque holds no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` when the next push needs more memory: it fails with borrowed
    /// and inline storage, and grows with dynamic storage.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == self.buffer.slots().len()
    }

    /// Makes sure `additional` more elements fit, growing dynamic storage
    /// when needed. Fails without changes when the storage is fixed and too
    /// small, or when the allocator refuses.
    #[inline]
    pub fn reserve(&mut self, additional: usize) -> Result<(), Full<()>> {
        match self.len.checked_add(additional) {
            Some(capacity) if capacity <= self.capacity() || self.grow(capacity) => Ok(()),
            _ => Err(Full(())),
        }
    }

    /// Grows the storage to at least `capacity` slots and puts the contents
    /// back into ring order. Returns `false`, with nothing changed, when
    /// the storage cannot grow.
    #[cold]
    fn grow(&mut self, capacity: usize) -> bool {
        let old = self.buffer.slots().len();
        if !self.buffer.grow(capacity) {
            return false;
        }
        let slots = self.buffer.slots_mut();
        // Elements that sat between the front and the old end of the block.
        let until_end = old - self.head;
        if self.len > until_end {
            // The contents wrapped around the old end. Move the front piece
            // to the new end, so that the block wraps there instead.
            let head = slots.len() - until_end;
            let base = slots.as_mut_ptr();
            // SAFETY: `self.head + until_end == old` and
            // `head + until_end == slots.len()`, so both ranges are inside
            // the block. They may overlap, which `ptr::copy` allows. The
            // slots are moved as raw `MaybeUninit`, so nothing is dropped
            // or duplicated: the old positions are no longer part of the
            // ring once `self.head` is updated.
            unsafe { ptr::copy(base.add(self.head), base.add(head), until_end) };
            self.head = head;
        }
        true
    }

    /// Appends `value` at the back. With dynamic storage the deque grows
    /// when needed.
    ///
    /// Hands the value back in [`Full`] when there is no room: the storage
    /// is fixed and full, or the allocator refused.
    #[inline]
    pub fn push_back(&mut self, value: T) -> Result<(), Full<T>> {
        if self.is_full() && !self.grow(self.len.saturating_add(1)) {
            return Err(Full(value));
        }
        let index = self.physical(self.len);
        match self.buffer.slots_mut().get_mut(index) {
            Some(slot) => {
                slot.write(value);
                self.len += 1;
                Ok(())
            }
            None => Err(Full(value)),
        }
    }

    /// Prepends `value` at the front. With dynamic storage the deque grows
    /// when needed.
    ///
    /// Hands the value back in [`Full`] when there is no room: the storage
    /// is fixed and full, or the allocator refused.
    #[inline]
    pub fn push_front(&mut self, value: T) -> Result<(), Full<T>> {
        if self.is_full() && !self.grow(self.len.saturating_add(1)) {
            return Err(Full(value));
        }
        let head = if self.head == 0 {
            self.buffer.slots().len() - 1
        } else {
            self.head - 1
        };
        match self.buffer.slots_mut().get_mut(head) {
            Some(slot) => {
                slot.write(value);
                self.head = head;
                self.len += 1;
                Ok(())
            }
            None => Err(Full(value)),
        }
    }

    /// Appends `value` at the back. When the deque is full, the front
    /// element is removed to make room and returned.
    ///
    /// This turns the deque into a "keep the last N" history buffer. With a
    /// capacity of zero the value itself is returned.
    ///
    /// It never grows the deque, also with dynamic storage: `N` is the
    /// current capacity (see [`reserve`](Self::reserve)).
    #[inline]
    pub fn push_overwrite(&mut self, value: T) -> Option<T> {
        let evicted = if self.is_full() {
            match self.pop_front() {
                Some(front) => Some(front),
                None => return Some(value),
            }
        } else {
            None
        };
        match self.push_back(value) {
            Ok(()) => evicted,
            Err(Full(value)) => Some(value),
        }
    }

    /// Removes and returns the front element, or `None` when empty.
    #[inline]
    pub fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let slot = self.buffer.slots().get(self.head)?;
        // SAFETY: the deque is not empty, so the slot at `head` is
        // initialised. `head` and `len` are updated right after, so the
        // value is moved out exactly once.
        let value = unsafe { slot.assume_init_read() };
        self.head = self.physical(1);
        self.len -= 1;
        Some(value)
    }

    /// Removes and returns the back element, or `None` when empty.
    #[inline]
    pub fn pop_back(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let slot = self.buffer.slots().get(self.physical(self.len - 1))?;
        // SAFETY: the deque is not empty, so its last slot is initialised.
        // `len` is decremented right after, so the value is moved out
        // exactly once.
        let value = unsafe { slot.assume_init_read() };
        self.len -= 1;
        Some(value)
    }

    /// The element at `index`, counted from the front.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        let slot = self.buffer.slots().get(self.physical(index))?;
        // SAFETY: `index < len`, so the slot is initialised.
        Some(unsafe { slot.assume_init_ref() })
    }

    /// The element at `index`, counted from the front, mutably.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        let index = self.physical(index);
        let slot = self.buffer.slots_mut().get_mut(index)?;
        // SAFETY: the logical index was below `len`, so the slot is
        // initialised.
        Some(unsafe { slot.assume_init_mut() })
    }

    /// The front element.
    #[inline]
    pub fn front(&self) -> Option<&T> {
        self.get(0)
    }

    /// The front element, mutably.
    #[inline]
    pub fn front_mut(&mut self) -> Option<&mut T> {
        self.get_mut(0)
    }

    /// The back element.
    #[inline]
    pub fn back(&self) -> Option<&T> {
        self.get(self.len.checked_sub(1)?)
    }

    /// The back element, mutably.
    #[inline]
    pub fn back_mut(&mut self) -> Option<&mut T> {
        self.get_mut(self.len.checked_sub(1)?)
    }

    /// The contents, front to back, as two slices. The second slice is empty
    /// unless the contents wrap around the end of the block.
    #[inline]
    pub fn as_slices(&self) -> (&[T], &[T]) {
        let (first_len, second_len) = self.split_lens();
        let (wrapped, from_head) = match self.buffer.slots().split_at_checked(self.head) {
            Some(parts) => parts,
            None => (&[][..], &[][..]),
        };
        let first = from_head.get(..first_len).unwrap_or_default();
        let second = wrapped.get(..second_len).unwrap_or_default();
        // SAFETY: both ranges lie inside the initialised region described by
        // the invariant, and `MaybeUninit<T>` has the layout of `T`.
        unsafe {
            (
                slice::from_raw_parts(first.as_ptr().cast::<T>(), first.len()),
                slice::from_raw_parts(second.as_ptr().cast::<T>(), second.len()),
            )
        }
    }

    /// The contents, front to back, as two mutable slices. The second slice
    /// is empty unless the contents wrap around the end of the block.
    #[inline]
    pub fn as_mut_slices(&mut self) -> (&mut [T], &mut [T]) {
        let (first_len, second_len) = self.split_lens();
        let (wrapped, from_head) = match self.buffer.slots_mut().split_at_mut_checked(self.head) {
            Some(parts) => parts,
            None => (&mut [][..], &mut [][..]),
        };
        let first = from_head.get_mut(..first_len).unwrap_or_default();
        let second = wrapped.get_mut(..second_len).unwrap_or_default();
        // SAFETY: as in `as_slices`; the two ranges are disjoint and `self`
        // is borrowed exclusively.
        unsafe {
            (
                slice::from_raw_parts_mut(first.as_mut_ptr().cast::<T>(), first.len()),
                slice::from_raw_parts_mut(second.as_mut_ptr().cast::<T>(), second.len()),
            )
        }
    }

    /// Iterates over the elements, front to back.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        let (first, second) = self.as_slices();
        first.iter().chain(second.iter())
    }

    /// Iterates mutably over the elements, front to back.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let (first, second) = self.as_mut_slices();
        first.iter_mut().chain(second.iter_mut())
    }

    /// Removes and drops all elements.
    pub fn clear(&mut self) {
        let (first, second) = self.as_mut_slices();
        let first: *mut [T] = first;
        let second: *mut [T] = second;
        // Empty the deque first so a panicking `Drop` cannot cause a double
        // drop.
        self.head = 0;
        self.len = 0;
        // SAFETY: the two slices covered exactly the initialised elements,
        // which are no longer reachable through `self`, so each is dropped
        // exactly once.
        unsafe {
            ptr::drop_in_place(first);
            ptr::drop_in_place(second);
        }
    }

    /// Storage index of the element `offset` places after the front.
    /// Requires `offset <= capacity`; never overflows.
    #[inline]
    fn physical(&self, offset: usize) -> usize {
        let until_end = self.buffer.slots().len() - self.head;
        if offset < until_end {
            self.head + offset
        } else {
            offset - until_end
        }
    }

    /// Lengths of the piece before the end of the block and the wrapped
    /// piece at its start.
    #[inline]
    fn split_lens(&self) -> (usize, usize) {
        let until_end = self.buffer.slots().len() - self.head;
        if self.len <= until_end {
            (self.len, 0)
        } else {
            (until_end, self.len - until_end)
        }
    }
}

impl<T, S: Storage<T>> Drop for Deque<T, S> {
    #[inline]
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T: fmt::Debug, S: Storage<T>> fmt::Debug for Deque<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'d, T, S: Storage<T>> IntoIterator for &'d Deque<T, S> {
    type Item = &'d T;
    type IntoIter = Iter<'d, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'d, T, S: Storage<T>> IntoIterator for &'d mut Deque<T, S> {
    type Item = &'d mut T;
    type IntoIter = IterMut<'d, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
