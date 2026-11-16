//! Binary tree over borrowed, inline or growing memory.

use core::fmt;

use crate::Full;
use crate::allocator::Allocator;
use crate::pool::{self, Handle, Pool};
use crate::storage::{Dynamic, Inline, IntoBuffer, Storage};

/// "No node" in a link.
const NONE: u32 = u32::MAX;

/// A value in a [`BinaryTree`] together with its links. Opaque; it appears
/// in the types [`Slot`] and [`NodeId`].
pub struct Node<T> {
    value: T,
    // Links are slot indices of live nodes, or `NONE`.
    parent: u32,
    left: u32,
    right: u32,
}

/// One slot of binary tree storage. It only exists so the user can size the
/// memory for a [`BinaryTree`]: `Memory::<Slot<T>, N>`.
pub type Slot<T> = pool::Slot<Node<T>>;

/// Identifies a node of a [`BinaryTree`]. `Copy`, 8 bytes, and it goes stale
/// when the node is removed.
pub type NodeId<T> = Handle<Node<T>>;

/// A node could not be inserted. Carries the value back to the caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InsertError<T> {
    /// The tree has no room left.
    Full(T),
    /// The parent has been removed.
    Stale(T),
    /// The position (root, left or right child) is already taken.
    Occupied(T),
}

impl<T> fmt::Debug for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "Full(..)",
            Self::Stale(_) => "Stale(..)",
            Self::Occupied(_) => "Occupied(..)",
        })
    }
}

impl<T> fmt::Display for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "container is full",
            Self::Stale(_) => "parent node no longer exists",
            Self::Occupied(_) => "position is already taken",
        })
    }
}

impl<T> core::error::Error for InsertError<T> {}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy)]
enum Order {
    Pre,
    In,
    Post,
}

/// A fixed-capacity binary tree that lives in memory provided by the caller:
/// one root, and every node has at most a left and a right child.
///
/// This is the plain structure: you decide where each node goes. It does not
/// keep values sorted. Use it for space partitions, decision and expression
/// trees.
///
/// ```
/// use vas::binary_tree::Slot;
/// use vas::{BinaryTree, Memory};
///
/// let mut memory = Memory::<Slot<&str>, 8>::new();
/// let mut tree = BinaryTree::new(&mut memory);
///
/// if let Ok(root) = tree.set_root("+") {
///     assert!(tree.insert_left(root, "1").is_ok());
///     assert!(tree.insert_right(root, "2").is_ok());
///     let text: [Option<&&str>; 3] = {
///         let mut walk = tree.in_order(root).map(|id| tree.get(id));
///         [walk.next().flatten(), walk.next().flatten(), walk.next().flatten()]
///     };
///     assert_eq!(text, [Some(&"1"), Some(&"+"), Some(&"2")]);
/// }
/// ```
///
/// With [`Dynamic`] storage the capacity is not fixed: the
/// memory comes from an allocator and the container grows when it runs out.
pub struct BinaryTree<T, S: Storage<Slot<T>>> {
    // Invariant: all links refer to live nodes and are consistent with each
    // other; every live node is reachable from `root`.
    nodes: Pool<Node<T>, S>,
    root: u32,
}

impl<T, const N: usize> BinaryTree<T, Inline<N>> {
    /// Creates an empty tree with room for `N` nodes inside itself.
    ///
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            nodes: Pool::inline(),
            root: NONE,
        }
    }
}

impl<T, const N: usize> Default for BinaryTree<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<T, A: Allocator + Default> Default for BinaryTree<T, Dynamic<A>> {
    /// Creates an empty tree that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<T, S: Storage<Slot<T>>> BinaryTree<T, S> {
    /// Creates an empty tree on `buffer`: a reference to memory borrows it,
    /// a `Memory` passed by value is stored inside the tree.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Slot<T>, Storage = S>,
    {
        Self {
            nodes: Pool::new(buffer),
            root: NONE,
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

    /// The root node.
    #[inline]
    pub fn root(&self) -> Option<NodeId<T>> {
        self.nodes.handle_at(self.root)
    }

    /// Creates the root of an empty tree.
    pub fn set_root(&mut self, value: T) -> Result<NodeId<T>, InsertError<T>> {
        if self.root != NONE {
            return Err(InsertError::Occupied(value));
        }
        let id = self.add(NONE, value)?;
        self.root = id.index();
        Ok(id)
    }

    /// Adds `value` as the left child of `parent`.
    #[inline]
    pub fn insert_left(
        &mut self,
        parent: NodeId<T>,
        value: T,
    ) -> Result<NodeId<T>, InsertError<T>> {
        self.insert_child(parent, Side::Left, value)
    }

    /// Adds `value` as the right child of `parent`.
    #[inline]
    pub fn insert_right(
        &mut self,
        parent: NodeId<T>,
        value: T,
    ) -> Result<NodeId<T>, InsertError<T>> {
        self.insert_child(parent, Side::Right, value)
    }

    /// Removes `id` **and all of its descendants**. Returns the value of
    /// `id`; the descendants' values are dropped.
    pub fn remove(&mut self, id: NodeId<T>) -> Option<T> {
        if !self.nodes.contains(id) {
            return None;
        }
        let top = id.index();
        self.unlink(top);
        // Repeatedly remove a leaf of the subtree, so no node is removed
        // before its children and no extra memory is needed.
        let mut current = top;
        loop {
            current = self.deepest(current);
            let handle = self.nodes.handle_at(current)?;
            if current == top {
                return self.nodes.remove(handle).map(|node| node.value);
            }
            let parent = self.link(current, |node| node.parent);
            self.unlink(current);
            drop(self.nodes.remove(handle));
            current = parent;
        }
    }

    /// Removes and drops all nodes. Every existing id goes stale.
    #[inline]
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = NONE;
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

    /// The parent of `id`; `None` for the root.
    #[inline]
    pub fn parent(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.nodes.handle_at(self.nodes.get(id)?.parent)
    }

    /// The left child of `id`.
    #[inline]
    pub fn left(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.nodes.handle_at(self.nodes.get(id)?.left)
    }

    /// The right child of `id`.
    #[inline]
    pub fn right(&self, id: NodeId<T>) -> Option<NodeId<T>> {
        self.nodes.handle_at(self.nodes.get(id)?.right)
    }

    /// Number of ancestors of `id`: 0 for the root.
    pub fn depth(&self, id: NodeId<T>) -> Option<usize> {
        let mut current = self.nodes.get(id)?.parent;
        let mut depth = 0;
        while current != NONE {
            depth += 1;
            current = self.link(current, |node| node.parent);
        }
        Some(depth)
    }

    /// Walks the subtree of `id`: each node, then its left subtree, then its
    /// right subtree.
    #[inline]
    pub fn pre_order(&self, id: NodeId<T>) -> Walk<'_, T, S> {
        self.walk(id, Order::Pre)
    }

    /// Walks the subtree of `id`: left subtree, then the node, then its
    /// right subtree.
    #[inline]
    pub fn in_order(&self, id: NodeId<T>) -> Walk<'_, T, S> {
        self.walk(id, Order::In)
    }

    /// Walks the subtree of `id`: left subtree, right subtree, then the
    /// node. Children always come before their parent.
    #[inline]
    pub fn post_order(&self, id: NodeId<T>) -> Walk<'_, T, S> {
        self.walk(id, Order::Post)
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

    fn walk(&self, id: NodeId<T>, order: Order) -> Walk<'_, T, S> {
        let top = if self.nodes.contains(id) {
            id.index()
        } else {
            NONE
        };
        let next = match order {
            Order::Pre => top,
            Order::In => self.leftmost(top),
            Order::Post => self.deepest(top),
        };
        Walk {
            tree: self,
            top,
            next,
            order,
        }
    }

    fn insert_child(
        &mut self,
        parent: NodeId<T>,
        side: Side,
        value: T,
    ) -> Result<NodeId<T>, InsertError<T>> {
        let taken = match (self.nodes.get(parent), side) {
            (None, _) => return Err(InsertError::Stale(value)),
            (Some(node), Side::Left) => node.left != NONE,
            (Some(node), Side::Right) => node.right != NONE,
        };
        if taken {
            return Err(InsertError::Occupied(value));
        }
        let id = self.add(parent.index(), value)?;
        if let Some(node) = self.nodes.get_at_mut(parent.index()) {
            match side {
                Side::Left => node.left = id.index(),
                Side::Right => node.right = id.index(),
            }
        }
        Ok(id)
    }

    /// Stores a new node whose parent link is already set.
    fn add(&mut self, parent: u32, value: T) -> Result<NodeId<T>, InsertError<T>> {
        let node = Node {
            value,
            parent,
            left: NONE,
            right: NONE,
        };
        self.nodes
            .insert(node)
            .map_err(|Full(node)| InsertError::Full(node.value))
    }

    /// Detaches node `index` from its parent (or from the root position).
    fn unlink(&mut self, index: u32) {
        let parent = self.link(index, |node| node.parent);
        match self.nodes.get_at_mut(parent) {
            Some(node) => {
                if node.left == index {
                    node.left = NONE;
                }
                if node.right == index {
                    node.right = NONE;
                }
            }
            None => {
                if self.root == index {
                    self.root = NONE;
                }
            }
        }
        if let Some(node) = self.nodes.get_at_mut(index) {
            node.parent = NONE;
        }
    }

    /// Follows one link of the live node in slot `index`.
    #[inline]
    fn link(&self, index: u32, pick: impl Fn(&Node<T>) -> u32) -> u32 {
        self.nodes.get_at(index).map_or(NONE, pick)
    }

    /// The node reached from `index` by going left as far as possible.
    fn leftmost(&self, mut index: u32) -> u32 {
        loop {
            let left = self.link(index, |node| node.left);
            if left == NONE {
                return index;
            }
            index = left;
        }
    }

    /// A leaf below `index`, preferring left children: the first node of a
    /// post-order walk.
    fn deepest(&self, mut index: u32) -> u32 {
        loop {
            let Some(node) = self.nodes.get_at(index) else {
                return index;
            };
            index = if node.left != NONE {
                node.left
            } else if node.right != NONE {
                node.right
            } else {
                return index;
            };
        }
    }
}

/// Iterator over the nodes of a subtree in a fixed order. See
/// [`BinaryTree::pre_order`], [`BinaryTree::in_order`] and
/// [`BinaryTree::post_order`].
///
/// Needs no extra memory: it follows the parent links.
pub struct Walk<'t, T, S: Storage<Slot<T>>> {
    tree: &'t BinaryTree<T, S>,
    top: u32,
    next: u32,
    order: Order,
}

impl<T, S: Storage<Slot<T>>> Walk<'_, T, S> {
    /// The node that follows `current` in the walk.
    fn after(&self, current: u32) -> u32 {
        let tree = self.tree;
        let Some(node) = tree.nodes.get_at(current) else {
            return NONE;
        };
        match self.order {
            Order::Pre => {
                if node.left != NONE {
                    return node.left;
                }
                if node.right != NONE {
                    return node.right;
                }
                // Climb until a right subtree that was not walked yet.
                let mut child = current;
                loop {
                    if child == self.top {
                        return NONE;
                    }
                    let parent = tree.link(child, |node| node.parent);
                    let Some(above) = tree.nodes.get_at(parent) else {
                        return NONE;
                    };
                    if above.left == child && above.right != NONE {
                        return above.right;
                    }
                    child = parent;
                }
            }
            Order::In => {
                if node.right != NONE {
                    return tree.leftmost(node.right);
                }
                // Climb until arriving from a left child.
                let mut child = current;
                loop {
                    if child == self.top {
                        return NONE;
                    }
                    let parent = tree.link(child, |node| node.parent);
                    if tree.link(parent, |node| node.left) == child {
                        return parent;
                    }
                    child = parent;
                }
            }
            Order::Post => {
                if current == self.top {
                    return NONE;
                }
                let Some(above) = tree.nodes.get_at(node.parent) else {
                    return NONE;
                };
                if above.left == current && above.right != NONE {
                    tree.deepest(above.right)
                } else {
                    node.parent
                }
            }
        }
    }
}

impl<T, S: Storage<Slot<T>>> Iterator for Walk<'_, T, S> {
    type Item = NodeId<T>;

    #[inline]
    fn next(&mut self) -> Option<NodeId<T>> {
        let id = self.tree.nodes.handle_at(self.next)?;
        self.next = self.after(self.next);
        Some(id)
    }
}
