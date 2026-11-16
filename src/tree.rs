//! Fixed-capacity hierarchy (parent / children / siblings) over
//! user-provided memory.

use core::fmt;

use crate::Full;
use crate::allocator::Allocator;
use crate::pool::{self, Handle, Pool};
use crate::storage::{Dynamic, Inline, IntoBuffer, Storage};

/// "No node" in a link.
const NONE: u32 = u32::MAX;

/// A value in a [`Tree`] together with its links. Opaque; it appears in the
/// types [`Slot`] and [`NodeId`].
pub struct Node<T> {
    value: T,
    // Links are slot indices of live nodes, or `NONE`.
    parent: u32,
    first_child: u32,
    last_child: u32,
    prev: u32,
    next: u32,
}

/// One slot of tree storage. It only exists so the user can size the memory
/// for a [`Tree`]: `Memory::<Slot<T>, N>`.
pub type Slot<T> = pool::Slot<Node<T>>;

/// Identifies a node of a [`Tree`]. `Copy`, 8 bytes, and it goes stale when
/// the node is removed: every lookup with it then finds nothing.
pub type NodeId<T> = Handle<Node<T>>;

/// A child could not be inserted. Carries the value back to the caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InsertError<T> {
    /// The tree has no room left.
    Full(T),
    /// The parent has been removed.
    Stale(T),
}

impl<T> fmt::Debug for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "Full(..)",
            Self::Stale(_) => "Stale(..)",
        })
    }
}

impl<T> fmt::Display for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "container is full",
            Self::Stale(_) => "parent node no longer exists",
        })
    }
}

impl<T> core::error::Error for InsertError<T> {}

/// A node could not be moved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveError {
    /// The node or the new parent has been removed.
    Stale,
    /// The new parent is the node itself or one of its descendants.
    Cycle,
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stale => "node no longer exists",
            Self::Cycle => "node cannot be moved under itself",
        })
    }
}

impl core::error::Error for MoveError {}

/// A fixed-capacity hierarchy of values that lives in memory provided by the
/// caller: every node has an optional parent and an ordered list of
/// children. Nodes without a parent are roots; there can be several.
///
/// Use it for scene graphs, UI trees and transform hierarchies.
///
/// ```
/// use vas::tree::Slot;
/// use vas::{Memory, Tree};
///
/// let mut memory = Memory::<Slot<&str>, 16>::new();
/// let mut tree = Tree::new(&mut memory);
///
/// if let Ok(body) = tree.insert_root("body") {
///     let arm = tree.insert_child(body, "arm");
///     assert!(arm.is_ok());
///     assert!(tree.insert_child(body, "leg").is_ok());
///     assert_eq!(tree.children(body).count(), 2);
///     if let Ok(arm) = arm {
///         assert_eq!(tree.parent(arm), Some(body));
///     }
/// }
/// ```
pub struct Tree<T, S: Storage<Slot<T>>> {
    // Invariant: all links refer to live nodes and are consistent with each
    // other (a node is in exactly one sibling list: its parent's, or the
    // root list).
    nodes: Pool<Node<T>, S>,
    first_root: u32,
    last_root: u32,
}

impl<T, const N: usize> Tree<T, Inline<N>> {
    /// Creates an empty tree with room for `N` nodes inside itself.
    ///
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            nodes: Pool::inline(),
            first_root: NONE,
            last_root: NONE,
        }
    }
}

impl<T, const N: usize> Default for Tree<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<T, A: Allocator + Default> Default for Tree<T, Dynamic<A>> {
    /// Creates an empty tree that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<T, S: Storage<Slot<T>>> Tree<T, S> {
    /// Creates an empty tree on `buffer`: a reference to memory borrows it,
    /// a `Memory` passed by value is stored inside the tree.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Slot<T>, Storage = S>,
    {
        Self {
            nodes: Pool::new(buffer),
            first_root: NONE,
            last_root: NONE,
        }
    }

    /// Number of nodes in the tree.
    #[inline]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Maximum number of nodes the tree can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    /// `true` when the tree holds no nodes.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `true` when no more nodes can be inserted.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.nodes.is_full()
    }

    /// Adds a node without a parent, after the existing roots.
    pub fn insert_root(&mut self, value: T) -> Result<NodeId<T>, Full<T>> {
        let id = self
            .nodes
            .insert(Node::new(value))
            .map_err(|Full(node)| Full(node.value))?;
        self.link_last(NONE, id.index());
        Ok(id)
    }

    /// Adds a node as the last child of `parent`.
    pub fn insert_child(
        &mut self,
        parent: NodeId<T>,
        value: T,
    ) -> Result<NodeId<T>, InsertError<T>> {
        if !self.nodes.contains(parent) {
            return Err(InsertError::Stale(value));
        }
        let id = self
            .nodes
            .insert(Node::new(value))
            .map_err(|Full(node)| InsertError::Full(node.value))?;
        self.link_last(parent.index(), id.index());
        Ok(id)
    }

    /// Removes `id` **and all of its descendants**. Returns the value of
    /// `id`; the descendants' values are dropped.
    pub fn remove(&mut self, id: NodeId<T>) -> Option<T> {
        if !self.nodes.contains(id) {
            return None;
        }
        let top = id.index();
        self.unlink(top);
        // Repeatedly remove the deepest first descendant, so no node is
        // removed before its children and no extra memory is needed.
        let mut current = top;
        loop {
            loop {
                let first_child = self.nodes.get_at(current)?.first_child;
                if first_child == NONE {
                    break;
                }
                current = first_child;
            }
            let handle = self.nodes.handle_at(current)?;
            if current == top {
                return self.nodes.remove(handle).map(|node| node.value);
            }
            let parent = self.nodes.get_at(current)?.parent;
            self.unlink(current);
            drop(self.nodes.remove(handle));
            current = parent;
        }
    }

    /// Moves `id`, with its whole subtree, to be the last child of
    /// `parent`, or the last root when `parent` is `None`.
    pub fn set_parent(
        &mut self,
        id: NodeId<T>,
        parent: Option<NodeId<T>>,
    ) -> Result<(), MoveError> {
        if !self.nodes.contains(id) {
            return Err(MoveError::Stale);
        }
        let target = match parent {
            Some(parent) => {
                if !self.nodes.contains(parent) {
                    return Err(MoveError::Stale);
                }
                // The new parent must not be inside the subtree being moved.
                let mut ancestor = parent.index();
                while ancestor != NONE {
                    if ancestor == id.index() {
                        return Err(MoveError::Cycle);
                    }
                    ancestor = self.link(ancestor, |node| node.parent);
                }
                parent.index()
            }
            None => NONE,
        };
        self.unlink(id.index());
        self.link_last(target, id.index());
        Ok(())
    }

    /// Removes and drops all nodes. Every existing id goes stale.
    #[inline]
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.first_root = NONE;
        self.last_root = NONE;
    }

    /// The value of node `id`.
    #[inline]
    pub fn get(&self, id: NodeId<T>) -> Option<&T> {
        self.nodes.get(id).map(|node| &node.value)
    }

    /// The value of node `id`, mutably.
    #[inline]
    pub fn get_mut(&mut self, id: NodeId<T>) -> Option<&mut T> {
        self.nodes.get_mut(id).map(|node| &mut node.value)
    }

    /// `true` when `id` still refers to a node of the tree.
    #[inline]
    pub fn contains(&self, id: NodeId<T>) -> bool {
        self.nodes.contains(id)
    }

    /// The parent of `id`; `None` for a root.
    #[inline]
    pub fn parent(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.neighbor(id, |node| node.parent)
    }

    /// The first child of `id`.
    #[inline]
    pub fn first_child(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.neighbor(id, |node| node.first_child)
    }

    /// The last child of `id`.
    #[inline]
    pub fn last_child(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.neighbor(id, |node| node.last_child)
    }

    /// The sibling after `id` (for a root: the next root).
    #[inline]
    pub fn next_sibling(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.neighbor(id, |node| node.next)
    }

    /// The sibling before `id` (for a root: the previous root).
    #[inline]
    pub fn prev_sibling(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.neighbor(id, |node| node.prev)
    }

    /// Number of ancestors of `id`: 0 for a root.
    #[inline]
    pub fn depth(&self, id: NodeId<T>) -> Option<usize> {
        self.nodes.contains(id).then(|| self.ancestors(id).count())
    }

    /// Iterates over the roots, in order.
    #[inline]
    pub fn roots(&self) -> Siblings<'_, T, S> {
        Siblings {
            tree: self,
            next: self.first_root,
        }
    }

    /// Iterates over the direct children of `id`, in order.
    #[inline]
    pub fn children(&self, id: NodeId<T>) -> Siblings<'_, T, S> {
        Siblings {
            tree: self,
            next: self.nodes.get(id).map_or(NONE, |node| node.first_child),
        }
    }

    /// Iterates over the parent of `id`, then its parent, up to the root.
    #[inline]
    pub fn ancestors(&self, id: NodeId<T>) -> Ancestors<'_, T, S> {
        Ancestors {
            tree: self,
            next: self.nodes.get(id).map_or(NONE, |node| node.parent),
        }
    }

    /// Iterates over everything below `id`, depth first: each node before
    /// its own children, children in order. `id` itself is not included.
    #[inline]
    pub fn descendants(&self, id: NodeId<T>) -> Descendants<'_, T, S> {
        Descendants {
            tree: self,
            top: id.index(),
            next: self.nodes.get(id).map_or(NONE, |node| node.first_child),
        }
    }

    /// Iterates over all nodes and their values, in no particular order.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (NodeId<T>, &T)> {
        self.nodes.iter().map(|(id, node)| (id, &node.value))
    }

    /// Iterates over all nodes with mutable access to their values, in no
    /// particular order.
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (NodeId<T>, &mut T)> {
        self.nodes
            .iter_mut()
            .map(|(id, node)| (id, &mut node.value))
    }

    /// Follows one link of the live node in slot `index`.
    #[inline]
    fn link(&self, index: u32, pick: impl Fn(&Node<T>) -> u32) -> u32 {
        self.nodes.get_at(index).map_or(NONE, pick)
    }

    /// Follows one link of `id` and turns the result into an id.
    #[inline]
    fn neighbor(&self, id: NodeId<T>, pick: impl Fn(&Node<T>) -> u32) -> Option<NodeId<T>> {
        self.nodes.handle_at(pick(self.nodes.get(id)?))
    }

    /// Appends the detached node `index` to the children of `parent`, or to
    /// the roots when `parent` is `NONE`.
    fn link_last(&mut self, parent: u32, index: u32) {
        let last = if parent == NONE {
            self.last_root
        } else {
            self.link(parent, |node| node.last_child)
        };
        if let Some(node) = self.nodes.get_at_mut(index) {
            node.parent = parent;
            node.prev = last;
            node.next = NONE;
        }
        if let Some(before) = self.nodes.get_at_mut(last) {
            before.next = index;
        } else {
            self.set_first(parent, index);
        }
        self.set_last(parent, index);
    }

    /// Takes node `index` out of its sibling list. Its own children stay
    /// attached to it.
    fn unlink(&mut self, index: u32) {
        let Some(node) = self.nodes.get_at_mut(index) else {
            return;
        };
        let (parent, prev, next) = (node.parent, node.prev, node.next);
        node.parent = NONE;
        node.prev = NONE;
        node.next = NONE;
        if let Some(before) = self.nodes.get_at_mut(prev) {
            before.next = next;
        } else {
            self.set_first(parent, next);
        }
        if let Some(after) = self.nodes.get_at_mut(next) {
            after.prev = prev;
        } else {
            self.set_last(parent, prev);
        }
    }

    fn set_first(&mut self, parent: u32, index: u32) {
        match self.nodes.get_at_mut(parent) {
            Some(node) => node.first_child = index,
            None => self.first_root = index,
        }
    }

    fn set_last(&mut self, parent: u32, index: u32) {
        match self.nodes.get_at_mut(parent) {
            Some(node) => node.last_child = index,
            None => self.last_root = index,
        }
    }
}

impl<T> Node<T> {
    #[inline]
    const fn new(value: T) -> Self {
        Self {
            value,
            parent: NONE,
            first_child: NONE,
            last_child: NONE,
            prev: NONE,
            next: NONE,
        }
    }
}

/// Iterator over a list of siblings. See [`Tree::children`] and
/// [`Tree::roots`].
pub struct Siblings<'t, T, S: Storage<Slot<T>>> {
    tree: &'t Tree<T, S>,
    next: u32,
}

impl<T, S: Storage<Slot<T>>> Iterator for Siblings<'_, T, S> {
    type Item = NodeId<T>;

    #[inline]
    fn next(&mut self) -> Option<NodeId<T>> {
        let id = self.tree.nodes.handle_at(self.next)?;
        self.next = self.tree.link(self.next, |node| node.next);
        Some(id)
    }
}

/// Iterator from a node's parent up to its root. See [`Tree::ancestors`].
pub struct Ancestors<'t, T, S: Storage<Slot<T>>> {
    tree: &'t Tree<T, S>,
    next: u32,
}

impl<T, S: Storage<Slot<T>>> Iterator for Ancestors<'_, T, S> {
    type Item = NodeId<T>;

    #[inline]
    fn next(&mut self) -> Option<NodeId<T>> {
        let id = self.tree.nodes.handle_at(self.next)?;
        self.next = self.tree.link(self.next, |node| node.parent);
        Some(id)
    }
}

/// Depth-first iterator over everything below a node. See
/// [`Tree::descendants`].
pub struct Descendants<'t, T, S: Storage<Slot<T>>> {
    tree: &'t Tree<T, S>,
    top: u32,
    next: u32,
}

impl<T, S: Storage<Slot<T>>> Iterator for Descendants<'_, T, S> {
    type Item = NodeId<T>;

    fn next(&mut self) -> Option<NodeId<T>> {
        let id = self.tree.nodes.handle_at(self.next)?;
        let node = self.tree.nodes.get_at(self.next)?;
        self.next = if node.first_child != NONE {
            node.first_child
        } else {
            // No children: go to the next sibling, climbing as far as needed
            // but never above the starting node.
            let mut current = self.next;
            loop {
                if current == self.top || current == NONE {
                    break NONE;
                }
                let next = self.tree.link(current, |node| node.next);
                if next != NONE {
                    break next;
                }
                current = self.tree.link(current, |node| node.parent);
            }
        };
        Some(id)
    }
}
