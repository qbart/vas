//! Fixed-capacity quadtree / octree for points, over user-provided memory.

#![allow(unsafe_code)]

use core::mem::MaybeUninit;
use core::{fmt, slice};

use crate::Full;
use crate::aabb::{Aabb, distance_squared};
use crate::pool::{self, Handle, Pool};

/// "No node" / "no entry" in a link.
const NONE: u32 = u32::MAX;

/// Deepest subdivision ever allowed; bounds the recursion.
const DEPTH_LIMIT: u32 = 32;

/// One node of tree storage. Opaque; it only exists so the user can size
/// the node memory: `Memory::<Node, COUNT>`.
#[derive(Clone, Copy)]
pub struct Node {
    // First of this node's 2^D children, which are stored next to each
    // other; `NONE` for a leaf. In a freed block: the next free block.
    first_child: u32,
    // Head of the list of entries stored here (leaves only).
    first_entry: u32,
    // Number of entries in this node's whole subtree.
    count: u32,
    parent: u32,
}

/// A value in an [`Orthtree`] together with its position and links. Opaque;
/// it appears in the types [`Slot`] and [`EntryId`].
pub struct Entry<T, const D: usize> {
    value: T,
    position: [f32; D],
    // The leaf this entry is listed in, and its neighbours in that list.
    node: u32,
    next: u32,
    prev: u32,
}

/// One slot of entry storage: `Memory::<Slot<T, D>, COUNT>`.
pub type Slot<T, const D: usize> = pool::Slot<Entry<T, D>>;

/// Identifies an entry of an [`Orthtree`]. `Copy`, 8 bytes, stale once the
/// entry is removed.
pub type EntryId<T, const D: usize> = Handle<Entry<T, D>>;

/// An [`Orthtree`] in two dimensions: each node splits into four.
pub type Quadtree<'a, T> = Orthtree<'a, T, 2>;

/// An [`Orthtree`] in three dimensions: each node splits into eight.
pub type Octree<'a, T> = Orthtree<'a, T, 3>;

/// Why an [`Orthtree`] could not be created.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NewError {
    /// The node memory is empty; at least one node (the root) is needed.
    Full,
    /// The bounds are not finite or have no extent along some axis, or `D`
    /// is not between 1 and 8.
    Invalid,
}

impl fmt::Display for NewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "node memory is empty",
            Self::Invalid => "tree bounds or dimension are invalid",
        })
    }
}

impl core::error::Error for NewError {}

/// The region a node covers, tracked while walking down the tree.
#[derive(Clone, Copy)]
struct Region<const D: usize> {
    center: [f32; D],
    half: [f32; D],
}

impl<const D: usize> Region<D> {
    /// Which child of this region `position` belongs to: bit `a` is set
    /// when it is in the upper half along axis `a`.
    #[inline]
    fn quadrant(&self, position: &[f32; D]) -> u32 {
        let mut quadrant = 0;
        for (axis, (value, center)) in position.iter().zip(self.center.iter()).enumerate() {
            if value >= center {
                quadrant |= 1 << axis;
            }
        }
        quadrant
    }

    /// The region of child `quadrant`.
    #[inline]
    fn child(&self, quadrant: u32) -> Self {
        let mut child = *self;
        for (axis, (center, half)) in child
            .center
            .iter_mut()
            .zip(child.half.iter_mut())
            .enumerate()
        {
            *half *= 0.5;
            if quadrant & (1 << axis) != 0 {
                *center += *half;
            } else {
                *center -= *half;
            }
        }
        child
    }

    /// `true` when child `quadrant` can hold points of `area`. Children are
    /// treated as unbounded outwards, so points outside the tree's bounds
    /// are still found.
    #[inline]
    fn child_touches(&self, quadrant: u32, area: &Aabb<D>) -> bool {
        self.center
            .iter()
            .zip(area.min.iter().zip(area.max.iter()))
            .enumerate()
            .all(|(axis, (center, (low, high)))| {
                if quadrant & (1 << axis) != 0 {
                    high >= center
                } else {
                    low < center
                }
            })
    }
}

/// A tree that splits space into 2^D equal parts wherever points are
/// crowded: a quadtree for `D = 2`, an octree for `D = 3`. It answers "which
/// points are in this box / within this distance", in memory provided by the
/// caller.
///
/// Unlike a uniform grid it adapts to uneven distributions: dense areas are
/// subdivided, empty areas cost nothing. Use [`Quadtree`] and [`Octree`] for
/// the two common cases.
///
/// Each entry is a point with a value. Positions outside the bounds are
/// allowed and are still found by queries that cover them.
///
/// ```
/// use vas::orthtree::{Node, Slot};
/// use vas::{Aabb, Memory, Quadtree};
///
/// let world = Aabb::new([0.0, 0.0], [100.0, 100.0]);
/// let mut nodes = Memory::<Node, 85>::new();
/// let mut entries = Memory::<Slot<&str, 2>, 64>::new();
///
/// if let Ok(mut tree) = Quadtree::new(world, &mut nodes, &mut entries) {
///     assert!(tree.insert([12.0, 14.0], "goblin").is_ok());
///     assert!(tree.insert([80.0, 75.0], "dragon").is_ok());
///
///     let mut seen = 0;
///     tree.query(&Aabb::new([0.0, 0.0], [50.0, 50.0]), |_, _| seen += 1);
///     assert_eq!(seen, 1);
/// }
/// ```
pub struct Orthtree<'a, T, const D: usize> {
    // Invariants:
    // - node 0 is the root; the children of a node are the `CHILDREN`
    //   consecutive nodes starting at `first_child`.
    // - a leaf's list holds exactly the live entries whose `node` is that
    //   leaf; inner nodes have no entries of their own.
    // - `count` of every node is the number of entries in its subtree.
    nodes: &'a mut [Node],
    entries: Pool<'a, Entry<T, D>>,
    root: Region<D>,
    bounds: Aabb<D>,
    leaf_size: u32,
    max_depth: u32,
    // Nodes `0..used` have been handed out at some point.
    used: u32,
    // Head of the list of freed child blocks.
    free_block: u32,
    // Nodes currently part of the tree.
    live_nodes: u32,
}

impl<'a, T, const D: usize> Orthtree<'a, T, D> {
    /// Number of children of a node that has been split.
    const CHILDREN: u32 = 1 << D;

    /// Creates an empty tree covering `bounds`, splitting a leaf once it
    /// holds more than 8 entries, at most 16 levels deep.
    ///
    /// `nodes` needs at least one node. Each split takes 2^D more; when none
    /// are left, leaves simply hold more entries, so the tree keeps working
    /// but gets slower in crowded areas.
    #[inline]
    pub fn new(
        bounds: Aabb<D>,
        nodes: &'a mut [MaybeUninit<Node>],
        entries: &'a mut [MaybeUninit<Slot<T, D>>],
    ) -> Result<Self, NewError> {
        Self::with_limits(bounds, 8, 16, nodes, entries)
    }

    /// Like [`new`](Self::new), with a chosen `leaf_size` (entries a leaf
    /// may hold before it is split; at least 1) and `max_depth` (levels of
    /// subdivision; at most 32).
    pub fn with_limits(
        bounds: Aabb<D>,
        leaf_size: u32,
        max_depth: u32,
        nodes: &'a mut [MaybeUninit<Node>],
        entries: &'a mut [MaybeUninit<Slot<T, D>>],
    ) -> Result<Self, NewError> {
        if D == 0 || D > 8 {
            return Err(NewError::Invalid);
        }
        let mut half = bounds.size();
        for ((half, low), high) in half
            .iter_mut()
            .zip(bounds.min.iter())
            .zip(bounds.max.iter())
        {
            if !low.is_finite() || !high.is_finite() || half.is_nan() || *half <= 0.0 {
                return Err(NewError::Invalid);
            }
            *half *= 0.5;
        }
        let usable = nodes.len().min((u32::MAX - 1) as usize);
        let nodes = nodes.get_mut(..usable).ok_or(NewError::Full)?;
        if nodes.is_empty() {
            return Err(NewError::Full);
        }
        for node in nodes.iter_mut() {
            node.write(Node::leaf(NONE));
        }
        // SAFETY: every element was just initialised, and
        // `MaybeUninit<Node>` has the layout of `Node`.
        let nodes =
            unsafe { slice::from_raw_parts_mut(nodes.as_mut_ptr().cast::<Node>(), nodes.len()) };
        Ok(Self {
            nodes,
            entries: Pool::new(entries),
            root: Region {
                center: bounds.center(),
                half,
            },
            bounds,
            leaf_size: leaf_size.max(1),
            max_depth: max_depth.min(DEPTH_LIMIT),
            used: 1,
            free_block: NONE,
            live_nodes: 1,
        })
    }

    /// Number of entries in the tree.
    #[inline]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Maximum number of entries the tree can hold.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// `true` when the tree holds no entries.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` when no more entries can be inserted.
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    /// The region the tree subdivides.
    #[inline]
    pub const fn bounds(&self) -> Aabb<D> {
        self.bounds
    }

    /// Number of nodes currently in use, the root included. Handy for
    /// judging how much node memory a workload needs.
    #[inline]
    pub const fn node_count(&self) -> usize {
        self.live_nodes as usize
    }

    /// Adds `value` at `position`, or hands the value back in [`Full`] when
    /// there is no room.
    pub fn insert(&mut self, position: [f32; D], value: T) -> Result<EntryId<T, D>, Full<T>> {
        let entry = Entry {
            value,
            position,
            node: NONE,
            next: NONE,
            prev: NONE,
        };
        let id = self
            .entries
            .insert(entry)
            .map_err(|Full(entry)| Full(entry.value))?;
        self.place(id.index(), &position);
        Ok(id)
    }

    /// Removes an entry and returns its value.
    pub fn remove(&mut self, id: EntryId<T, D>) -> Option<T> {
        if !self.entries.contains(id) {
            return None;
        }
        self.detach(id.index());
        self.entries.remove(id).map(|entry| entry.value)
    }

    /// Moves an entry to `position`. Returns `false` when the id is stale.
    pub fn set_position(&mut self, id: EntryId<T, D>, position: [f32; D]) -> bool {
        if !self.entries.contains(id) {
            return false;
        }
        self.detach(id.index());
        if let Some(entry) = self.entries.get_at_mut(id.index()) {
            entry.position = position;
        }
        self.place(id.index(), &position);
        true
    }

    /// The value of entry `id`.
    #[inline]
    pub fn get(&self, id: EntryId<T, D>) -> Option<&T> {
        self.entries.get(id).map(|entry| &entry.value)
    }

    /// The value of entry `id`, mutably.
    #[inline]
    pub fn get_mut(&mut self, id: EntryId<T, D>) -> Option<&mut T> {
        self.entries.get_mut(id).map(|entry| &mut entry.value)
    }

    /// The position of entry `id`.
    #[inline]
    pub fn position(&self, id: EntryId<T, D>) -> Option<[f32; D]> {
        self.entries.get(id).map(|entry| entry.position)
    }

    /// Removes and drops all entries and collapses the tree to its root.
    /// Every existing id goes stale.
    pub fn clear(&mut self) {
        self.entries.clear();
        if let Some(root) = self.nodes.first_mut() {
            *root = Node::leaf(NONE);
        }
        self.used = 1;
        self.free_block = NONE;
        self.live_nodes = 1;
    }

    /// Calls `visit(id, value)` for every entry whose position is inside
    /// `area` (boundary included). The order is unspecified.
    pub fn query(&self, area: &Aabb<D>, mut visit: impl FnMut(EntryId<T, D>, &T)) {
        self.visit_node(0, &self.root, area, &mut |id, entry: &Entry<T, D>| {
            if area.contains_point(&entry.position) {
                visit(id, &entry.value);
            }
        });
    }

    /// Calls `visit(id, value)` for every entry within `radius` of `center`
    /// (boundary included). The order is unspecified.
    pub fn query_radius(
        &self,
        center: [f32; D],
        radius: f32,
        mut visit: impl FnMut(EntryId<T, D>, &T),
    ) {
        if radius.is_nan() || radius < 0.0 {
            return;
        }
        let limit = radius * radius;
        let area = Aabb::around(center, radius);
        self.visit_node(0, &self.root, &area, &mut |id, entry: &Entry<T, D>| {
            if distance_squared(&entry.position, &center) <= limit {
                visit(id, &entry.value);
            }
        });
    }

    /// Iterates over all entries as `(id, position, value)`, in no
    /// particular order.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (EntryId<T, D>, [f32; D], &T)> {
        self.entries
            .iter()
            .map(|(id, entry)| (id, entry.position, &entry.value))
    }

    /// Visits the entries of every leaf below `node` that `area` can reach.
    /// Recursion is at most `max_depth` deep.
    fn visit_node<F>(&self, node: u32, region: &Region<D>, area: &Aabb<D>, visit: &mut F)
    where
        F: FnMut(EntryId<T, D>, &Entry<T, D>),
    {
        let Some(current) = self.nodes.get(node as usize) else {
            return;
        };
        if current.count == 0 {
            return;
        }
        if current.first_child == NONE {
            let mut next = current.first_entry;
            while let Some(entry) = self.entries.get_at(next) {
                if let Some(id) = self.entries.handle_at(next) {
                    visit(id, entry);
                }
                next = entry.next;
            }
            return;
        }
        for quadrant in 0..Self::CHILDREN {
            if region.child_touches(quadrant, area) {
                self.visit_node(
                    current.first_child + quadrant,
                    &region.child(quadrant),
                    area,
                    visit,
                );
            }
        }
    }

    /// Puts the detached entry `index` into the leaf for `position`,
    /// splitting that leaf while it is over full.
    fn place(&mut self, index: u32, position: &[f32; D]) {
        let mut node = 0u32;
        let mut region = self.root;
        let mut depth = 0u32;
        loop {
            let Some(current) = self.nodes.get_mut(node as usize) else {
                return;
            };
            current.count += 1;
            if current.first_child == NONE {
                break;
            }
            let quadrant = region.quadrant(position);
            node = current.first_child + quadrant;
            region = region.child(quadrant);
            depth += 1;
        }
        self.link(index, node);

        // Split while the leaf holds too much. All entries may fall into one
        // child, which then has to be split in turn.
        loop {
            let Some(current) = self.nodes.get(node as usize) else {
                return;
            };
            if current.count <= self.leaf_size || depth >= self.max_depth {
                return;
            }
            let mut next = current.first_entry;
            let Some(block) = self.take_block(node) else {
                return;
            };
            if let Some(current) = self.nodes.get_mut(node as usize) {
                current.first_child = block;
                current.first_entry = NONE;
            }
            while let Some(entry) = self.entries.get_at(next) {
                let after = entry.next;
                let child = block + region.quadrant(&entry.position);
                self.link(next, child);
                if let Some(child) = self.nodes.get_mut(child as usize) {
                    child.count += 1;
                }
                next = after;
            }
            let crowded = (0..Self::CHILDREN).find(|quadrant| {
                self.nodes
                    .get((block + quadrant) as usize)
                    .is_some_and(|child| child.count > self.leaf_size)
            });
            let Some(quadrant) = crowded else {
                return;
            };
            node = block + quadrant;
            region = region.child(quadrant);
            depth += 1;
        }
    }

    /// Takes entry `index` out of the tree's structure (the entry itself
    /// stays in the pool), merging nodes that became sparse.
    fn detach(&mut self, index: u32) {
        let Some(leaf) = self.entries.get_at(index).map(|entry| entry.node) else {
            return;
        };
        self.unlink(index);
        // Walk up, fixing the counts, and remember the highest split node
        // that is now sparse enough to become a leaf again.
        let merge_at = self.leaf_size / 2;
        let mut merge = NONE;
        let mut node = leaf;
        while let Some(current) = self.nodes.get_mut(node as usize) {
            current.count = current.count.saturating_sub(1);
            if current.first_child != NONE && current.count <= merge_at {
                merge = node;
            }
            node = current.parent;
        }
        if let Some(current) = self.nodes.get_mut(merge as usize) {
            let block = current.first_child;
            current.first_child = NONE;
            current.first_entry = NONE;
            self.gather(block, merge);
        }
    }

    /// Moves every entry below the child block `block` into leaf `target`
    /// and frees the blocks. Recursion is at most `max_depth` deep.
    fn gather(&mut self, block: u32, target: u32) {
        for quadrant in 0..Self::CHILDREN {
            let Some(child) = self.nodes.get((block + quadrant) as usize).copied() else {
                continue;
            };
            if child.first_child != NONE {
                self.gather(child.first_child, target);
                continue;
            }
            let mut next = child.first_entry;
            while let Some(entry) = self.entries.get_at(next) {
                let after = entry.next;
                self.link(next, target);
                next = after;
            }
        }
        // Free the block: its first node links to the next free block.
        if let Some(first) = self.nodes.get_mut(block as usize) {
            first.first_child = self.free_block;
            self.free_block = block;
            self.live_nodes -= Self::CHILDREN;
        }
    }

    /// Gets a block of `CHILDREN` fresh leaves whose parent is `parent`, or
    /// `None` when the node memory is used up.
    fn take_block(&mut self, parent: u32) -> Option<u32> {
        let block = if self.free_block != NONE {
            let block = self.free_block;
            self.free_block = self.nodes.get(block as usize)?.first_child;
            block
        } else {
            let end = self.used.checked_add(Self::CHILDREN)?;
            if end as usize > self.nodes.len() {
                return None;
            }
            let block = self.used;
            self.used = end;
            block
        };
        let children = self
            .nodes
            .get_mut(block as usize..(block + Self::CHILDREN) as usize)?;
        children.fill(Node::leaf(parent));
        self.live_nodes += Self::CHILDREN;
        Some(block)
    }

    /// Puts entry `index` at the front of the list of leaf `node`. Does not
    /// touch counts.
    fn link(&mut self, index: u32, node: u32) {
        let Some(leaf) = self.nodes.get_mut(node as usize) else {
            return;
        };
        let first = leaf.first_entry;
        leaf.first_entry = index;
        if let Some(entry) = self.entries.get_at_mut(index) {
            entry.node = node;
            entry.prev = NONE;
            entry.next = first;
        }
        if let Some(after) = self.entries.get_at_mut(first) {
            after.prev = index;
        }
    }

    /// Takes entry `index` out of its leaf's list. Does not touch counts.
    fn unlink(&mut self, index: u32) {
        let Some(entry) = self.entries.get_at(index) else {
            return;
        };
        let (node, next, prev) = (entry.node, entry.next, entry.prev);
        if let Some(before) = self.entries.get_at_mut(prev) {
            before.next = next;
        } else if let Some(leaf) = self.nodes.get_mut(node as usize) {
            leaf.first_entry = next;
        }
        if let Some(after) = self.entries.get_at_mut(next) {
            after.prev = prev;
        }
    }
}

impl Node {
    #[inline]
    const fn leaf(parent: u32) -> Self {
        Self {
            first_child: NONE,
            first_entry: NONE,
            count: 0,
            parent,
        }
    }
}
