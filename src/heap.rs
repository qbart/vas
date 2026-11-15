//! Fixed-capacity binary heap (priority queue) over user-provided memory.

use core::cmp::Ordering;
use core::fmt;
use core::mem::{self, MaybeUninit};

use crate::{Full, Vector};

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
    items: Vector<'a, T>,
    order: O,
}

impl<'a, T: Ord> Heap<'a, T, Max> {
    /// Creates an empty heap that pops the largest element first.
    #[inline]
    pub const fn new(storage: &'a mut [MaybeUninit<T>]) -> Self {
        Self::with_order(storage, Max)
    }
}

impl<'a, T: Ord> Heap<'a, T, Min> {
    /// Creates an empty heap that pops the smallest element first.
    #[inline]
    pub const fn min(storage: &'a mut [MaybeUninit<T>]) -> Self {
        Self::with_order(storage, Min)
    }
}

impl<'a, T, O> Heap<'a, T, O> {
    /// Creates an empty heap that pops first whichever element `order`
    /// ranks greatest.
    #[inline]
    pub const fn with_order(storage: &'a mut [MaybeUninit<T>], order: O) -> Self {
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
    pub const fn capacity(&self) -> usize {
        self.items.capacity()
    }

    /// Number of elements that can still be pushed.
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.items.remaining()
    }

    /// `true` when the heap holds no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `true` when no more elements can be pushed.
    #[inline]
    pub const fn is_full(&self) -> bool {
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
    pub const fn as_slice(&self) -> &[T] {
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
        self.sift_up(self.items.len() - 1);
        Ok(())
    }

    /// Removes and returns the greatest element, or `None` when empty.
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        // Takes the root out and puts the last element in its place.
        let top = self.items.swap_remove(0)?;
        self.sift_down(0);
        Some(top)
    }

    /// Moves the element at `index` up until its parent is not smaller.
    fn sift_up(&mut self, mut index: usize) {
        while index > 0 {
            let parent = (index - 1) / 2;
            let Ok([child, above]) = self.items.get_disjoint_mut([index, parent]) else {
                return;
            };
            if self.order.cmp(child, above) != Ordering::Greater {
                return;
            }
            mem::swap(child, above);
            index = parent;
        }
    }

    /// Moves the element at `index` down until no child is greater.
    fn sift_down(&mut self, mut index: usize) {
        loop {
            let Some(left) = index.checked_mul(2).and_then(|i| i.checked_add(1)) else {
                return;
            };
            let Some(left_item) = self.items.get(left) else {
                return;
            };
            // Pick the greater of the two children.
            let child = match self.items.get(left + 1) {
                Some(right_item) if self.order.cmp(right_item, left_item) == Ordering::Greater => {
                    left + 1
                }
                _ => left,
            };
            let Ok([above, below]) = self.items.get_disjoint_mut([index, child]) else {
                return;
            };
            if self.order.cmp(below, above) != Ordering::Greater {
                return;
            }
            mem::swap(above, below);
            index = child;
        }
    }
}

impl<T: fmt::Debug, O> fmt::Debug for Heap<'_, T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_slice(), f)
    }
}
