//! Fixed-capacity binary heap (priority queue) over user-provided memory.

#![allow(unsafe_code)]

use core::cmp::Ordering;
use core::fmt;
use core::mem::{ManuallyDrop, MaybeUninit};
use core::ptr;

use crate::{Borrowed, Full, Vector};

/// Decides which element a [`Heap`] hands out first: the one that compares
/// [`Greater`](Ordering::Greater) than all others.
///
/// Implemented by [`Max`], [`Min`] and by any
/// `Fn(&T, &T) -> Ordering` closure or function.
pub trait Order<T> {
    /// Compares two elements. The heap pops the greatest element first.
    fn cmp(&self, a: &T, b: &T) -> Ordering;
}

/// Largest element first, by `Ord`. The default order.
#[derive(Clone, Copy, Default, Debug)]
pub struct Max;

/// Smallest element first, by `Ord`.
#[derive(Clone, Copy, Default, Debug)]
pub struct Min;

impl<T: Ord> Order<T> for Max {
    #[inline]
    fn cmp(&self, a: &T, b: &T) -> Ordering {
        a.cmp(b)
    }
}

impl<T: Ord> Order<T> for Min {
    #[inline]
    fn cmp(&self, a: &T, b: &T) -> Ordering {
        b.cmp(a)
    }
}

impl<T, F: Fn(&T, &T) -> Ordering> Order<T> for F {
    #[inline]
    fn cmp(&self, a: &T, b: &T) -> Ordering {
        self(a, b)
    }
}

/// A fixed-capacity priority queue that lives in memory provided by the
/// caller.
///
/// `push` and `pop` are O(log n), `peek` is O(1).
///
/// ```
/// use vas::{Heap, Memory};
///
/// let mut memory = Memory::<u32, 16>::new();
/// let mut heap = Heap::min(&mut memory); // smallest first
///
/// assert!(heap.push(5).is_ok());
/// assert!(heap.push(2).is_ok());
/// assert!(heap.push(8).is_ok());
/// assert_eq!(heap.pop(), Some(2));
/// ```
///
/// Types that are not `Ord`, such as `f32` costs, take a comparison:
///
/// ```
/// use vas::{Heap, Memory};
///
/// let mut memory = Memory::<f32, 16>::new();
/// // Lowest cost first.
/// let mut open = Heap::with_order(&mut memory, |a: &f32, b: &f32| b.total_cmp(a));
/// assert!(open.push(2.5).is_ok());
/// assert!(open.push(0.5).is_ok());
/// assert_eq!(open.pop(), Some(0.5));
/// ```
pub struct Heap<'a, T, O = Max> {
    // Invariant (heap property): no element is greater, by `order`, than its
    // parent at `(index - 1) / 2`.
    items: Vector<T, Borrowed<'a>>,
    order: O,
}

impl<'a, T: Ord> Heap<'a, T, Max> {
    /// Creates an empty heap that pops the largest element first.
    #[inline]
    pub fn new(storage: &'a mut [MaybeUninit<T>]) -> Self {
        Self::with_order(storage, Max)
    }
}

impl<'a, T: Ord> Heap<'a, T, Min> {
    /// Creates an empty heap that pops the smallest element first.
    #[inline]
    pub fn min(storage: &'a mut [MaybeUninit<T>]) -> Self {
        Self::with_order(storage, Min)
    }
}

impl<'a, T, O> Heap<'a, T, O> {
    /// Creates an empty heap that pops first whichever element `order`
    /// ranks greatest.
    #[inline]
    pub fn with_order(storage: &'a mut [MaybeUninit<T>], order: O) -> Self {
        Self {
            items: Vector::new(storage),
            order,
        }
    }

    /// Number of elements in the heap.
    #[inline]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Maximum number of elements the heap can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.items.capacity()
    }

    /// Number of elements that can still be pushed.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.items.remaining()
    }

    /// `true` when the heap holds no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `true` when no more elements can be pushed.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.items.is_full()
    }

    /// The element `pop` would return next.
    #[inline]
    pub fn peek(&self) -> Option<&T> {
        self.items.first()
    }

    /// All elements as one contiguous slice, in heap order: the first is the
    /// next to pop, the rest are in no useful order.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self.items.as_slice()
    }

    /// Removes and drops all elements.
    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl<T, O: Order<T>> Heap<'_, T, O> {
    /// Adds `value`, or hands it back in [`Full`] when there is no room.
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), Full<T>> {
        self.items.push(value)?;
        let last = self.items.len() - 1;
        // SAFETY: `last` is the index of the element just pushed.
        unsafe { sift_up(self.items.as_mut_slice(), &self.order, 0, last) };
        Ok(())
    }

    /// Removes and returns the greatest element, or `None` when empty.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        // Takes the root out and puts the last element in its place.
        let top = self.items.swap_remove(0)?;
        if !self.items.is_empty() {
            // SAFETY: the heap is not empty, so index 0 is in bounds.
            unsafe { sift_down_to_bottom(self.items.as_mut_slice(), &self.order, 0) };
        }
        Some(top)
    }
}

/// A gap in a slice whose element has been taken out. The element is put
/// back, at the gap's current position, when the hole is dropped, so the
/// slice is whole again even if a comparison panics.
struct Hole<'h, T> {
    data: &'h mut [T],
    element: ManuallyDrop<T>,
    position: usize,
}

impl<'h, T> Hole<'h, T> {
    /// # Safety
    /// `position` must be in bounds of `data`.
    #[inline]
    unsafe fn new(data: &'h mut [T], position: usize) -> Self {
        // SAFETY: the caller guarantees `position` is in bounds. The slot is
        // treated as empty until `drop` writes the element back.
        let element = unsafe { ptr::read(data.get_unchecked(position)) };
        Self {
            data,
            element: ManuallyDrop::new(element),
            position,
        }
    }

    #[inline]
    fn element(&self) -> &T {
        &self.element
    }

    /// # Safety
    /// `index` must be in bounds of the slice and not the hole's position.
    #[inline]
    unsafe fn get(&self, index: usize) -> &T {
        // SAFETY: guaranteed by the caller.
        unsafe { self.data.get_unchecked(index) }
    }

    /// Moves the element at `index` into the hole; the hole is then at
    /// `index`.
    ///
    /// # Safety
    /// `index` must be in bounds of the slice and not the hole's position.
    #[inline]
    unsafe fn move_to(&mut self, index: usize) {
        // SAFETY: both positions are in bounds and distinct, so the copy
        // does not overlap. The source slot becomes the new hole.
        unsafe {
            let base = self.data.as_mut_ptr();
            ptr::copy_nonoverlapping(base.add(index), base.add(self.position), 1);
        }
        self.position = index;
    }
}

impl<T> Drop for Hole<'_, T> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: `position` is in bounds and is the one slot that currently
        // holds no element; the taken element is written back exactly once.
        unsafe {
            let slot = self.data.get_unchecked_mut(self.position);
            ptr::copy_nonoverlapping(&*self.element, slot, 1);
        }
    }
}

/// Moves the element at `position` up until its parent is not smaller, but
/// not above `start`.
///
/// # Safety
/// `position` must be in bounds of `data`.
#[inline]
unsafe fn sift_up<T, O: Order<T>>(data: &mut [T], order: &O, start: usize, position: usize) {
    // SAFETY: the caller guarantees `position` is in bounds.
    let mut hole = unsafe { Hole::new(data, position) };
    while hole.position > start {
        let parent = (hole.position - 1) / 2;
        // SAFETY: `parent < hole.position`, so it is in bounds and is not
        // the hole.
        if order.cmp(hole.element(), unsafe { hole.get(parent) }) != Ordering::Greater {
            break;
        }
        // SAFETY: as above.
        unsafe { hole.move_to(parent) };
    }
}

/// Moves the element at `position` all the way down to a leaf, following the
/// greater child each time, then back up to where it belongs. For an element
/// that came from the bottom of the heap this needs fewer comparisons than
/// stopping on the way down.
///
/// # Safety
/// `position` must be in bounds of `data`.
#[inline]
unsafe fn sift_down_to_bottom<T, O: Order<T>>(data: &mut [T], order: &O, position: usize) {
    let end = data.len();
    // SAFETY: the caller guarantees `position` is in bounds.
    let mut hole = unsafe { Hole::new(data, position) };
    let mut child = hole.position.saturating_mul(2).saturating_add(1);
    // While there are two children, move the greater one up.
    while child <= end.saturating_sub(2) {
        // SAFETY: `child` and `child + 1` are below `end` and greater than
        // the hole's position.
        let right_is_greater = order
            .cmp(unsafe { hole.get(child + 1) }, unsafe { hole.get(child) })
            == Ordering::Greater;
        child += usize::from(right_is_greater);
        // SAFETY: `child` is still below `end` and is not the hole.
        unsafe { hole.move_to(child) };
        child = hole.position.saturating_mul(2).saturating_add(1);
    }
    if child == end - 1 {
        // SAFETY: `child` is the last index and greater than the hole's
        // position.
        unsafe { hole.move_to(child) };
    }
    let bottom = hole.position;
    // Writes the element back at `bottom`.
    drop(hole);
    // SAFETY: `bottom` was a valid hole position, so it is in bounds.
    unsafe { sift_up(data, order, position, bottom) };
}

impl<T: fmt::Debug, O> fmt::Debug for Heap<'_, T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}
