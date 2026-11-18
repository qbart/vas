//! Contiguous vector over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::cmp::Ordering;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::{fmt, ptr, slice};

use crate::allocator::Allocator;
use crate::storage::{Allocation, Buffer, Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, InsertError, sort};

/// A contiguous vector.
///
/// The storage parameter `S` decides where the elements live (see
/// [`storage`](crate::storage)):
///
/// - `Vector<T, Borrowed<'a>>` uses memory provided by the caller.
/// - `Vector<T, Inline<N>>` holds room for `N` elements itself.
/// - `Vector<T, Dynamic<A>>` takes memory from the allocator `A` and grows.
///
/// With borrowed and inline storage the capacity never changes. The
/// contents are always one contiguous block, available through
/// [`as_slice`](Self::as_slice).
///
/// ```
/// use vas::{Inline, Memory, Vector};
///
/// // Borrowed: pass a reference to the memory.
/// let mut memory = Memory::<u32, 4>::new();
/// let mut v = Vector::new(&mut memory);
/// assert!(v.push(1).is_ok());
/// assert!(v.push(2).is_ok());
/// assert_eq!(v.as_slice(), &[1, 2]);
///
/// // Inline: the vector owns its memory, so it can be a struct field.
/// struct Particles {
///     x: Vector<f32, Inline<1024>>,
///     y: Vector<f32, Inline<1024>>,
/// }
/// let particles = Particles { x: Vector::inline(), y: Vector::inline() };
/// assert_eq!(particles.x.capacity() + particles.y.capacity(), 2048);
/// ```
///
/// Growing, here inside an arena:
///
/// ```
/// use vas::{Arena, Dynamic, Memory, Vector};
///
/// let mut block = Memory::<u8, 4096>::new();
/// let arena = Arena::new(&mut block);
/// let mut v = Vector::new(Dynamic(&arena));
/// for i in 0..500u32 {
///     assert!(v.push(i).is_ok());
/// }
/// assert_eq!(v.len(), 500);
/// ```
pub struct Vector<T, S: Storage<T>> {
    // Invariant: `len <= buffer.slots().len()` and the first `len` slots
    // are initialised.
    buffer: S::Buffer,
    len: usize,
}

impl<T, const N: usize> Vector<T, Inline<N>> {
    /// Creates an empty vector with room for `N` elements inside itself.
    ///
    /// Unlike [`new`](Self::new) this is `const`, so it can initialise a
    /// `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            buffer: [const { MaybeUninit::uninit() }; N],
            len: 0,
        }
    }
}

impl<T, const N: usize> Default for Vector<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Clones the elements into a vector of the same capacity. Only vectors
/// with inline storage can be cloned: they are the only ones that carry
/// their own memory.
impl<T: Clone, const N: usize> Clone for Vector<T, Inline<N>> {
    fn clone(&self) -> Self {
        let mut copy = Self::inline();
        for (slot, value) in copy.buffer.iter_mut().zip(self.as_slice()) {
            slot.write(value.clone());
            // Counted one by one, so a panicking `clone` leaks nothing.
            copy.len += 1;
        }
        copy
    }
}

impl<T, A: Allocator + Default> Default for Vector<T, Dynamic<A>> {
    /// Creates an empty vector that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self {
            buffer: Allocation::new(A::default()),
            len: 0,
        }
    }
}

impl<T, S: Storage<T>> Vector<T, S> {
    /// Creates an empty vector on `buffer`.
    ///
    /// Pass a reference (`&mut Memory`, `&mut [MaybeUninit<T>]`, a piece of
    /// an arena) to borrow the memory, a value (`Memory::new()`) to store
    /// it inside the vector, or `Dynamic(allocator)` for a vector that
    /// grows.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<T, Storage = S>,
    {
        Self {
            buffer: buffer.into_buffer(),
            len: 0,
        }
    }

    /// Number of elements in the vector.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Number of elements the vector can hold without growing. For borrowed
    /// and inline storage this is the maximum.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.buffer.slots().len()
    }

    /// Number of elements that can still be pushed without growing.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.capacity() - self.len
    }

    /// `true` when the vector holds no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` when the next push needs more memory: it fails with borrowed
    /// and inline storage, and grows with dynamic storage.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len == self.capacity()
    }

    /// Makes sure `additional` more elements fit, growing dynamic storage
    /// when needed. Fails without changes when the storage is fixed and too
    /// small, or when the allocator refuses.
    #[inline]
    pub fn reserve(&mut self, additional: usize) -> Result<(), Full<()>> {
        match self.len.checked_add(additional) {
            Some(capacity) if self.fits(capacity) => Ok(()),
            _ => Err(Full(())),
        }
    }

    /// `true` when there is room for `capacity` elements, after growing the
    /// storage if it can grow.
    #[inline]
    fn fits(&mut self, capacity: usize) -> bool {
        capacity <= self.capacity() || self.buffer.grow(capacity)
    }

    /// Appends `value`. With dynamic storage the vector grows when needed.
    ///
    /// Hands the value back in [`Full`] when there is no room: the storage
    /// is fixed and full, or the allocator refused.
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), Full<T>> {
        if self.len == self.capacity() && !self.buffer.grow(self.len.saturating_add(1)) {
            return Err(Full(value));
        }
        match self.buffer.slots_mut().get_mut(self.len) {
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
        Some(unsafe {
            self.buffer
                .slots()
                .get_unchecked(self.len)
                .assume_init_read()
        })
    }

    /// Appends all of `values` with a single copy, or nothing when they do
    /// not fit.
    #[inline]
    pub fn extend_from_slice(&mut self, values: &[T]) -> Result<(), Full<()>>
    where
        T: Copy,
    {
        self.reserve(values.len())?;
        // SAFETY: `len + values.len() <= capacity`, so the destination range
        // is inside the buffer. `values` is a shared borrow and the buffer
        // is reached through `&mut self`, so the ranges cannot overlap.
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
        if !self.fits(len) {
            return Err(Full(()));
        }
        let Some(slots) = self.buffer.slots_mut().get_mut(self.len..len) else {
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
        if self.is_full() && !self.buffer.grow(self.len.saturating_add(1)) {
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

    /// Moves all elements onto `buffer` with a single copy and returns a
    /// vector over it, leaving the old memory free. This is how a vector
    /// "grows": get a bigger block, then move into it. It can also change
    /// the kind of storage, for example from borrowed to inline.
    ///
    /// Hands the vector back unchanged when `buffer` is too small (for
    /// `Dynamic`, when the allocator refuses).
    #[inline]
    pub fn move_into<B>(mut self, buffer: B) -> Result<Vector<T, B::Storage>, Full<Self>>
    where
        B: IntoBuffer<T>,
    {
        let len = self.len;
        let mut buffer = buffer.into_buffer();
        if buffer.slots().len() < len && !buffer.grow(len) {
            return Err(Full(self));
        }
        let slots = buffer.slots_mut();
        // SAFETY: the source holds `len` initialised elements and the
        // destination has room for at least `len`. They are two separate
        // buffers, so they cannot overlap. Setting `self.len` to zero
        // afterwards transfers ownership of the elements: the old vector
        // drops nothing.
        unsafe {
            ptr::copy_nonoverlapping(self.as_mut_ptr(), slots.as_mut_ptr().cast::<T>(), len);
        }
        self.len = 0;
        Ok(Vector { buffer, len })
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

    /// Keeps only the elements for which `keep` returns `true`, in their
    /// order, and drops the others. O(n); `keep` is called once per element,
    /// front to back.
    #[inline]
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.retain_mut(|value| keep(value));
    }

    /// Like [`retain`](Self::retain), with mutable access to the elements.
    pub fn retain_mut(&mut self, mut keep: impl FnMut(&mut T) -> bool) {
        let items = self.as_mut_slice();
        let mut kept = 0;
        for index in 0..items.len() {
            if items.get_mut(index).is_some_and(&mut keep) {
                // Kept elements move to the front; the rejected ones collect
                // at the back and are dropped below. A panic in `keep`
                // leaves every element in the vector, only reordered.
                items.swap(kept, index);
                kept += 1;
            }
        }
        self.truncate(kept);
    }

    /// Removes consecutive repeated elements, keeping the first of each
    /// run. On a sorted vector this removes all duplicates. O(n).
    #[inline]
    pub fn dedup(&mut self)
    where
        T: PartialEq,
    {
        self.dedup_by(|a, b| a == b);
    }

    /// Removes consecutive elements that map to the same key, keeping the
    /// first of each run. O(n).
    #[inline]
    pub fn dedup_by_key<K: PartialEq>(&mut self, mut key: impl FnMut(&mut T) -> K) {
        self.dedup_by(|a, b| key(a) == key(b));
    }

    /// Removes each element for which `same(element, kept)` returns `true`,
    /// where `kept` is the last element that stayed. O(n).
    pub fn dedup_by(&mut self, mut same: impl FnMut(&mut T, &mut T) -> bool) {
        let items = self.as_mut_slice();
        if items.is_empty() {
            return;
        }
        let mut kept = 1;
        for index in 1..items.len() {
            let (front, back) = items.split_at_mut(index);
            let repeated = match (back.first_mut(), front.get_mut(kept - 1)) {
                (Some(current), Some(last)) => same(current, last),
                _ => false,
            };
            if !repeated {
                items.swap(kept, index);
                kept += 1;
            }
        }
        self.truncate(kept);
    }

    /// Sorts the elements; equal elements may change order. vas's own
    /// quicksort, not the slice's: it never panics, also on a comparison
    /// that is not a total order, and puts equal elements in the same order
    /// on every compiler and target. O(n log n) worst case, O(n) on sorted
    /// or reversed input, no memory. See [`sort`](crate::sort).
    #[inline]
    pub fn sort_unstable(&mut self)
    where
        T: Ord,
    {
        sort::unstable(self.as_mut_slice());
    }

    /// Sorts the elements by `compare`; equal elements may change order.
    /// See [`sort_unstable`](Self::sort_unstable).
    #[inline]
    pub fn sort_unstable_by(&mut self, compare: impl FnMut(&T, &T) -> Ordering) {
        sort::unstable_by(self.as_mut_slice(), compare);
    }

    /// Sorts the elements by the key `key` gives; elements with equal keys
    /// may change order. The key is computed on every comparison. See
    /// [`sort_unstable`](Self::sort_unstable).
    #[inline]
    pub fn sort_unstable_by_key<K: Ord>(&mut self, key: impl FnMut(&T) -> K) {
        sort::unstable_by_key(self.as_mut_slice(), key);
    }

    /// Sorts the elements, keeping equal elements in their order. Needs no
    /// memory; O(n log n) comparisons and O(n log² n) moves.
    ///
    /// [`sort_unstable`](Self::sort_unstable) is faster when the order of
    /// equal elements does not matter.
    #[inline]
    pub fn sort_stable(&mut self)
    where
        T: Ord,
    {
        sort::stable(self.as_mut_slice());
    }

    /// Sorts the elements by `compare`, keeping equal elements in their
    /// order. See [`sort_stable`](Self::sort_stable).
    #[inline]
    pub fn sort_stable_by(&mut self, compare: impl FnMut(&T, &T) -> Ordering) {
        sort::stable_by(self.as_mut_slice(), compare);
    }

    /// Sorts the elements by `compare`, keeping equal elements in their
    /// order, using `scratch` to go faster. Any amount of scratch memory
    /// works; with room for half the elements every merge uses it, and the
    /// sort is O(n log n) moves instead of O(n log² n).
    ///
    /// `scratch` is only borrowed for the call: pass a `Memory` block, an
    /// arena piece or any `&mut [MaybeUninit<T>]`. Its contents are not
    /// read and are left uninitialised.
    #[inline]
    pub fn sort_stable_in(
        &mut self,
        scratch: &mut [MaybeUninit<T>],
        compare: impl FnMut(&T, &T) -> Ordering,
    ) {
        sort::stable_in(self.as_mut_slice(), scratch, compare);
    }

    /// Sorts the elements by the key `key` gives, keeping elements with
    /// equal keys in their order. The key is computed on every comparison.
    /// See [`sort_stable`](Self::sort_stable).
    #[inline]
    pub fn sort_stable_by_key<K: Ord>(&mut self, key: impl FnMut(&T) -> K) {
        sort::stable_by_key(self.as_mut_slice(), key);
    }

    /// The contents as one contiguous slice.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: the first `len` slots are initialised by the invariant and
        // `MaybeUninit<T>` has the same layout as `T`.
        unsafe { slice::from_raw_parts(self.buffer.slots().as_ptr().cast::<T>(), self.len) }
    }

    /// The contents as one contiguous mutable slice.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: as in `as_slice`, and `self` is borrowed exclusively.
        let len = self.len;
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), len) }
    }

    /// All slots, initialised or not. Together with [`set_len`](Self::set_len)
    /// this lets a container fill slots out of order.
    #[inline]
    pub(crate) fn slots_mut(&mut self) -> &mut [MaybeUninit<T>] {
        self.buffer.slots_mut()
    }

    /// Declares the first `len` slots initialised.
    ///
    /// # Safety
    ///
    /// `len` must not exceed the capacity and the first `len` slots must be
    /// initialised.
    #[inline]
    pub(crate) unsafe fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    #[inline]
    fn as_mut_ptr(&mut self) -> *mut T {
        self.buffer.slots_mut().as_mut_ptr().cast::<T>()
    }
}

impl<T, S: Storage<T>> Drop for Vector<T, S> {
    #[inline]
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T, S: Storage<T>> Deref for Vector<T, S> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, S: Storage<T>> DerefMut for Vector<T, S> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: fmt::Debug, S: Storage<T>> fmt::Debug for Vector<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}
