//! Shortest-path search (A* / Dijkstra) over borrowed, inline or growing memory.

use core::cmp::Ordering;
use core::fmt;

use crate::allocator::Allocator;
use crate::heap::{Heap, Order};
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// The thing being searched: anything that can list the neighbours of a
/// node. Nodes are dense `u32` indices, for example `y * width + x` on a
/// tile grid.
pub trait Space {
    /// Calls `visit(neighbor, cost)` for every node reachable from `node` in
    /// one step. `cost` must be finite and not negative; other costs are
    /// ignored.
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, visit: F);

    /// Estimated remaining cost from `from` to `goal`. It must never
    /// overestimate, or the path found may not be the cheapest.
    ///
    /// The default of zero turns the search into Dijkstra's algorithm: always
    /// correct, but it explores more nodes.
    #[inline]
    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        let _ = (from, goal);
        0.0
    }
}

/// Why a search did not produce a path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathError {
    /// The goal cannot be reached from the start.
    NoPath,
    /// The open list ran out of room. Give the finder more `Open` memory.
    Full,
    /// The start, the goal or a neighbour is not below the number of nodes
    /// the finder has memory for.
    OutOfBounds,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoPath => "goal is not reachable",
            Self::Full => "open list is full",
            Self::OutOfBounds => "node is out of bounds",
        })
    }
}

impl core::error::Error for PathError {}

/// Per-node search state. Opaque; it only exists so the user can size the
/// memory for a [`PathFinder`]: one per node, `Memory::<Node, N>`.
#[derive(Clone, Copy)]
pub struct Node {
    // The search that last touched this node. Other fields are only
    // meaningful when it equals the finder's current stamp.
    stamp: u32,
    // The node this one was reached from.
    parent: u32,
    // Cheapest known cost from the start.
    cost: f32,
}

/// An entry of the open list. Opaque; it only exists so the user can size
/// the memory for a [`PathFinder`]: `Memory::<Open, N>`.
#[derive(Clone, Copy)]
pub struct Open {
    // Cost so far plus the heuristic.
    priority: f32,
    // Cost from the start when this entry was pushed.
    cost: f32,
    node: u32,
}

/// Lowest priority first; on ties, the entry further along the path.
#[derive(Clone, Copy)]
struct Cheapest;

impl Order<Open> for Cheapest {
    #[inline]
    fn cmp(&self, a: &Open, b: &Open) -> Ordering {
        b.priority
            .total_cmp(&a.priority)
            .then_with(|| a.cost.total_cmp(&b.cost))
    }
}

/// A reusable shortest-path search over memory provided by the caller.
///
/// It needs two blocks: one [`Node`] per node of the space, and room for the
/// open list ([`Open`] entries). The same finder can run any number of
/// searches; starting a search does not clear the node memory.
///
/// ```
/// use vas::path::{Node, Open, Space};
/// use vas::{Memory, PathFinder};
///
/// // Five nodes in a row: 0 - 1 - 2 - 3 - 4.
/// struct Row;
///
/// impl Space for Row {
///     fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
///         if node > 0 {
///             visit(node - 1, 1.0);
///         }
///         if node < 4 {
///             visit(node + 1, 1.0);
///         }
///     }
/// }
///
/// let mut nodes = Memory::<Node, 5>::new();
/// let mut open = Memory::<Open, 16>::new();
/// let mut finder = PathFinder::new(&mut nodes, &mut open);
///
/// assert_eq!(finder.find(&Row, 0, 3), Ok(3.0));
/// assert!(finder.path_rev().eq([3, 2, 1, 0]));
/// ```
///
/// With [`Dynamic`] storage the finder takes memory from an
/// allocator as searches need it: node memory up to the highest node
/// touched, and an open list that never fills up.
pub struct PathFinder<SN: Storage<Node>, SO: Storage<Open> = SN> {
    // Every slot is initialised, so `nodes.len()` is the node capacity.
    nodes: Vector<Node, SN>,
    open: Heap<Open, SO, Cheapest>,
    // Identifies the current search; see `Node::stamp`.
    stamp: u32,
    // Start and goal of the last search, when it found a path.
    found: Option<(u32, u32)>,
}

/// The copy remembers the last search. Only finders with inline storage
/// can be cloned.
impl<const NODES: usize, const OPEN: usize> Clone for PathFinder<Inline<NODES>, Inline<OPEN>> {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            open: self.open.clone(),
            stamp: self.stamp,
            found: self.found,
        }
    }
}

impl<AN, AO> Default for PathFinder<Dynamic<AN>, Dynamic<AO>>
where
    AN: Allocator + Default,
    AO: Allocator + Default,
{
    /// Creates a finder that grows through the default allocators.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(AN::default()), Dynamic(AO::default()))
    }
}

/// A node no search has reached yet.
const UNSEEN: Node = Node {
    stamp: 0,
    parent: 0,
    cost: 0.0,
};

/// The search state of node `index`. With dynamic storage the node memory
/// grows to reach it; with fixed storage an index past the end is `None`.
#[inline]
fn node_mut<S: Storage<Node>>(nodes: &mut Vector<Node, S>, index: u32) -> Option<&mut Node> {
    let index = index as usize;
    if index >= nodes.len() {
        nodes.resize_with(index.checked_add(1)?, || UNSEEN).ok()?;
    }
    nodes.get_mut(index)
}

/// Node memory as the search sees it: fixed (a slice) or growing (a
/// vector over dynamic storage).
trait Nodes {
    /// The search state of node `index`, if there is memory for it.
    fn node(&mut self, index: u32) -> Option<&mut Node>;
}

impl Nodes for [Node] {
    #[inline]
    fn node(&mut self, index: u32) -> Option<&mut Node> {
        self.get_mut(index as usize)
    }
}

impl<S: Storage<Node>> Nodes for Vector<Node, S> {
    #[inline]
    fn node(&mut self, index: u32) -> Option<&mut Node> {
        node_mut(self, index)
    }
}

/// Offers every neighbour of `current` a route through it, and queues the
/// ones it improves.
#[inline]
fn expand<S, N, SO>(
    space: &S,
    nodes: &mut N,
    open: &mut Heap<Open, SO, Cheapest>,
    current: &Open,
    goal: u32,
    stamp: u32,
) -> Result<(), PathError>
where
    S: Space,
    N: Nodes + ?Sized,
    SO: Storage<Open>,
{
    let mut error = None;
    space.neighbors(current.node, |next, step| {
        if error.is_some() || !step.is_finite() || step < 0.0 {
            return;
        }
        let Some(node) = nodes.node(next) else {
            error = Some(PathError::OutOfBounds);
            return;
        };
        let cost = current.cost + step;
        if node.stamp == stamp && node.cost <= cost {
            return;
        }
        *node = Node {
            stamp,
            parent: current.node,
            cost,
        };
        let entry = Open {
            priority: cost + space.heuristic(next, goal),
            cost,
            node: next,
        };
        if open.push(entry).is_err() {
            error = Some(PathError::Full);
        }
    });
    error.map_or(Ok(()), Err)
}

impl<SN: Storage<Node>, SO: Storage<Open>> PathFinder<SN, SO> {
    /// Creates a finder for a space of `nodes.len()` nodes, with `open` as
    /// the memory for its open list.
    ///
    /// The open list holds one entry per improvement found for a node, so
    /// its worst case is the number of edges; a few times the expected path
    /// length is usually plenty. Running out is reported as
    /// [`PathError::Full`], never a wrong path.
    ///
    /// Initialises the node memory, so creation is O(nodes).
    ///
    /// References to memory borrow it; `Memory` blocks passed by value are
    /// stored inside the finder.
    pub fn new<BN, BO>(nodes: BN, open: BO) -> Self
    where
        BN: IntoBuffer<Node, Storage = SN>,
        BO: IntoBuffer<Open, Storage = SO>,
    {
        let mut nodes = Vector::new(nodes);
        let capacity = nodes.capacity();
        // Cannot fail: the length asked for is the capacity.
        let _ = nodes.resize_with(capacity, || UNSEEN);
        Self {
            nodes,
            open: Heap::with_order(open, Cheapest),
            stamp: 0,
            found: None,
        }
    }

    /// Number of nodes the finder has memory for. With dynamic storage it
    /// grows to the highest node a search touches.
    #[inline]
    pub const fn node_capacity(&self) -> usize {
        self.nodes.len()
    }

    /// Makes sure there is memory for `nodes` nodes, growing dynamic
    /// storage when needed. Fails without changes when the storage is fixed
    /// and too small, or when the allocator refuses.
    pub fn reserve(&mut self, nodes: usize) -> Result<(), Full<()>> {
        if nodes <= self.nodes.len() {
            return Ok(());
        }
        self.nodes.resize_with(nodes, || UNSEEN)
    }

    /// Finds the cheapest path from `start` to `goal` and returns its cost.
    ///
    /// Uses A* with the space's heuristic (Dijkstra when the heuristic is
    /// zero). After `Ok`, read the path with [`path_rev`](Self::path_rev) or
    /// [`path_into`](Self::path_into).
    pub fn find<S: Space>(&mut self, space: &S, start: u32, goal: u32) -> Result<f32, PathError> {
        self.found = None;
        self.open.clear();
        self.next_stamp();
        let stamp = self.stamp;

        if node_mut(&mut self.nodes, goal).is_none() {
            return Err(PathError::OutOfBounds);
        }
        let first = node_mut(&mut self.nodes, start).ok_or(PathError::OutOfBounds)?;
        *first = Node {
            stamp,
            parent: start,
            cost: 0.0,
        };
        self.open
            .push(Open {
                priority: space.heuristic(start, goal),
                cost: 0.0,
                node: start,
            })
            .map_err(|_| PathError::Full)?;

        while let Some(current) = self.open.pop() {
            // Skip entries that were superseded by a cheaper route.
            match self.nodes.get(current.node as usize) {
                Some(node) if current.cost <= node.cost => {}
                _ => continue,
            }
            if current.node == goal {
                self.found = Some((start, goal));
                return Ok(current.cost);
            }

            // With fixed storage the nodes are a plain slice for the whole
            // search, which is measurably faster than going through the
            // vector for every neighbour.
            if <SN::Buffer as Buffer<Node>>::GROWS {
                expand(
                    space,
                    &mut self.nodes,
                    &mut self.open,
                    &current,
                    goal,
                    stamp,
                )?;
            } else {
                let nodes = self.nodes.as_mut_slice();
                expand(space, nodes, &mut self.open, &current, goal, stamp)?;
            }
        }
        Err(PathError::NoPath)
    }

    /// Cheapest known cost from the start of the last search to `node`, or
    /// `None` when that search did not reach it.
    ///
    /// The cost is final for every node on the found path; for other nodes
    /// it is the best seen before the search stopped.
    #[inline]
    pub fn cost(&self, node: u32) -> Option<f32> {
        let state = self.nodes.get(node as usize)?;
        (self.stamp != 0 && state.stamp == self.stamp).then_some(state.cost)
    }

    /// The path of the last search, **from the goal back to the start**.
    /// Empty when the last search found no path.
    ///
    /// Needs no extra memory. Use [`path_into`](Self::path_into) for the
    /// path in forward order.
    #[inline]
    pub fn path_rev(&self) -> PathRev<'_> {
        PathRev {
            nodes: self.nodes.as_slice(),
            next: self.found.map(|(_, goal)| goal),
            start: self.found.map_or(0, |(start, _)| start),
            remaining: self.nodes.len(),
        }
    }

    /// Appends the path of the last search to `out`, from start to goal, and
    /// returns the number of nodes written (0 when no path was found).
    ///
    /// Leaves `out` unchanged when the path does not fit.
    pub fn path_into<S: Storage<u32>>(&self, out: &mut Vector<u32, S>) -> Result<usize, Full<()>> {
        let before = out.len();
        for node in self.path_rev() {
            if out.push(node).is_err() {
                out.truncate(before);
                return Err(Full(()));
            }
        }
        if let Some(path) = out.get_mut(before..) {
            path.reverse();
        }
        Ok(out.len() - before)
    }

    /// Moves to a fresh stamp, which invalidates all node state at once.
    fn next_stamp(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            // The counter wrapped: old stamps could be mistaken for new
            // ones, so reset them all.
            for node in self.nodes.iter_mut() {
                node.stamp = 0;
            }
            self.stamp = 1;
        }
    }
}

/// Iterator over a found path, from the goal back to the start. See
/// [`PathFinder::path_rev`].
pub struct PathRev<'f> {
    nodes: &'f [Node],
    next: Option<u32>,
    start: u32,
    // Upper bound on the path length; guards against ever looping.
    remaining: usize,
}

impl Iterator for PathRev<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        let node = self.next?;
        self.remaining = self.remaining.checked_sub(1)?;
        self.next = if node == self.start {
            None
        } else {
            self.nodes.get(node as usize).map(|state| state.parent)
        };
        Some(node)
    }
}
