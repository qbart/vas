//! Bounding volume hierarchy over user-provided memory.

#![allow(unsafe_code)]

use core::mem::MaybeUninit;
use core::{fmt, slice};

use crate::aabb::Aabb;

/// "No child": marks a leaf.
const NONE: u32 = u32::MAX;

/// Entries of the traversal stack. The tree is balanced, so its depth is at
/// most 33 for any number of items that fits in `u32`.
const STACK: usize = 64;

/// One node of tree storage. Opaque; it only exists so the user can size
/// the node memory: `Memory::<Node<D>, COUNT>`.
#[derive(Clone, Copy)]
pub struct Node<const D: usize> {
    bounds: Aabb<D>,
    // Inner node: index of the left child; the right child follows it.
    // Leaf: `NONE`.
    left: u32,
    // The items this node covers: `items[start..start + count]`.
    start: u32,
    count: u32,
}

/// Why a [`Bvh`] could not be built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildError {
    /// There are items but no node memory (at least one node is needed), or
    /// more items than fit in a `u32`.
    Full,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("node memory is too small for the tree")
    }
}

impl core::error::Error for BuildError {}

/// A bounding volume hierarchy: a tree of nested boxes over a set of items
/// that each have a bounding box, in memory provided by the caller.
///
/// Unlike the point structures, items here have a **size**. It answers
/// "which boxes overlap this box / contain this point / are crossed by this
/// ray", visiting only a small part of the set. Use it for collision broad
/// phase, picking, line of sight and culling.
///
/// The tree is built in one go over a slice of `(Aabb, T)` pairs, which it
/// reorders in place. When the items move, change their boxes and call
/// [`refit`](Self::refit), which is much cheaper than building again.
///
/// ```
/// use vas::bvh::Node;
/// use vas::{Aabb, Bvh, Memory};
///
/// let mut items = [
///     (Aabb::new([0.0, 0.0], [1.0, 1.0]), "crate"),
///     (Aabb::new([5.0, 0.0], [6.0, 2.0]), "door"),
/// ];
/// let mut nodes = Memory::<Node<2>, 3>::new();
///
/// if let Ok(bvh) = Bvh::build(&mut items, &mut nodes) {
///     // What does a ray going right from (-1, 0.5) cross?
///     let mut crossed = 0;
///     bvh.query_ray(&[-1.0, 0.5], &[1.0, 0.0], 100.0, |_, _, _| crossed += 1);
///     assert_eq!(crossed, 2);
/// }
/// ```
pub struct Bvh<'a, T, const D: usize> {
    // Invariants:
    // - node 0 is the root (when there are items); children have higher
    //   indices than their parent, and the two children of a node are
    //   consecutive.
    // - a node's `bounds` contain the boxes of all items in its range (as of
    //   the last build or refit).
    items: &'a mut [(Aabb<D>, T)],
    nodes: &'a mut [Node<D>],
    leaf_size: u32,
}

impl<'a, T, const D: usize> Bvh<'a, T, D> {
    /// Builds a tree over `items`, with up to 4 items per leaf.
    ///
    /// `items` is reordered in place. `nodes` needs at least one node; a
    /// full tree takes at most `2 * items.len() - 1`. With fewer, some
    /// leaves hold more items: the tree stays correct but gets slower.
    ///
    /// O(n log n), no memory beyond the two blocks.
    #[inline]
    pub fn build(
        items: &'a mut [(Aabb<D>, T)],
        nodes: &'a mut [MaybeUninit<Node<D>>],
    ) -> Result<Self, BuildError> {
        Self::build_with(items, nodes, 4)
    }

    /// Like [`build`](Self::build), with a chosen number of items per leaf
    /// (at least 1). Fewer items per leaf means more nodes and fewer box
    /// tests per query.
    pub fn build_with(
        items: &'a mut [(Aabb<D>, T)],
        nodes: &'a mut [MaybeUninit<Node<D>>],
        leaf_size: u32,
    ) -> Result<Self, BuildError> {
        let count = u32::try_from(items.len()).map_err(|_| BuildError::Full)?;
        if count > 0 && nodes.is_empty() {
            return Err(BuildError::Full);
        }
        // A tree never needs more than 2n - 1 nodes.
        let needed = (items.len().saturating_mul(2)).saturating_sub(1);
        let usable = nodes.len().min(needed).min((u32::MAX - 1) as usize);
        let nodes = nodes.get_mut(..usable).ok_or(BuildError::Full)?;
        let empty = Node {
            bounds: Aabb::point([0.0; D]),
            left: NONE,
            start: 0,
            count: 0,
        };
        for node in nodes.iter_mut() {
            node.write(empty);
        }
        // SAFETY: every element was just initialised, and
        // `MaybeUninit<Node<D>>` has the layout of `Node<D>`.
        let nodes =
            unsafe { slice::from_raw_parts_mut(nodes.as_mut_ptr().cast::<Node<D>>(), nodes.len()) };

        let mut bvh = Self {
            items,
            nodes,
            leaf_size: leaf_size.max(1),
        };
        let mut used = u32::from(count > 0);
        if count > 0 {
            bvh.split(0, 0, count, &mut used);
        }
        // Keep only the nodes that are part of the tree.
        let nodes = core::mem::take(&mut bvh.nodes);
        bvh.nodes = nodes.get_mut(..used as usize).unwrap_or_default();
        Ok(bvh)
    }

    /// Number of items in the tree.
    #[inline]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when the tree holds no items.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Number of nodes the tree uses.
    #[inline]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The box containing every item, or `None` for an empty tree.
    #[inline]
    pub fn bounds(&self) -> Option<Aabb<D>> {
        self.nodes.first().map(|root| root.bounds)
    }

    /// The items, in the order the tree arranged them.
    #[inline]
    pub const fn items(&self) -> &[(Aabb<D>, T)] {
        self.items
    }

    /// The items, mutably. After changing any box, call
    /// [`refit`](Self::refit) before the next query, or queries may miss
    /// items.
    #[inline]
    pub const fn items_mut(&mut self) -> &mut [(Aabb<D>, T)] {
        self.items
    }

    /// Recomputes every node's box from the items' current boxes, keeping
    /// the tree's shape. O(n).
    ///
    /// Use it each frame for moving objects. Queries stay exact; they slow
    /// down gradually as items drift far from where they were when the tree
    /// was built, at which point building again restores the speed.
    pub fn refit(&mut self) {
        // Children have higher indices than their parent, so going backwards
        // handles every child before its parent.
        for index in (0..self.nodes.len()).rev() {
            let Some(node) = self.nodes.get(index).copied() else {
                continue;
            };
            let bounds = if node.left == NONE {
                self.range_bounds(node.start, node.count)
            } else {
                let children = self
                    .nodes
                    .get(node.left as usize)
                    .zip(self.nodes.get(node.left as usize + 1));
                match children {
                    Some((left, right)) => left.bounds.union(&right.bounds),
                    None => node.bounds,
                }
            };
            if let Some(node) = self.nodes.get_mut(index) {
                node.bounds = bounds;
            }
        }
    }

    /// Calls `visit(aabb, value)` for every item whose box overlaps or
    /// touches `area`. The order is unspecified.
    #[inline]
    pub fn query(&self, area: &Aabb<D>, mut visit: impl FnMut(&Aabb<D>, &T)) {
        self.walk(
            |bounds| bounds.intersects(area).then_some(()),
            |aabb, value, ()| visit(aabb, value),
        );
    }

    /// Calls `visit(aabb, value)` for every item whose box contains `point`
    /// (boundary included). The order is unspecified.
    #[inline]
    pub fn query_point(&self, point: &[f32; D], mut visit: impl FnMut(&Aabb<D>, &T)) {
        self.walk(
            |bounds| bounds.contains_point(point).then_some(()),
            |aabb, value, ()| visit(aabb, value),
        );
    }

    /// Calls `visit(aabb, value, distance)` for every item whose box is
    /// crossed by the ray from `origin` along `direction`, within
    /// `max_distance`. `distance` is where the ray enters the box (0 when it
    /// starts inside), in multiples of the length of `direction`.
    ///
    /// The order is unspecified, not nearest first. The test is against the
    /// item's box; test the actual shape yourself for an exact hit.
    #[inline]
    pub fn query_ray(
        &self,
        origin: &[f32; D],
        direction: &[f32; D],
        max_distance: f32,
        mut visit: impl FnMut(&Aabb<D>, &T, f32),
    ) {
        self.walk(
            |bounds| bounds.ray(origin, direction, max_distance),
            |aabb, value, distance| visit(aabb, value, distance),
        );
    }

    /// Visits every item whose box passes `test`, skipping whole subtrees
    /// whose box fails it.
    fn walk<R>(
        &self,
        test: impl Fn(&Aabb<D>) -> Option<R>,
        mut visit: impl FnMut(&Aabb<D>, &T, R),
    ) {
        let mut stack = [0u32; STACK];
        let mut depth = usize::from(!self.nodes.is_empty());
        while depth > 0 {
            depth -= 1;
            let Some(node) = stack
                .get(depth)
                .and_then(|index| self.nodes.get(*index as usize))
            else {
                continue;
            };
            if test(&node.bounds).is_none() {
                continue;
            }
            if node.left == NONE {
                let range = node.start as usize..(node.start + node.count) as usize;
                for (aabb, value) in self.items.get(range).unwrap_or_default() {
                    if let Some(result) = test(aabb) {
                        visit(aabb, value, result);
                    }
                }
                continue;
            }
            for child in [node.left, node.left + 1] {
                if let Some(slot) = stack.get_mut(depth) {
                    *slot = child;
                    depth += 1;
                }
            }
        }
    }

    /// Builds the subtree of node `index` over `items[start..start + count]`.
    /// Recursion is at most 33 deep, because each split halves the range.
    fn split(&mut self, index: u32, start: u32, count: u32, used: &mut u32) {
        let bounds = self.range_bounds(start, count);
        let mut node = Node {
            bounds,
            left: NONE,
            start,
            count,
        };
        let can_split = count > self.leaf_size && (*used as usize + 2) <= self.nodes.len();
        let range = start as usize..(start + count) as usize;
        if let (true, Some(items)) = (can_split, self.items.get_mut(range)) {
            // Split along the axis on which the box centres spread the most,
            // at the median, so both halves get the same number of items.
            let mut low = [f32::INFINITY; D];
            let mut high = [f32::NEG_INFINITY; D];
            for (aabb, _) in items.iter() {
                let axes = low.iter_mut().zip(high.iter_mut()).zip(aabb.center());
                for ((low, high), center) in axes {
                    *low = low.min(center);
                    *high = high.max(center);
                }
            }
            let axis = low
                .iter()
                .zip(high.iter())
                .map(|(low, high)| high - low)
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(0, |(axis, _)| axis);
            let half = count / 2;
            if (half as usize) < items.len() {
                let center = |aabb: &Aabb<D>| aabb.center().get(axis).copied().unwrap_or(0.0);
                items.select_nth_unstable_by(half as usize, |a, b| {
                    center(&a.0).total_cmp(&center(&b.0))
                });
            }
            let left = *used;
            *used += 2;
            node.left = left;
            self.split(left, start, half, used);
            self.split(left + 1, start + half, count - half, used);
        }
        if let Some(slot) = self.nodes.get_mut(index as usize) {
            *slot = node;
        }
    }

    /// The box containing `items[start..start + count]`.
    fn range_bounds(&self, start: u32, count: u32) -> Aabb<D> {
        let range = start as usize..(start + count) as usize;
        let mut items = self.items.get(range).unwrap_or_default().iter();
        let first = items.next().map_or(Aabb::point([0.0; D]), |item| item.0);
        items.fold(first, |bounds, item| bounds.union(&item.0))
    }
}
