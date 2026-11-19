//! Disjoint sets (union-find) over borrowed, inline or growing memory.

use core::fmt;

use crate::allocator::Allocator;
use crate::storage::{Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, OutOfBounds, Vector};

/// The state of one element. Opaque; it only exists so the user can size
/// the memory: `Memory::<Link, N>` for `N` elements.
#[derive(Clone, Copy, Debug)]
pub struct Link {
    // The element this one points towards its set's root; a root points
    // at itself.
    parent: u32,
    // For a root, the number of elements in its set.
    size: u32,
}

/// Elements `0..len()` split into disjoint sets: union-find.
///
/// `union` joins the sets of two elements and `same` asks whether two
/// elements are in one set, both in nearly O(1) (inverse Ackermann,
/// amortised), with union by size and path halving. Nothing recurses.
///
/// Use it for connectivity that only grows: which rooms a maze generator
/// has joined, which tiles form one island, Kruskal's minimum spanning tree.
///
/// ```
/// use vas::union_find::Link;
/// use vas::{Memory, UnionFind};
///
/// let mut memory = Memory::<Link, 16>::new();
/// let mut rooms = UnionFind::new(&mut memory);
/// assert!(rooms.reset(16).is_ok());
///
/// assert_eq!(rooms.union(3, 7), Ok(true)); // a door between 3 and 7
/// assert_eq!(rooms.union(7, 12), Ok(true));
/// assert_eq!(rooms.union(3, 12), Ok(false)); // already connected
/// assert!(rooms.same(12, 3));
/// assert_eq!(rooms.set_count(), 14);
/// ```
pub struct UnionFind<S: Storage<Link>> {
    // Invariant: every `parent` is below `links.len()`, and following
    // parents reaches a root without cycles.
    links: Vector<Link, S>,
    sets: usize,
}

impl<const N: usize> UnionFind<Inline<N>> {
    /// Creates an empty structure for up to `N` elements, held inside
    /// itself. This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            links: Vector::inline(),
            sets: 0,
        }
    }
}

impl<const N: usize> Default for UnionFind<Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Only structures with inline storage can be cloned.
impl<const N: usize> Clone for UnionFind<Inline<N>> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            links: self.links.clone(),
            sets: self.sets,
        }
    }
}

impl<A: Allocator + Default> Default for UnionFind<Dynamic<A>> {
    /// Creates an empty structure that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<S: Storage<Link>> UnionFind<S> {
    /// Creates an empty structure on `buffer`: a reference to memory borrows
    /// it, a `Memory` passed by value is stored inside.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Link, Storage = S>,
    {
        Self {
            links: Vector::new(buffer),
            sets: 0,
        }
    }

    /// Number of elements.
    #[inline]
    pub const fn len(&self) -> usize {
        self.links.len()
    }

    /// `true` when there are no elements.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// Number of elements there is memory for. With dynamic storage it
    /// grows.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.links.capacity()
    }

    /// Number of distinct sets.
    #[inline]
    pub const fn set_count(&self) -> usize {
        self.sets
    }

    /// Adds an element in a set of its own and returns it: the next index.
    pub fn add(&mut self) -> Result<u32, Full<()>> {
        let element = u32::try_from(self.links.len()).map_err(|_| Full(()))?;
        self.links
            .push(Link {
                parent: element,
                size: 1,
            })
            .map_err(|_| Full(()))?;
        self.sets += 1;
        Ok(element)
    }

    /// Replaces everything by `count` elements, each in a set of its own.
    /// O(count). Fails without changes when there is no room, or `count`
    /// does not fit a `u32`.
    pub fn reset(&mut self, count: usize) -> Result<(), Full<()>> {
        if u32::try_from(count).is_err() {
            return Err(Full(()));
        }
        if count > self.links.len() {
            self.links.reserve(count - self.links.len())?;
        }
        self.links.clear();
        let mut next = 0;
        self.links.resize_with(count, || {
            let link = Link {
                parent: next,
                size: 1,
            };
            next += 1;
            link
        })?;
        self.sets = count;
        Ok(())
    }

    /// Removes every element.
    #[inline]
    pub fn clear(&mut self) {
        self.links.clear();
        self.sets = 0;
    }

    /// The root of `element`'s set, which stands for the whole set: two
    /// elements are in one set exactly when their roots are equal. `None`
    /// past the last element.
    ///
    /// Shortens the path it walks (path halving), which keeps later calls
    /// fast; [`root`](Self::root) does the same without changing anything.
    pub fn find(&mut self, element: u32) -> Option<u32> {
        let links = self.links.as_mut_slice();
        let mut current = element;
        loop {
            let parent = links.get(current as usize)?.parent;
            if parent == current {
                return Some(current);
            }
            let grandparent = links.get(parent as usize)?.parent;
            // Point at the grandparent: every other node on the path
            // halves its distance to the root.
            links.get_mut(current as usize)?.parent = grandparent;
            current = grandparent;
        }
    }

    /// The root of `element`'s set, like [`find`](Self::find), without
    /// shortening the path. O(log n) with union by size.
    pub fn root(&self, element: u32) -> Option<u32> {
        let mut current = element;
        // Bounded by the number of elements: guards against ever looping.
        for _ in 0..=self.links.len() {
            let parent = self.links.get(current as usize)?.parent;
            if parent == current {
                return Some(current);
            }
            current = parent;
        }
        None
    }

    /// Joins the sets of `a` and `b`. Returns `true` when they were apart,
    /// `false` when they already were one set. The smaller set goes under
    /// the larger.
    pub fn union(&mut self, a: u32, b: u32) -> Result<bool, OutOfBounds> {
        let a = self.find(a).ok_or(OutOfBounds)?;
        let b = self.find(b).ok_or(OutOfBounds)?;
        if a == b {
            return Ok(false);
        }
        let links = self.links.as_mut_slice();
        let (Some(&Link { size: size_a, .. }), Some(&Link { size: size_b, .. })) =
            (links.get(a as usize), links.get(b as usize))
        else {
            return Err(OutOfBounds);
        };
        let (root, child) = if size_a >= size_b { (a, b) } else { (b, a) };
        if let Some(link) = links.get_mut(child as usize) {
            link.parent = root;
        }
        if let Some(link) = links.get_mut(root as usize) {
            // At most the number of elements, which fits a `u32`.
            link.size = size_a + size_b;
        }
        self.sets -= 1;
        Ok(true)
    }

    /// `true` when `a` and `b` are in one set; `false` when either is past
    /// the last element.
    #[inline]
    pub fn same(&mut self, a: u32, b: u32) -> bool {
        match (self.find(a), self.find(b)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    /// The number of elements in `element`'s set.
    #[inline]
    pub fn set_size(&mut self, element: u32) -> Option<u32> {
        let root = self.find(element)?;
        self.links.get(root as usize).map(|link| link.size)
    }
}

impl<S: Storage<Link>> fmt::Debug for UnionFind<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnionFind")
            .field("len", &self.len())
            .field("sets", &self.sets)
            .finish()
    }
}
