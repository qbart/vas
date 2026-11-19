//! Flow fields (Dijkstra maps): the cheapest way to the nearest goal from
//! every node at once, over borrowed, inline or growing memory.

use core::fmt;

use crate::allocator::Allocator;
use crate::heap::Heap;
use crate::path::{Cheapest, Node, Nodes, Open, Space, UNSEEN, node_mut};
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// Why a [`FlowField`] could not be built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlowError {
    /// The open list ran out of room. Give the field more `Open` memory.
    Full,
    /// A goal or a neighbour is not below the number of nodes the field has
    /// memory for.
    OutOfBounds,
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "open list is full",
            Self::OutOfBounds => "node is out of bounds",
        })
    }
}

impl core::error::Error for FlowError {}

/// The cheapest way from every node of a [`Space`] to the nearest of a set
/// of goals, found by one search: a flow field, or Dijkstra map.
///
/// Where [`PathFinder`](crate::PathFinder) finds one path for one unit, a
/// flow field serves any number of units heading for the same goals: each
/// one reads [`next`](Self::next) for the node it is on.
///
/// The field is searched from the goals outwards, following the steps
/// into each node: [`Space::predecessors`]. A [`Grid`](crate::Grid) space
/// lists them exactly. Other spaces list their neighbours by default, which
/// is right when every step can be taken back at the same cost; for a
/// [`Graph`](crate::Graph) or [`StaticGraph`](crate::StaticGraph), give
/// every edge a twin going back.
///
/// It needs the same memory as a `PathFinder`: one [`Node`] per node of
/// the space, and room for the open list ([`Open`] entries).
///
/// ```
/// use vas::grid::Moves;
/// use vas::path::{Node, Open};
/// use vas::{FlowField, Grid, Memory};
///
/// let Ok(map) = Grid::<bool, _>::new(Memory::<bool, 16>::new(), 4, 4, false) else {
///     return;
/// };
/// let space = map.space(Moves::Four, |_, wall| (!wall).then_some(1.0));
///
/// let mut nodes = Memory::<Node, 16>::new();
/// let mut open = Memory::<Open, 32>::new();
/// let mut field = FlowField::new(&mut nodes, &mut open);
/// let exit = map.index(3, 3).unwrap_or(0);
/// assert!(field.build(&space, &[exit], f32::INFINITY).is_ok());
///
/// // Every unit walks downhill.
/// let unit = map.index(0, 0).unwrap_or(0);
/// assert_eq!(field.cost(unit), Some(6.0));
/// assert_eq!(field.route(unit).count(), 7);
/// assert_eq!(field.route(unit).last(), Some(exit));
/// ```
pub struct FlowField<SN: Storage<Node>, SO: Storage<Open> = SN> {
    // Every slot is initialised, so `nodes.len()` is the node capacity.
    // A node's `parent` is its next step; a goal is its own parent.
    nodes: Vector<Node, SN>,
    open: Heap<Open, SO, Cheapest>,
    // Identifies the current field; see `Node::stamp`. 0 means no field.
    stamp: u32,
}

/// Only fields with inline storage can be cloned.
impl<const NODES: usize, const OPEN: usize> Clone for FlowField<Inline<NODES>, Inline<OPEN>> {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            open: self.open.clone(),
            stamp: self.stamp,
        }
    }
}

impl<AN, AO> Default for FlowField<Dynamic<AN>, Dynamic<AO>>
where
    AN: Allocator + Default,
    AO: Allocator + Default,
{
    /// Creates a field that grows through the default allocators.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(AN::default()), Dynamic(AO::default()))
    }
}

/// Offers every neighbour of `current` a route through it within
/// `max_cost`, and queues the ones it improves.
#[inline]
fn expand<S, N, SO>(
    space: &S,
    nodes: &mut N,
    open: &mut Heap<Open, SO, Cheapest>,
    current: &Open,
    max_cost: f32,
    stamp: u32,
) -> Result<(), FlowError>
where
    S: Space,
    N: Nodes + ?Sized,
    SO: Storage<Open>,
{
    let mut error = None;
    space.predecessors(current.node, |next, step| {
        let cost = current.cost + step;
        if error.is_some() || !step.is_finite() || step < 0.0 || cost > max_cost {
            return;
        }
        let Some(node) = nodes.node(next) else {
            error = Some(FlowError::OutOfBounds);
            return;
        };
        if node.stamp == stamp && node.cost <= cost {
            return;
        }
        *node = Node {
            stamp,
            parent: current.node,
            cost,
        };
        let entry = Open {
            priority: cost,
            cost,
            node: next,
        };
        if open.push(entry).is_err() {
            error = Some(FlowError::Full);
        }
    });
    error.map_or(Ok(()), Err)
}

impl<SN: Storage<Node>, SO: Storage<Open>> FlowField<SN, SO> {
    /// Creates an empty field for a space of `nodes.len()` nodes, with
    /// `open` as the memory for its open list.
    ///
    /// The open list holds one entry per improvement found for a node; the
    /// number of nodes is usually plenty, and running out is reported as
    /// [`FlowError::Full`]. Initialises the node memory, so creation is
    /// O(nodes).
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
        }
    }

    /// Number of nodes the field has memory for. With dynamic storage it
    /// grows to the highest node a build touches.
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

    /// Replaces the field by one leading to the nearest of `goals`, over
    /// every node whose route costs at most `max_cost` (pass
    /// `f32::INFINITY` for the whole space).
    ///
    /// Dijkstra's algorithm from all goals at once: O((nodes + edges)
    /// log nodes) for the nodes within `max_cost`. On error no field is
    /// left: every node reads as not reached.
    pub fn build<S: Space>(
        &mut self,
        space: &S,
        goals: &[u32],
        max_cost: f32,
    ) -> Result<(), FlowError> {
        self.open.clear();
        self.next_stamp();
        let result = self.search(space, goals, max_cost);
        if result.is_err() {
            self.next_stamp();
        }
        result
    }

    fn search<S: Space>(
        &mut self,
        space: &S,
        goals: &[u32],
        max_cost: f32,
    ) -> Result<(), FlowError> {
        let stamp = self.stamp;
        for &goal in goals {
            let node = node_mut(&mut self.nodes, goal).ok_or(FlowError::OutOfBounds)?;
            if node.stamp == stamp {
                // Listed twice.
                continue;
            }
            *node = Node {
                stamp,
                parent: goal,
                cost: 0.0,
            };
            let entry = Open {
                priority: 0.0,
                cost: 0.0,
                node: goal,
            };
            self.open.push(entry).map_err(|_| FlowError::Full)?;
        }

        while let Some(current) = self.open.pop() {
            // Skip entries that were superseded by a cheaper route.
            match self.nodes.get(current.node as usize) {
                Some(node) if current.cost <= node.cost => {}
                _ => continue,
            }
            // As in `PathFinder::find`: fixed node memory is searched as a
            // plain slice.
            if <SN::Buffer as Buffer<Node>>::GROWS {
                expand(
                    space,
                    &mut self.nodes,
                    &mut self.open,
                    &current,
                    max_cost,
                    stamp,
                )?;
            } else {
                let nodes = self.nodes.as_mut_slice();
                expand(space, nodes, &mut self.open, &current, max_cost, stamp)?;
            }
        }
        Ok(())
    }

    /// The state of `node` in the current field, when it was reached.
    #[inline]
    fn reached(&self, node: u32) -> Option<&Node> {
        let state = self.nodes.get(node as usize)?;
        (self.stamp != 0 && state.stamp == self.stamp).then_some(state)
    }

    /// The cost of the cheapest route from `node` to its nearest goal, or
    /// `None` when the field does not reach it. 0 at a goal.
    #[inline]
    pub fn cost(&self, node: u32) -> Option<f32> {
        self.reached(node).map(|state| state.cost)
    }

    /// The next node on the cheapest route from `node` to its nearest goal.
    /// `None` at a goal, and when the field does not reach `node`.
    #[inline]
    pub fn next(&self, node: u32) -> Option<u32> {
        let state = self.reached(node)?;
        (state.parent != node).then_some(state.parent)
    }

    /// The whole route from `node` to its nearest goal: `node` first, the
    /// goal last. Empty when the field does not reach `node`.
    #[inline]
    pub fn route(&self, node: u32) -> Route<'_> {
        Route {
            nodes: self.nodes.as_slice(),
            stamp: self.stamp,
            next: self.reached(node).map(|_| node),
            remaining: self.nodes.len(),
        }
    }

    /// Moves to a fresh stamp, which forgets the whole field at once.
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

/// Iterator over a route to the nearest goal. See [`FlowField::route`].
pub struct Route<'f> {
    nodes: &'f [Node],
    stamp: u32,
    next: Option<u32>,
    // Upper bound on the route length; guards against ever looping.
    remaining: usize,
}

impl Iterator for Route<'_> {
    type Item = u32;

    #[inline]
    fn next(&mut self) -> Option<u32> {
        let node = self.next?;
        self.remaining = self.remaining.checked_sub(1)?;
        self.next = self
            .nodes
            .get(node as usize)
            .filter(|state| state.stamp == self.stamp && state.parent != node)
            .map(|state| state.parent);
        Some(node)
    }
}
