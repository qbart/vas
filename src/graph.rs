//! Fixed-capacity directed graph over user-provided memory.

use core::fmt;
use core::mem::MaybeUninit;

use crate::Full;
use crate::path::Space;
use crate::pool::{self, Handle, Pool};
use crate::storage::Borrowed;

/// "No edge" in a link.
const NONE: u32 = u32::MAX;

/// A node value together with its links. Opaque; it appears in the types
/// [`NodeSlot`] and [`NodeId`].
pub struct Node<N> {
    value: N,
    // Heads of the lists of edges leaving and entering this node.
    first_out: u32,
    first_in: u32,
}

/// An edge value together with its links. Opaque; it appears in the types
/// [`EdgeSlot`] and [`EdgeId`].
pub struct Edge<E> {
    value: E,
    // Slot indices of the two end nodes.
    from: u32,
    to: u32,
    // Doubly linked lists: the edges leaving `from`, and those entering `to`.
    next_out: u32,
    prev_out: u32,
    next_in: u32,
    prev_in: u32,
}

/// One slot of node storage: `Memory::<NodeSlot<N>, COUNT>`.
pub type NodeSlot<N> = pool::Slot<Node<N>>;

/// One slot of edge storage: `Memory::<EdgeSlot<E>, COUNT>`.
pub type EdgeSlot<E> = pool::Slot<Edge<E>>;

/// Identifies a node of a [`Graph`]. `Copy`, 8 bytes, stale once the node is
/// removed.
pub type NodeId<N> = Handle<Node<N>>;

/// Identifies an edge of a [`Graph`]. `Copy`, 8 bytes, stale once the edge
/// is removed.
pub type EdgeId<E> = Handle<Edge<E>>;

/// An edge could not be added. Carries the value back to the caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InsertError<E> {
    /// The graph has no room for another edge.
    Full(E),
    /// One of the two nodes has been removed.
    Stale(E),
}

impl<E> fmt::Debug for InsertError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "Full(..)",
            Self::Stale(_) => "Stale(..)",
        })
    }
}

impl<E> fmt::Display for InsertError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "container is full",
            Self::Stale(_) => "node no longer exists",
        })
    }
}

impl<E> core::error::Error for InsertError<E> {}

/// Edge values that have a travel cost, so a [`Graph`] can be searched by a
/// [`PathFinder`](crate::PathFinder). Implemented for `f32`.
pub trait Cost {
    /// The cost of following this edge. Must be finite and not negative.
    fn cost(&self) -> f32;
}

impl Cost for f32 {
    #[inline]
    fn cost(&self) -> f32 {
        *self
    }
}

/// A fixed-capacity directed graph that lives in memory provided by the
/// caller: nodes carrying a value `N`, connected by edges carrying a value
/// `E`. Nodes and edges can be added and removed at any time.
///
/// Edges have a direction. For a two-way connection add an edge each way.
/// Several edges between the same two nodes, and edges from a node to
/// itself, are allowed.
///
/// ```
/// use vas::graph::{EdgeSlot, NodeSlot};
/// use vas::{Graph, Memory};
///
/// let mut nodes = Memory::<NodeSlot<&str>, 8>::new();
/// let mut edges = Memory::<EdgeSlot<f32>, 16>::new();
/// let mut graph = Graph::new(&mut nodes, &mut edges);
///
/// if let (Ok(town), Ok(castle)) = (graph.add_node("town"), graph.add_node("castle")) {
///     assert!(graph.add_edge(town, castle, 5.0).is_ok());
///     assert_eq!(graph.outgoing(town).count(), 1);
///     assert_eq!(graph.incoming(castle).count(), 1);
/// }
/// ```
pub struct Graph<'a, N, E> {
    // Invariant: every edge's `from` and `to` are live nodes, and every edge
    // is in exactly the out-list of `from` and the in-list of `to`.
    nodes: Pool<Node<N>, Borrowed<'a>>,
    edges: Pool<Edge<E>, Borrowed<'a>>,
}

impl<'a, N, E> Graph<'a, N, E> {
    /// Creates an empty graph using the two blocks as its memory.
    #[inline]
    pub fn new(
        nodes: &'a mut [MaybeUninit<NodeSlot<N>>],
        edges: &'a mut [MaybeUninit<EdgeSlot<E>>],
    ) -> Self {
        Self {
            nodes: Pool::new(nodes),
            edges: Pool::new(edges),
        }
    }

    /// Number of nodes in the graph.
    #[inline]
    pub const fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges in the graph.
    #[inline]
    pub const fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Maximum number of nodes the graph can hold.
    #[inline]
    pub fn node_capacity(&self) -> usize {
        self.nodes.capacity()
    }

    /// Maximum number of edges the graph can hold.
    #[inline]
    pub fn edge_capacity(&self) -> usize {
        self.edges.capacity()
    }

    /// Adds a node, or hands the value back in [`Full`] when there is no
    /// room.
    pub fn add_node(&mut self, value: N) -> Result<NodeId<N>, Full<N>> {
        let node = Node {
            value,
            first_out: NONE,
            first_in: NONE,
        };
        self.nodes
            .insert(node)
            .map_err(|Full(node)| Full(node.value))
    }

    /// Adds an edge from `from` to `to`.
    pub fn add_edge(
        &mut self,
        from: NodeId<N>,
        to: NodeId<N>,
        value: E,
    ) -> Result<EdgeId<E>, InsertError<E>> {
        let (Some(source), Some(target)) = (self.nodes.get(from), self.nodes.get(to)) else {
            return Err(InsertError::Stale(value));
        };
        // The new edge goes to the front of both lists.
        let (next_out, next_in) = (source.first_out, target.first_in);
        let edge = Edge {
            value,
            from: from.index(),
            to: to.index(),
            next_out,
            prev_out: NONE,
            next_in,
            prev_in: NONE,
        };
        let id = self
            .edges
            .insert(edge)
            .map_err(|Full(edge)| InsertError::Full(edge.value))?;
        let index = id.index();
        if let Some(after) = self.edges.get_at_mut(next_out) {
            after.prev_out = index;
        }
        if let Some(after) = self.edges.get_at_mut(next_in) {
            after.prev_in = index;
        }
        if let Some(source) = self.nodes.get_at_mut(from.index()) {
            source.first_out = index;
        }
        if let Some(target) = self.nodes.get_at_mut(to.index()) {
            target.first_in = index;
        }
        Ok(id)
    }

    /// Removes an edge and returns its value.
    pub fn remove_edge(&mut self, id: EdgeId<E>) -> Option<E> {
        if !self.edges.contains(id) {
            return None;
        }
        self.unlink(id.index());
        self.edges.remove(id).map(|edge| edge.value)
    }

    /// Removes a node **and every edge that starts or ends at it**. Returns
    /// the node's value; the edges' values are dropped.
    pub fn remove_node(&mut self, id: NodeId<N>) -> Option<N> {
        if !self.nodes.contains(id) {
            return None;
        }
        let index = id.index();
        loop {
            let node = self.nodes.get_at(index)?;
            let edge = if node.first_out != NONE {
                node.first_out
            } else {
                node.first_in
            };
            let Some(handle) = self.edges.handle_at(edge) else {
                break;
            };
            self.unlink(edge);
            drop(self.edges.remove(handle));
        }
        self.nodes.remove(id).map(|node| node.value)
    }

    /// Removes and drops all nodes and edges. Every existing id goes stale.
    #[inline]
    pub fn clear(&mut self) {
        self.edges.clear();
        self.nodes.clear();
    }

    /// The value of node `id`.
    #[inline]
    pub fn node(&self, id: NodeId<N>) -> Option<&N> {
        self.nodes.get(id).map(|node| &node.value)
    }

    /// The value of node `id`, mutably.
    #[inline]
    pub fn node_mut(&mut self, id: NodeId<N>) -> Option<&mut N> {
        self.nodes.get_mut(id).map(|node| &mut node.value)
    }

    /// `true` when `id` still refers to a node of the graph.
    #[inline]
    pub fn contains_node(&self, id: NodeId<N>) -> bool {
        self.nodes.contains(id)
    }

    /// The id of the node in slot `index`, the inverse of
    /// [`NodeId::index`](Handle::index). This is how node indices produced by
    /// a path search are turned back into ids.
    #[inline]
    pub fn node_at(&self, index: u32) -> Option<NodeId<N>> {
        self.nodes.handle_at(index)
    }

    /// The value of edge `id`.
    #[inline]
    pub fn edge(&self, id: EdgeId<E>) -> Option<&E> {
        self.edges.get(id).map(|edge| &edge.value)
    }

    /// The value of edge `id`, mutably.
    #[inline]
    pub fn edge_mut(&mut self, id: EdgeId<E>) -> Option<&mut E> {
        self.edges.get_mut(id).map(|edge| &mut edge.value)
    }

    /// `true` when `id` still refers to an edge of the graph.
    #[inline]
    pub fn contains_edge(&self, id: EdgeId<E>) -> bool {
        self.edges.contains(id)
    }

    /// The nodes edge `id` goes from and to.
    #[inline]
    pub fn endpoints(&self, id: EdgeId<E>) -> Option<(NodeId<N>, NodeId<N>)> {
        let edge = self.edges.get(id)?;
        Some((
            self.nodes.handle_at(edge.from)?,
            self.nodes.handle_at(edge.to)?,
        ))
    }

    /// The first edge found going from `from` to `to`. O(edges leaving
    /// `from`).
    pub fn find_edge(&self, from: NodeId<N>, to: NodeId<N>) -> Option<EdgeId<E>> {
        if !self.nodes.contains(to) {
            return None;
        }
        self.outgoing(from)
            .find(|(_, target)| *target == to)
            .map(|(edge, _)| edge)
    }

    /// Iterates over the edges leaving `id`, as `(edge, target node)`, most
    /// recently added first.
    #[inline]
    pub fn outgoing(&self, id: NodeId<N>) -> Outgoing<'_, 'a, N, E> {
        Outgoing {
            graph: self,
            next: self.nodes.get(id).map_or(NONE, |node| node.first_out),
        }
    }

    /// Iterates over the edges entering `id`, as `(edge, source node)`, most
    /// recently added first.
    #[inline]
    pub fn incoming(&self, id: NodeId<N>) -> Incoming<'_, 'a, N, E> {
        Incoming {
            graph: self,
            next: self.nodes.get(id).map_or(NONE, |node| node.first_in),
        }
    }

    /// Iterates over all nodes and their values, in no particular order.
    #[inline]
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId<N>, &N)> {
        self.nodes.iter().map(|(id, node)| (id, &node.value))
    }

    /// Iterates over all edges and their values, in no particular order.
    #[inline]
    pub fn edges(&self) -> impl Iterator<Item = (EdgeId<E>, &E)> {
        self.edges.iter().map(|(id, edge)| (id, &edge.value))
    }

    /// Takes the edge in slot `index` out of both lists it is in.
    fn unlink(&mut self, index: u32) {
        let Some(edge) = self.edges.get_at(index) else {
            return;
        };
        let (from, to) = (edge.from, edge.to);
        let (next_out, prev_out) = (edge.next_out, edge.prev_out);
        let (next_in, prev_in) = (edge.next_in, edge.prev_in);

        if let Some(before) = self.edges.get_at_mut(prev_out) {
            before.next_out = next_out;
        } else if let Some(node) = self.nodes.get_at_mut(from) {
            node.first_out = next_out;
        }
        if let Some(after) = self.edges.get_at_mut(next_out) {
            after.prev_out = prev_out;
        }

        if let Some(before) = self.edges.get_at_mut(prev_in) {
            before.next_in = next_in;
        } else if let Some(node) = self.nodes.get_at_mut(to) {
            node.first_in = next_in;
        }
        if let Some(after) = self.edges.get_at_mut(next_in) {
            after.prev_in = prev_in;
        }
    }
}

/// A graph whose edges have a [`Cost`] can be searched directly: nodes are
/// addressed by [`NodeId::index`](Handle::index), and the search needs one
/// `path::Node` per unit of [`Graph::node_capacity`].
///
/// There is no heuristic, so the search is Dijkstra's algorithm.
impl<N, E: Cost> Space for Graph<'_, N, E> {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        let mut next = self.nodes.get_at(node).map_or(NONE, |node| node.first_out);
        while let Some(edge) = self.edges.get_at(next) {
            visit(edge.to, edge.value.cost());
            next = edge.next_out;
        }
    }
}

/// Iterator over the edges leaving a node. See [`Graph::outgoing`].
pub struct Outgoing<'g, 'a, N, E> {
    graph: &'g Graph<'a, N, E>,
    next: u32,
}

impl<N, E> Iterator for Outgoing<'_, '_, N, E> {
    type Item = (EdgeId<E>, NodeId<N>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let edge = self.graph.edges.get_at(self.next)?;
        let id = self.graph.edges.handle_at(self.next)?;
        self.next = edge.next_out;
        Some((id, self.graph.nodes.handle_at(edge.to)?))
    }
}

/// Iterator over the edges entering a node. See [`Graph::incoming`].
pub struct Incoming<'g, 'a, N, E> {
    graph: &'g Graph<'a, N, E>,
    next: u32,
}

impl<N, E> Iterator for Incoming<'_, '_, N, E> {
    type Item = (EdgeId<E>, NodeId<N>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let edge = self.graph.edges.get_at(self.next)?;
        let id = self.graph.edges.handle_at(self.next)?;
        self.next = edge.next_in;
        Some((id, self.graph.nodes.handle_at(edge.from)?))
    }
}
