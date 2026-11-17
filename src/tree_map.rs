//! Ordered map as a red-black tree over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::borrow::Borrow;
use core::cmp::Ordering;
use core::marker::PhantomData;
use core::ops::{Bound, RangeBounds};
use core::{fmt, mem};

use crate::allocator::Allocator;
use crate::storage::{Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// "No node". Never a valid node index: a map holds fewer than `NONE`
/// entries.
const NONE: u32 = 0x7FFF_FFFF;

/// Set in `Node::parent` while the node is red.
const RED: u32 = 0x8000_0000;

/// One entry of a [`TreeMap`] with its links. Opaque; it only exists so the
/// user can size the memory for a map: `Memory::<Node<K, V>, N>`.
///
/// It is the key, the value and 12 bytes of links.
#[derive(Clone)]
pub struct Node<K, V> {
    key: K,
    value: V,
    // Index of the parent in the low 31 bits (`NONE` at the root); the top
    // bit is the colour.
    parent: u32,
    left: u32,
    right: u32,
}

/// A map that keeps its entries sorted by key, as a red-black tree in
/// memory provided by the caller.
///
/// Insert, remove and lookup are O(log n) at any size. Use it for large
/// ordered maps that change often; for small maps, or maps that are mostly
/// read, [`OrderedMap`](crate::OrderedMap) is faster and smaller.
///
/// ```
/// use vas::tree_map::Node;
/// use vas::{Memory, TreeMap};
///
/// let mut memory = Memory::<Node<u32, &str>, 16>::new();
/// let mut events = TreeMap::new(&mut memory);
///
/// assert_eq!(events.insert(20, "explode"), Ok(None));
/// assert_eq!(events.insert(5, "spawn"), Ok(None));
/// assert_eq!(events.insert(10, "arm"), Ok(None));
///
/// // Sorted by key, whatever the insertion order.
/// assert_eq!(events.first(), Some((&5, &"spawn")));
/// // The next event at or after time 7.
/// assert_eq!(events.at_or_after(&7), Some((&10, &"arm")));
/// assert_eq!(events.pop_first(), Some((5, "spawn")));
/// ```
///
/// With [`Dynamic`] storage the capacity is not fixed: the memory comes
/// from an allocator and the map grows when it runs out.
pub struct TreeMap<K, V, S: Storage<Node<K, V>>> {
    // Invariants:
    // - the nodes are exactly the entries, packed at the front of the
    //   block in no particular order; links are indices into it.
    // - binary search tree: keys in a node's left subtree are smaller and
    //   keys in its right subtree greater than its own.
    // - red-black: the root is black, a red node has no red child, and
    //   every path from the root to a missing child crosses the same
    //   number of black nodes.
    nodes: Vector<Node<K, V>, S>,
    root: u32,
}

impl<K, V, const N: usize> TreeMap<K, V, Inline<N>> {
    /// Creates an empty map with room for `N` entries inside itself.
    ///
    /// Usable in `const` and `static` items.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            nodes: Vector::inline(),
            root: NONE,
        }
    }
}

impl<K, V, const N: usize> Default for TreeMap<K, V, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Only maps with inline storage can be cloned.
impl<K: Clone, V: Clone, const N: usize> Clone for TreeMap<K, V, Inline<N>> {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            root: self.root,
        }
    }
}

impl<K, V, A: Allocator + Default> Default for TreeMap<K, V, Dynamic<A>> {
    /// An empty map that takes memory from `A::default()` on first use.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<K, V, S: Storage<Node<K, V>>> TreeMap<K, V, S> {
    /// Creates an empty map on `buffer`: a reference to memory borrows it,
    /// a `Memory` passed by value is stored inside the map.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Node<K, V>, Storage = S>,
    {
        Self {
            nodes: Vector::new(buffer),
            root: NONE,
        }
    }

    /// Number of entries in the map.
    #[inline]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Number of entries the map can hold without growing. For borrowed
    /// and inline storage this is the maximum. Never more than 2^31 - 1.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.nodes.capacity().min(NONE as usize)
    }

    /// Number of entries that can still be inserted without growing.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.capacity() - self.len()
    }

    /// Makes sure `additional` more entries fit, growing dynamic storage
    /// when needed. Fails without changes when the storage is fixed and too
    /// small, or when the allocator refuses.
    #[inline]
    pub fn reserve(&mut self, additional: usize) -> Result<(), Full<()>> {
        match self.len().checked_add(additional) {
            Some(total) if total <= NONE as usize => self.nodes.reserve(additional),
            _ => Err(Full(())),
        }
    }

    /// `true` when the map holds no entries.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `true` when the next insert of a new key needs more memory: it fails
    /// with borrowed and inline storage, and grows with dynamic storage.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity()
    }

    /// The entry with the smallest key. O(log n).
    #[inline]
    pub fn first(&self) -> Option<(&K, &V)> {
        let nodes = self.nodes.as_slice();
        entry(nodes, outermost(nodes, self.root, |node| node.left))
    }

    /// The entry with the greatest key. O(log n).
    #[inline]
    pub fn last(&self) -> Option<(&K, &V)> {
        let nodes = self.nodes.as_slice();
        entry(nodes, outermost(nodes, self.root, |node| node.right))
    }

    /// Removes and returns the entry with the smallest key. O(log n).
    #[inline]
    pub fn pop_first(&mut self) -> Option<(K, V)> {
        let first = outermost(self.nodes.as_slice(), self.root, |node| node.left);
        self.remove_at(first)
    }

    /// Removes and returns the entry with the greatest key. O(log n).
    #[inline]
    pub fn pop_last(&mut self) -> Option<(K, V)> {
        let last = outermost(self.nodes.as_slice(), self.root, |node| node.right);
        self.remove_at(last)
    }

    /// Removes and drops all entries. O(n) when keys or values need
    /// dropping, O(1) otherwise.
    #[inline]
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = NONE;
    }

    /// Iterates over all entries in key order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        let nodes = self.nodes.as_slice();
        Iter {
            nodes,
            front: outermost(nodes, self.root, |node| node.left),
            back: outermost(nodes, self.root, |node| node.right),
            done: nodes.is_empty(),
        }
    }

    /// Iterates over all entries in key order, with mutable access to the
    /// values.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        let nodes = self.nodes.as_slice();
        let front = outermost(nodes, self.root, |node| node.left);
        let back = outermost(nodes, self.root, |node| node.right);
        let nodes = self.nodes.as_mut_slice();
        IterMut {
            len: nodes.len(),
            nodes: nodes.as_mut_ptr(),
            front,
            back,
            done: nodes.is_empty(),
            marker: PhantomData,
        }
    }

    #[inline]
    fn node(&self, index: u32) -> Option<&Node<K, V>> {
        self.nodes.as_slice().get(index as usize)
    }

    #[inline]
    fn node_mut(&mut self, index: u32) -> Option<&mut Node<K, V>> {
        self.nodes.as_mut_slice().get_mut(index as usize)
    }

    /// Left child of `index`; `NONE` when there is none or `index` is.
    #[inline]
    fn left(&self, index: u32) -> u32 {
        self.node(index).map_or(NONE, |node| node.left)
    }

    #[inline]
    fn right(&self, index: u32) -> u32 {
        self.node(index).map_or(NONE, |node| node.right)
    }

    #[inline]
    fn parent(&self, index: u32) -> u32 {
        self.node(index).map_or(NONE, |node| node.parent & NONE)
    }

    /// `false` for black nodes and for `NONE`, which counts as black.
    #[inline]
    fn is_red(&self, index: u32) -> bool {
        self.node(index).is_some_and(|node| node.parent & RED != 0)
    }

    #[inline]
    fn set_left(&mut self, index: u32, left: u32) {
        if let Some(node) = self.node_mut(index) {
            node.left = left;
        }
    }

    #[inline]
    fn set_right(&mut self, index: u32, right: u32) {
        if let Some(node) = self.node_mut(index) {
            node.right = right;
        }
    }

    /// Sets the parent link of `index`, keeping its colour. Does nothing
    /// for `NONE`.
    #[inline]
    fn set_parent(&mut self, index: u32, parent: u32) {
        if let Some(node) = self.node_mut(index) {
            node.parent = (node.parent & RED) | parent;
        }
    }

    #[inline]
    fn set_red(&mut self, index: u32, red: bool) {
        if let Some(node) = self.node_mut(index) {
            node.parent = (node.parent & NONE) | if red { RED } else { 0 };
        }
    }

    /// Makes `new` the child of `parent` in the place of `old`; with no
    /// parent, `new` becomes the root.
    #[inline]
    fn replace_child(&mut self, parent: u32, old: u32, new: u32) {
        match self.node_mut(parent) {
            None => self.root = new,
            Some(node) if node.left == old => node.left = new,
            Some(node) => node.right = new,
        }
    }

    /// Puts the subtree at `new` (possibly `NONE`) where `old` hangs.
    #[inline]
    fn transplant(&mut self, old: u32, new: u32) {
        let parent = self.parent(old);
        self.replace_child(parent, old, new);
        self.set_parent(new, parent);
    }

    /// Lifts the right child of `top` above it.
    fn rotate_left(&mut self, top: u32) {
        let child = self.right(top);
        let inner = self.left(child);
        self.set_right(top, inner);
        self.set_parent(inner, top);
        let parent = self.parent(top);
        self.set_parent(child, parent);
        self.replace_child(parent, top, child);
        self.set_left(child, top);
        self.set_parent(top, child);
    }

    /// Lifts the left child of `top` above it.
    fn rotate_right(&mut self, top: u32) {
        let child = self.left(top);
        let inner = self.right(child);
        self.set_left(top, inner);
        self.set_parent(inner, top);
        let parent = self.parent(top);
        self.set_parent(child, parent);
        self.replace_child(parent, top, child);
        self.set_right(child, top);
        self.set_parent(top, child);
    }

    /// Restores the red-black rules after the red node `node` was linked
    /// in as a leaf.
    fn insert_fixup(&mut self, mut node: u32) {
        while self.is_red(self.parent(node)) {
            let parent = self.parent(node);
            // A red parent is never the root, so there is a grandparent.
            let grandparent = self.parent(parent);
            let parent_is_left = self.left(grandparent) == parent;
            let uncle = if parent_is_left {
                self.right(grandparent)
            } else {
                self.left(grandparent)
            };
            if self.is_red(uncle) {
                self.set_red(parent, false);
                self.set_red(uncle, false);
                self.set_red(grandparent, true);
                node = grandparent;
                continue;
            }
            // Black uncle: one or two rotations finish the job.
            let mut top = parent;
            if parent_is_left {
                if self.right(parent) == node {
                    self.rotate_left(parent);
                    top = node;
                }
                self.rotate_right(grandparent);
            } else {
                if self.left(parent) == node {
                    self.rotate_right(parent);
                    top = node;
                }
                self.rotate_left(grandparent);
            }
            self.set_red(top, false);
            self.set_red(grandparent, true);
            break;
        }
        self.set_red(self.root, false);
    }

    /// Restores the red-black rules after a black node was unlinked.
    /// `node` (possibly `NONE`) took its place under `parent` and is one
    /// black short.
    fn remove_fixup(&mut self, mut node: u32, mut parent: u32) {
        while node != self.root && !self.is_red(node) {
            let node_is_left = self.left(parent) == node;
            // The sibling exists: its side has at least one black more.
            let mut sibling = if node_is_left {
                self.right(parent)
            } else {
                self.left(parent)
            };
            if self.is_red(sibling) {
                self.set_red(sibling, false);
                self.set_red(parent, true);
                if node_is_left {
                    self.rotate_left(parent);
                    sibling = self.right(parent);
                } else {
                    self.rotate_right(parent);
                    sibling = self.left(parent);
                }
            }
            let (near, far) = if node_is_left {
                (self.left(sibling), self.right(sibling))
            } else {
                (self.right(sibling), self.left(sibling))
            };
            if !self.is_red(near) && !self.is_red(far) {
                // Push the shortage one level up.
                self.set_red(sibling, true);
                node = parent;
                parent = self.parent(node);
                continue;
            }
            if !self.is_red(far) {
                // Turn the red near nephew into a red far nephew.
                self.set_red(near, false);
                self.set_red(sibling, true);
                if node_is_left {
                    self.rotate_right(sibling);
                    sibling = self.right(parent);
                } else {
                    self.rotate_left(sibling);
                    sibling = self.left(parent);
                }
            }
            self.set_red(sibling, self.is_red(parent));
            self.set_red(parent, false);
            if node_is_left {
                self.set_red(self.right(sibling), false);
                self.rotate_left(parent);
            } else {
                self.set_red(self.left(sibling), false);
                self.rotate_right(parent);
            }
            node = self.root;
        }
        self.set_red(node, false);
    }

    /// Unlinks the node at `index`, frees its slot and returns its entry.
    fn remove_at(&mut self, index: u32) -> Option<(K, V)> {
        let left = self.node(index)?.left;
        let right = self.right(index);
        let mut removed_black = !self.is_red(index);
        // The node that ends up one black short, and its parent.
        let (child, child_parent);
        if left == NONE {
            child = right;
            child_parent = self.parent(index);
            self.transplant(index, right);
        } else if right == NONE {
            child = left;
            child_parent = self.parent(index);
            self.transplant(index, left);
        } else {
            // Two children: the next entry in key order takes this place
            // and colour, and is itself unlinked from further down.
            let next = outermost(self.nodes.as_slice(), right, |node| node.left);
            let colour = self.is_red(index);
            removed_black = !self.is_red(next);
            child = self.right(next);
            if self.parent(next) == index {
                child_parent = next;
            } else {
                child_parent = self.parent(next);
                self.transplant(next, child);
                self.set_right(next, right);
                self.set_parent(right, next);
            }
            self.transplant(index, next);
            self.set_left(next, left);
            self.set_parent(left, next);
            self.set_red(next, colour);
        }
        if removed_black {
            self.remove_fixup(child, child_parent);
        }

        // Free the slot. The last node moves into it, so the links that
        // pointed at the last node are redirected.
        let last = (self.nodes.len() - 1) as u32;
        let node = self.nodes.swap_remove(index as usize)?;
        if index != last {
            let parent = self.parent(index);
            self.replace_child(parent, last, index);
            self.set_parent(self.left(index), index);
            self.set_parent(self.right(index), index);
        }
        Some((node.key, node.value))
    }
}

impl<K: Ord, V, S: Storage<Node<K, V>>> TreeMap<K, V, S> {
    /// Inserts `value` under `key`. O(log n).
    ///
    /// Returns `Ok(Some(old))` when the key was already present (the value
    /// is replaced, the existing key is kept), `Ok(None)` when it is new, and
    /// hands both back in [`Full`] when the key is new and there is no room.
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, Full<(K, V)>> {
        let mut parent = NONE;
        let mut goes_left = false;
        let mut current = self.root;
        while let Some(node) = self.node_mut(current) {
            parent = current;
            let order = key.cmp(&node.key);
            if order == Ordering::Equal {
                return Ok(Some(mem::replace(&mut node.value, value)));
            }
            // Picked without a branch, as in `find`.
            goes_left = order == Ordering::Less;
            current = [node.right, node.left][usize::from(goes_left)];
        }
        let index = self.nodes.len();
        if index >= NONE as usize {
            return Err(Full((key, value)));
        }
        let index = index as u32;
        self.nodes
            .push(Node {
                key,
                value,
                parent: parent | RED,
                left: NONE,
                right: NONE,
            })
            .map_err(|Full(node)| Full((node.key, node.value)))?;
        if parent == NONE {
            self.root = index;
        } else if goes_left {
            self.set_left(parent, index);
        } else {
            self.set_right(parent, index);
        }
        self.insert_fixup(index);
        Ok(None)
    }

    /// The value stored under `key`. O(log n).
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.node(self.find(key)).map(|node| &node.value)
    }

    /// The value stored under `key`, mutably. O(log n).
    #[inline]
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let index = self.find(key);
        self.node_mut(index).map(|node| &mut node.value)
    }

    /// `true` when the map has an entry for `key`. O(log n).
    #[inline]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.find(key) != NONE
    }

    /// Removes the entry for `key` and returns its value. O(log n).
    #[inline]
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.remove_entry(key).map(|(_, value)| value)
    }

    /// Removes the entry for `key` and returns its key and value. O(log n).
    #[inline]
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let index = self.find(key);
        self.remove_at(index)
    }

    /// The entry with the greatest key that is not above `key`. O(log n).
    ///
    /// For keyframes or timed events: the one in effect at `key`.
    #[inline]
    pub fn at_or_before<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        entry(self.nodes.as_slice(), self.last_where(|k| k <= key))
    }

    /// The entry with the smallest key that is not below `key`. O(log n).
    #[inline]
    pub fn at_or_after<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        entry(self.nodes.as_slice(), self.first_where(|k| k >= key))
    }

    /// Iterates, in key order, over the entries whose keys fall in `range`
    /// (`a..b`, `a..=b`, `..b`, `a..`, `..`). An inverted range is empty.
    pub fn range<Q, R>(&self, range: R) -> Iter<'_, K, V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
        R: RangeBounds<Q>,
    {
        let front = match range.start_bound() {
            Bound::Included(key) => self.first_where(|k| k >= key),
            Bound::Excluded(key) => self.first_where(|k| k > key),
            Bound::Unbounded => self.first_where(|_| true),
        };
        let back = match range.end_bound() {
            Bound::Included(key) => self.last_where(|k| k <= key),
            Bound::Excluded(key) => self.last_where(|k| k < key),
            Bound::Unbounded => self.last_where(|_| true),
        };
        // Empty when either end is missing or the ends have crossed.
        let done = match (self.node(front), self.node(back)) {
            (Some(front), Some(back)) => front.key > back.key,
            _ => true,
        };
        Iter {
            nodes: self.nodes.as_slice(),
            front,
            back,
            done,
        }
    }

    /// Index of the node for `key`, or `NONE`.
    #[inline]
    fn find<Q>(&self, key: &Q) -> u32
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let mut current = self.root;
        while let Some(node) = self.node(current) {
            let order = key.cmp(node.key.borrow());
            if order == Ordering::Equal {
                return current;
            }
            // Picked without a branch: which way a lookup goes is close to
            // random, so a branch here would be mispredicted half the time.
            current = [node.left, node.right][usize::from(order == Ordering::Greater)];
        }
        NONE
    }

    /// Index of the first node, in key order, whose key satisfies `test`,
    /// or `NONE`. `test` must hold for all keys from some key on.
    #[inline]
    fn first_where<Q>(&self, test: impl Fn(&Q) -> bool) -> u32
    where
        K: Borrow<Q>,
        Q: ?Sized,
    {
        let mut found = NONE;
        let mut current = self.root;
        while let Some(node) = self.node(current) {
            if test(node.key.borrow()) {
                found = current;
                current = node.left;
            } else {
                current = node.right;
            }
        }
        found
    }

    /// Index of the last node, in key order, whose key satisfies `test`,
    /// or `NONE`. `test` must hold for all keys up to some key.
    #[inline]
    fn last_where<Q>(&self, test: impl Fn(&Q) -> bool) -> u32
    where
        K: Borrow<Q>,
        Q: ?Sized,
    {
        let mut found = NONE;
        let mut current = self.root;
        while let Some(node) = self.node(current) {
            if test(node.key.borrow()) {
                found = current;
                current = node.right;
            } else {
                current = node.left;
            }
        }
        found
    }
}

/// Follows `step` (a child link) from `index` as far as it goes. `NONE`
/// when `index` is.
#[inline]
fn outermost<K, V>(nodes: &[Node<K, V>], index: u32, step: impl Fn(&Node<K, V>) -> u32) -> u32 {
    let mut current = index;
    while let Some(node) = nodes.get(current as usize) {
        let next = step(node);
        if next == NONE {
            break;
        }
        current = next;
    }
    current
}

#[inline]
fn entry<K, V>(nodes: &[Node<K, V>], index: u32) -> Option<(&K, &V)> {
    nodes
        .get(index as usize)
        .map(|node| (&node.key, &node.value))
}

/// One of the three links of a node.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Link {
    Parent,
    Left,
    Right,
}

/// The node after (`forward`) or before `index` in key order; `NONE` at
/// the end. `link(index, which)` reads a link of a node and returns `NONE`
/// for an index that is out of range.
fn neighbour(link: impl Fn(u32, Link) -> u32, index: u32, forward: bool) -> u32 {
    let (toward, away) = if forward {
        (Link::Right, Link::Left)
    } else {
        (Link::Left, Link::Right)
    };
    let mut current = link(index, toward);
    if current != NONE {
        // Down one step, then as far back as possible.
        loop {
            let next = link(current, away);
            if next == NONE {
                return current;
            }
            current = next;
        }
    }
    // Up until we arrive from the far side.
    current = index;
    loop {
        let parent = link(current, Link::Parent);
        if parent == NONE || link(parent, toward) != current {
            return parent;
        }
        current = parent;
    }
}

impl<K: fmt::Debug, V: fmt::Debug, S: Storage<Node<K, V>>> fmt::Debug for TreeMap<K, V, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'m, K, V, S: Storage<Node<K, V>>> IntoIterator for &'m TreeMap<K, V, S> {
    type Item = (&'m K, &'m V);
    type IntoIter = Iter<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'m, K, V, S: Storage<Node<K, V>>> IntoIterator for &'m mut TreeMap<K, V, S> {
    type Item = (&'m K, &'m mut V);
    type IntoIter = IterMut<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Iterator over the entries of a [`TreeMap`] in key order. Created by
/// [`TreeMap::iter`] and [`TreeMap::range`].
pub struct Iter<'m, K, V> {
    nodes: &'m [Node<K, V>],
    // The next entries to yield from the front and from the back.
    front: u32,
    back: u32,
    // Set once `front` and `back` have met and that entry was yielded.
    done: bool,
}

impl<K, V> Iter<'_, K, V> {
    #[inline]
    fn link(&self, index: u32, which: Link) -> u32 {
        self.nodes
            .get(index as usize)
            .map_or(NONE, |node| match which {
                Link::Parent => node.parent & NONE,
                Link::Left => node.left,
                Link::Right => node.right,
            })
    }
}

impl<'m, K, V> Iterator for Iter<'m, K, V> {
    type Item = (&'m K, &'m V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let current = self.front;
        if current == self.back {
            self.done = true;
        } else {
            self.front = neighbour(|index, which| self.link(index, which), current, true);
        }
        entry(self.nodes, current)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.done {
            (0, Some(0))
        } else {
            (1, Some(self.nodes.len()))
        }
    }
}

impl<K, V> DoubleEndedIterator for Iter<'_, K, V> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let current = self.back;
        if current == self.front {
            self.done = true;
        } else {
            self.back = neighbour(|index, which| self.link(index, which), current, false);
        }
        entry(self.nodes, current)
    }
}

/// Iterator over the entries of a [`TreeMap`] in key order, with mutable
/// access to the values. Created by [`TreeMap::iter_mut`].
pub struct IterMut<'m, K, V> {
    // The map's nodes, borrowed exclusively for `'m`.
    nodes: *mut Node<K, V>,
    len: usize,
    front: u32,
    back: u32,
    done: bool,
    marker: PhantomData<&'m mut [Node<K, V>]>,
}

impl<'m, K, V> IterMut<'m, K, V> {
    /// Reads a link without borrowing the node, whose value may be
    /// borrowed by the caller of the iterator.
    #[inline]
    fn link(&self, index: u32, which: Link) -> u32 {
        if index as usize >= self.len {
            return NONE;
        }
        // SAFETY: the index is in range of the block of `len` initialised
        // nodes. Only the link field is read, through a raw pointer; the
        // iterator hands out references to keys and values only, so the
        // link fields are never borrowed.
        unsafe {
            let node = self.nodes.add(index as usize);
            match which {
                Link::Parent => (&raw const (*node).parent).read() & NONE,
                Link::Left => (&raw const (*node).left).read(),
                Link::Right => (&raw const (*node).right).read(),
            }
        }
    }

    /// The entry at `index`, which must not have been yielded before.
    #[inline]
    fn entry(&mut self, index: u32) -> Option<(&'m K, &'m mut V)> {
        if index as usize >= self.len {
            return None;
        }
        // SAFETY: the index is in range of the block borrowed for `'m`.
        // Every node is yielded at most once (the iterator stops when
        // `front` and `back` meet), so no two references to the same key
        // or value exist. The link fields, which the iterator keeps
        // reading, are not part of what is handed out.
        unsafe {
            let node = self.nodes.add(index as usize);
            Some((&(*node).key, &mut (*node).value))
        }
    }
}

impl<'m, K, V> Iterator for IterMut<'m, K, V> {
    type Item = (&'m K, &'m mut V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let current = self.front;
        if current == self.back {
            self.done = true;
        } else {
            self.front = neighbour(|index, which| self.link(index, which), current, true);
        }
        self.entry(current)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.done {
            (0, Some(0))
        } else {
            (1, Some(self.len))
        }
    }
}

impl<K, V> DoubleEndedIterator for IterMut<'_, K, V> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let current = self.back;
        if current == self.front {
            self.done = true;
        } else {
            self.back = neighbour(|index, which| self.link(index, which), current, false);
        }
        self.entry(current)
    }
}

// SAFETY: an `IterMut` is a `&mut [Node<K, V>]` that only hands out `&K`
// and `&mut V`, so it can cross threads when those can.
unsafe impl<K: Sync, V: Send> Send for IterMut<'_, K, V> {}
// SAFETY: a shared `IterMut` gives access to nothing.
unsafe impl<K: Sync, V: Sync> Sync for IterMut<'_, K, V> {}

#[cfg(test)]
mod tests {
    use super::{NONE, Node, TreeMap};
    use crate::storage::Storage;
    use crate::{Inline, OrderedMap, Rng};

    /// Checks every invariant of the subtree at `index` and returns the
    /// number of black nodes on its paths, or `None` when one is broken.
    fn black_height<S: Storage<Node<u32, u32>>>(
        map: &TreeMap<u32, u32, S>,
        index: u32,
        parent: u32,
        low: Option<u32>,
        high: Option<u32>,
        count: &mut usize,
    ) -> Option<u32> {
        let Some(node) = map.node(index) else {
            return (index == NONE).then_some(0);
        };
        *count += 1;
        let in_order = low.is_none_or(|low| node.key > low) && high.is_none_or(|k| node.key < k);
        let linked = map.parent(index) == parent;
        let red = map.is_red(index);
        let no_red_pair = !(red && (map.is_red(node.left) || map.is_red(node.right)));
        if !(in_order && linked && no_red_pair) {
            return None;
        }
        let left = black_height(map, node.left, index, low, Some(node.key), count)?;
        let right = black_height(map, node.right, index, Some(node.key), high, count)?;
        (left == right).then_some(left + u32::from(!red))
    }

    fn is_valid<S: Storage<Node<u32, u32>>>(map: &TreeMap<u32, u32, S>) -> bool {
        let mut count = 0;
        let balanced = black_height(map, map.root, NONE, None, None, &mut count).is_some();
        balanced && !map.is_red(map.root) && count == map.len()
    }

    #[test]
    fn stays_a_red_black_tree_under_random_operations() {
        let mut rng = Rng::new(7);
        let mut map = TreeMap::<u32, u32, Inline<256>>::inline();
        let mut model = OrderedMap::<u32, u32, Inline<256>>::inline();
        // Fewer steps under Miri, which is slow.
        let steps = if cfg!(miri) { 300 } else { 20_000 };
        for step in 0..steps {
            let key = rng.u32_below(400);
            match rng.u32_below(5) {
                0 | 1 => assert_eq!(map.remove(&key), model.remove(&key)),
                2 if step % 7 == 0 => assert_eq!(map.pop_first(), model.pop_first()),
                2 if step % 7 == 1 => assert_eq!(map.pop_last(), model.pop_last()),
                _ => assert_eq!(map.insert(key, step).ok(), model.insert(key, step).ok()),
            }
            assert!(is_valid(&map));
            assert_eq!(map.len(), model.len());
        }
        assert!(map.iter().eq(model.iter()));
    }

    #[test]
    fn sorted_input_stays_shallow() {
        let mut map = TreeMap::<u32, u32, Inline<1024>>::inline();
        // Fewer keys under Miri, which is slow.
        let keys = if cfg!(miri) { 96 } else { 1024 };
        for key in 0..keys {
            assert_eq!(map.insert(key, key), Ok(None));
        }
        assert!(is_valid(&map));
        let mut count = 0;
        // A red-black tree of n nodes has at most log2(n + 1) black levels.
        let height = black_height(&map, map.root, NONE, None, None, &mut count);
        assert!(height.is_some_and(|height| height <= 11));
        for key in (0..keys).rev() {
            assert_eq!(map.remove(&key), Some(key));
            assert!(is_valid(&map));
        }
        assert!(map.is_empty());
    }
}
