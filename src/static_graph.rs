//! Read-only directed graph in compressed form over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::fmt;

use crate::Vector;
use crate::graph::Cost;
use crate::path::Space;
use crate::storage::{Inline, IntoBuffer, Storage};

/// One outgoing edge of a [`StaticGraph`]: the node it leads to and its
/// value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge<E> {
    /// Index of the node this edge leads to.
    pub to: u32,
    /// The edge's value, for example its cost.
    pub value: E,
}

/// Why a [`StaticGraph`] could not be built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildError {
    /// The memory is too small: `offsets` needs `node_count + 1` entries and
    /// `edges` one entry per edge.
    Full,
    /// An edge refers to a node that is not below `node_count`.
    OutOfBounds,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "memory is too small for the graph",
            Self::OutOfBounds => "edge refers to a node out of bounds",
        })
    }
}

impl core::error::Error for BuildError {}

/// A directed graph that is built once and then only read, in memory
/// provided by the caller.
///
/// All edges leaving a node are stored next to each other (compressed sparse
/// row), so listing a node's neighbours is a slice lookup with no links to
/// follow. This is the fastest layout for searching a graph that does not
/// change, such as a navigation mesh or a waypoint network. Use
/// [`Graph`](crate::Graph) when nodes or edges must be added or removed
/// later.
///
/// Nodes are the indices `0..node_count` and carry no value of their own;
/// keep node data in your own array, indexed the same way.
///
/// ```
/// use vas::static_graph::Edge;
/// use vas::{Memory, StaticGraph};
///
/// let mut offsets = Memory::<u32, 4>::new(); // node_count + 1
/// let mut edges = Memory::<Edge<f32>, 3>::new();
///
/// // (from, to, value)
/// let input = [(0, 1, 1.5), (0, 2, 4.0), (1, 2, 1.0)];
/// let graph = StaticGraph::build(3, &input, &mut offsets, &mut edges);
///
/// assert!(graph.is_ok_and(|graph| graph.neighbors(0).len() == 2));
/// ```
///
/// With [`Dynamic`](crate::Dynamic) storage `build` takes exactly the memory
/// the graph needs from an allocator.
pub struct StaticGraph<E, SO: Storage<u32>, SE: Storage<Edge<E>> = SO> {
    // Invariant: `offsets` has `node_count + 1` non-decreasing entries, the
    // first 0 and the last `edges.len()`. The edges of node `i` are
    // `edges[offsets[i]..offsets[i + 1]]`.
    offsets: Vector<u32, SO>,
    edges: Vector<Edge<E>, SE>,
}

/// Only graphs with inline storage can be cloned.
impl<E: Clone, const OFFSETS: usize, const EDGES: usize> Clone
    for StaticGraph<E, Inline<OFFSETS>, Inline<EDGES>>
{
    fn clone(&self) -> Self {
        Self {
            offsets: self.offsets.clone(),
            edges: self.edges.clone(),
        }
    }
}

impl<E: Copy, SO: Storage<u32>, SE: Storage<Edge<E>>> StaticGraph<E, SO, SE> {
    /// Builds a graph of `node_count` nodes from `(from, to, value)` triples
    /// in any order.
    ///
    /// `offsets` needs `node_count + 1` entries and `edges` one entry per
    /// input edge. The edges of each node keep the order they have in
    /// `input`. O(nodes + edges), no extra memory.
    pub fn build<BO, BE>(
        node_count: usize,
        input: &[(u32, u32, E)],
        offsets: BO,
        edges: BE,
    ) -> Result<Self, BuildError>
    where
        BO: IntoBuffer<u32, Storage = SO>,
        BE: IntoBuffer<Edge<E>, Storage = SE>,
    {
        let offset_count = node_count.checked_add(1).ok_or(BuildError::Full)?;
        if u32::try_from(input.len()).is_err() {
            return Err(BuildError::Full);
        }
        let mut offsets_vector = Vector::new(offsets);
        offsets_vector
            .resize(offset_count, 0)
            .map_err(|_| BuildError::Full)?;
        let offsets = offsets_vector.as_mut_slice();
        let mut edges_vector = Vector::new(edges);
        edges_vector
            .reserve(input.len())
            .map_err(|_| BuildError::Full)?;
        let edges = edges_vector
            .slots_mut()
            .get_mut(..input.len())
            .ok_or(BuildError::Full)?;

        // Count the edges leaving each node, one position ahead.
        for (from, to, _) in input {
            if *to as usize >= node_count {
                return Err(BuildError::OutOfBounds);
            }
            let count = (*from as usize)
                .checked_add(1)
                .and_then(|index| offsets.get_mut(index))
                .ok_or(BuildError::OutOfBounds)?;
            *count += 1;
        }
        // Running total: `offsets[i]` becomes the start of node `i`.
        let mut total = 0u32;
        for offset in offsets.iter_mut() {
            total += *offset;
            *offset = total;
        }
        // Place each edge at its node's cursor, using `offsets[from]` as the
        // cursor. Afterwards `offsets[i]` is the start of node `i + 1`.
        for (from, to, value) in input {
            let cursor = offsets
                .get_mut(*from as usize)
                .ok_or(BuildError::OutOfBounds)?;
            let slot = edges.get_mut(*cursor as usize).ok_or(BuildError::Full)?;
            slot.write(Edge {
                to: *to,
                value: *value,
            });
            *cursor += 1;
        }
        // Shift the cursors back into start offsets.
        for index in (1..offsets.len()).rev() {
            if let Ok([current, before]) = offsets.get_disjoint_mut([index, index - 1]) {
                *current = *before;
            }
        }
        if let Some(first) = offsets.first_mut() {
            *first = 0;
        }

        // SAFETY: the cursors of the nodes partition `0..input.len()` and
        // each input edge was written at a distinct cursor position, so all
        // `input.len()` slots are initialised. `input.len()` fits the
        // vector, as checked when taking the slots.
        unsafe { edges_vector.set_len(input.len()) };
        Ok(Self {
            offsets: offsets_vector,
            edges: edges_vector,
        })
    }
}

impl<E, SO: Storage<u32>, SE: Storage<Edge<E>>> StaticGraph<E, SO, SE> {
    /// Number of nodes in the graph.
    #[inline]
    pub const fn node_count(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    /// Number of edges in the graph.
    #[inline]
    pub const fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// The edges leaving `node`, as one contiguous slice. Empty for a node
    /// that is out of bounds.
    #[inline]
    pub fn neighbors(&self, node: u32) -> &[Edge<E>] {
        let node = node as usize;
        let (Some(start), Some(end)) = (
            self.offsets.get(node),
            node.checked_add(1).and_then(|next| self.offsets.get(next)),
        ) else {
            return &[];
        };
        self.edges
            .get(*start as usize..*end as usize)
            .unwrap_or_default()
    }

    /// Number of edges leaving `node`.
    #[inline]
    pub fn out_degree(&self, node: u32) -> usize {
        self.neighbors(node).len()
    }
}

/// A graph whose edges have a [`Cost`] can be searched directly by a
/// [`PathFinder`](crate::PathFinder). There is no heuristic, so the search
/// is Dijkstra's algorithm.
impl<E: Cost, SO: Storage<u32>, SE: Storage<Edge<E>>> Space for StaticGraph<E, SO, SE> {
    #[inline]
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        for edge in StaticGraph::neighbors(self, node) {
            visit(edge.to, edge.value.cost());
        }
    }
}

impl<E: fmt::Debug, SO: Storage<u32>, SE: Storage<Edge<E>>> fmt::Debug for StaticGraph<E, SO, SE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries((0..self.node_count()).map(|node| (node, self.neighbors(node as u32))))
            .finish()
    }
}
