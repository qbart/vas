//! Graph traversal (breadth first, depth first, topological order) over
//! borrowed, inline or growing memory.
//!
//! Works on anything that implements [`Space`]: [`Graph`](crate::Graph),
//! [`StaticGraph`](crate::StaticGraph) or your own tile grid. Edge costs
//! are ignored; every listed neighbour is one step.

use core::fmt;

use crate::allocator::Allocator;
use crate::path::Space;
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// "No node", in the links of the depth-first stack.
const NONE: u32 = u32::MAX;

/// Per-node state of a [`Traversal`]. Opaque; it only exists so the user
/// can size the memory: one per node, `Memory::<Mark, N>` (12 bytes each).
#[derive(Clone, Copy)]
pub struct Mark {
    // Which search touched the node last, and how; see `Traversal::stamp`.
    stamp: u32,
    // Breadth first: steps from the start. Depth first: the node above in
    // the stack. Topological order: edges still to come in.
    a: u32,
    // Depth first: the node below in the stack.
    b: u32,
}

const UNSEEN: Mark = Mark {
    stamp: 0,
    a: 0,
    b: 0,
};

/// Why a traversal stopped.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraversalError {
    /// The order list ran out of room. Give the traversal more `u32`
    /// memory, at most one per node.
    Full,
    /// The start or a neighbour is not below the number of nodes the
    /// traversal has memory for.
    OutOfBounds,
    /// Topological order only: the listed nodes have a cycle, so no order
    /// exists.
    Cycle,
}

impl fmt::Display for TraversalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "order list is full",
            Self::OutOfBounds => "node is out of bounds",
            Self::Cycle => "graph has a cycle",
        })
    }
}

impl core::error::Error for TraversalError {}

/// What the last search was, for [`Traversal::steps`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Last {
    None,
    BreadthFirst,
    Other,
}

/// Breadth-first search, depth-first search and topological sorting over a
/// [`Space`], in memory provided by the caller.
///
/// Two blocks: one [`Mark`] per node of the space, and one `u32` per node
/// the search may reach, which receives the order. A traversal is reusable:
/// each search is stamped, so starting a new one does not clear the marks.
///
/// ```
/// use vas::traversal::Mark;
/// use vas::{Memory, StaticGraph, Traversal};
///
/// // Unlocks in a tech tree: an edge means "needed before".
/// let edges = [(0, 2, 1.0), (1, 2, 1.0), (2, 3, 1.0)];
/// let graph: Option<StaticGraph<f32, _, _>> =
///     StaticGraph::build(4, &edges, Memory::<u32, 5>::new(), Memory::<_, 3>::new()).ok();
///
/// let mut marks = Memory::<Mark, 4>::new();
/// let mut order = Memory::<u32, 4>::new();
/// let mut traversal = Traversal::new(&mut marks, &mut order);
/// if let Some(graph) = &graph {
///     assert_eq!(traversal.topological_order(graph, 0..4), Ok(&[0, 1, 2, 3][..]));
///     assert_eq!(traversal.breadth_first(graph, 1, u32::MAX), Ok(&[1, 2, 3][..]));
///     assert_eq!(traversal.steps(3), Some(2));
/// }
/// ```
pub struct Traversal<SM: Storage<Mark>, SL: Storage<u32> = SM> {
    // Every slot is initialised, so `marks.len()` is the node capacity.
    marks: Vector<Mark, SM>,
    order: Vector<u32, SL>,
    // Identifies the current search: a mark stamped `stamp` was reached by
    // it, `stamp + 1` was also finished (output), `stamp + 2` was put on
    // the depth-first stack by the node being expanded. Older stamps mean
    // "not reached". Stamps grow by 3 per search.
    stamp: u32,
    last: Last,
}

impl<AM, AL> Default for Traversal<Dynamic<AM>, Dynamic<AL>>
where
    AM: Allocator + Default,
    AL: Allocator + Default,
{
    /// A traversal that takes memory from `AM::default()` and
    /// `AL::default()` as searches need it.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(AM::default()), Dynamic(AL::default()))
    }
}

/// The copy remembers the last search. Only traversals with inline storage
/// can be cloned.
impl<const MARKS: usize, const ORDER: usize> Clone for Traversal<Inline<MARKS>, Inline<ORDER>> {
    fn clone(&self) -> Self {
        Self {
            marks: self.marks.clone(),
            order: self.order.clone(),
            stamp: self.stamp,
            last: self.last,
        }
    }
}

impl<SM: Storage<Mark>, SL: Storage<u32>> Traversal<SM, SL> {
    /// Creates a traversal on `marks` (one per node of the space) and
    /// `order` (one `u32` per node a search may reach).
    ///
    /// Initialises the marks, so creation is O(nodes). References to memory
    /// borrow it; `Memory` blocks passed by value are stored inside.
    pub fn new<BM, BL>(marks: BM, order: BL) -> Self
    where
        BM: IntoBuffer<Mark, Storage = SM>,
        BL: IntoBuffer<u32, Storage = SL>,
    {
        let mut marks = Vector::new(marks);
        let capacity = marks.capacity();
        // Cannot fail: the length asked for is the capacity.
        let _ = marks.resize_with(capacity, || UNSEEN);
        Self {
            marks,
            order: Vector::new(order),
            stamp: 0,
            last: Last::None,
        }
    }

    /// Number of nodes the traversal has memory for. With dynamic storage
    /// it grows to the highest node a search touches.
    #[inline]
    pub const fn node_capacity(&self) -> usize {
        self.marks.len()
    }

    /// Makes sure there is memory for `nodes` nodes, growing dynamic
    /// storage when needed. Fails without changes when the storage is fixed
    /// and too small, or when the allocator refuses.
    pub fn reserve(&mut self, nodes: usize) -> Result<(), Full<()>> {
        if nodes <= self.marks.len() {
            return Ok(());
        }
        self.marks.resize_with(nodes, || UNSEEN)
    }

    /// The nodes the last search output, in its order. Empty after a
    /// failed search.
    #[inline]
    pub fn order(&self) -> &[u32] {
        self.order.as_slice()
    }

    /// `true` when the last search reached `node`.
    #[inline]
    pub fn reached(&self, node: u32) -> bool {
        self.marks
            .get(node as usize)
            .is_some_and(|mark| self.last != Last::None && mark.stamp >= self.stamp)
    }

    /// Number of steps from the start to `node` in the last
    /// [`breadth_first`](Self::breadth_first) search: the fewest edges on
    /// any path. `None` when `node` was not reached, or the last search was
    /// of another kind.
    #[inline]
    pub fn steps(&self, node: u32) -> Option<u32> {
        if self.last != Last::BreadthFirst || !self.reached(node) {
            return None;
        }
        self.marks.get(node as usize).map(|mark| mark.a)
    }

    /// Visits every node reachable from `start` in at most `max_steps`
    /// steps, nearest first, and returns them in that order. Pass
    /// `u32::MAX` for no limit. O(nodes + edges reached).
    ///
    /// Flood fill, "every tile within 5 moves", the fewest steps to every
    /// node (read with [`steps`](Self::steps)).
    pub fn breadth_first<S: Space>(
        &mut self,
        space: &S,
        start: u32,
        max_steps: u32,
    ) -> Result<&[u32], TraversalError> {
        let stamp = self.begin(Last::BreadthFirst);
        let mark = self.mark(start)?;
        *mark = Mark { stamp, a: 0, b: 0 };
        self.push(start)?;
        let mut cursor = 0;
        while let Some(node) = self.order.get(cursor).copied() {
            cursor += 1;
            let steps = self.marks.get(node as usize).map_or(0, |mark| mark.a);
            if steps >= max_steps {
                continue;
            }
            let mut result = Ok(());
            let (marks, order) = (&mut self.marks, &mut self.order);
            space.neighbors(node, |next, _| {
                if result.is_err() {
                    return;
                }
                let Some(mark) = grow_to(marks, next) else {
                    result = Err(out_of_bounds::<SM>());
                    return;
                };
                if mark.stamp >= stamp {
                    return;
                }
                *mark = Mark {
                    stamp,
                    a: steps + 1,
                    b: 0,
                };
                if order.push(next).is_err() {
                    result = Err(TraversalError::Full);
                }
            });
            self.check(result)?;
        }
        Ok(self.order.as_slice())
    }

    /// Visits every node reachable from `start`, going as deep as possible
    /// before backing up, and returns them in the order they were first
    /// visited (pre-order). The order is the one a recursive search would
    /// give, neighbours in the order the space lists them, but no recursion
    /// and no memory beyond the two blocks is used.
    /// O(nodes + edges reached).
    ///
    /// Reachability, dependency walks, maze generation.
    pub fn depth_first<S: Space>(
        &mut self,
        space: &S,
        start: u32,
    ) -> Result<&[u32], TraversalError> {
        let stamp = self.begin(Last::Other);
        let (on_stack, done, batch) = (stamp, stamp + 1, stamp + 2);
        // The stack is a doubly linked list through the marks (`a` above,
        // `b` below), so a node is on it at most once: when it is reached
        // again before being visited, it is moved to its new place, which
        // is exactly when a recursive search would visit it.
        *self.mark(start)? = Mark {
            stamp: on_stack,
            a: NONE,
            b: NONE,
        };
        let mut top = start;
        while top != NONE {
            let node = top;
            let below = self.marks.get(node as usize).map_or(NONE, |mark| mark.b);
            top = below;
            set(&mut self.marks, below, |mark| mark.a = NONE);
            set(&mut self.marks, node, |mark| mark.stamp = done);
            self.push(node)?;

            // The neighbours go on top, in order: the first one on top, each
            // next one right below the one before.
            let mut last = NONE;
            let mut result = Ok(());
            let marks = &mut self.marks;
            space.neighbors(node, |next, _| {
                if result.is_err() {
                    return;
                }
                let Some(mark) = grow_to(marks, next) else {
                    result = Err(out_of_bounds::<SM>());
                    return;
                };
                let (state, above, under) = (mark.stamp, mark.a, mark.b);
                if state == done || state == batch {
                    // Visited, or listed twice by this node: a recursive
                    // search would visit it at its first listing.
                    return;
                }
                if state == on_stack {
                    // Unlink from its old place.
                    set(marks, under, |mark| mark.a = above);
                    if above == NONE {
                        top = under;
                    } else {
                        set(marks, above, |mark| mark.b = under);
                    }
                }
                let (above, under) = if last == NONE {
                    (NONE, top)
                } else {
                    (last, marks.get(last as usize).map_or(NONE, |mark| mark.b))
                };
                set(marks, next, |mark| {
                    *mark = Mark {
                        stamp: batch,
                        a: above,
                        b: under,
                    };
                });
                set(marks, under, |mark| mark.a = next);
                if above == NONE {
                    top = next;
                } else {
                    set(marks, above, |mark| mark.b = next);
                }
                last = next;
            });
            self.check(result)?;
            // The new entries are now ordinary stack entries.
            // They run from the top down to `last`.
            let mut current = if last == NONE { NONE } else { top };
            while let Some(mark) = self.marks.get_mut(current as usize) {
                mark.stamp = on_stack;
                current = if current == last { NONE } else { mark.b };
            }
        }
        Ok(self.order.as_slice())
    }

    /// Orders `nodes` so that for every edge between two of them, the node
    /// it leaves comes first (Kahn's algorithm). Edges to nodes that are
    /// not listed are ignored; a node listed twice counts once.
    /// O(nodes + edges).
    ///
    /// Build orders, tech trees, system update order, anything with
    /// "must come before". Fails with [`Cycle`](TraversalError::Cycle) when
    /// no such order exists.
    pub fn topological_order<S: Space>(
        &mut self,
        space: &S,
        nodes: impl IntoIterator<Item = u32>,
    ) -> Result<&[u32], TraversalError> {
        let stamp = self.begin(Last::Other);
        let (listed, done) = (stamp, stamp + 1);
        // The listed nodes are chained through `b`, so the order list is
        // free to serve as the queue.
        let mut first = NONE;
        let mut count = 0usize;
        for node in nodes {
            let mark = self.mark(node)?;
            if mark.stamp >= stamp {
                continue;
            }
            *mark = Mark {
                stamp: listed,
                a: 0,
                b: first,
            };
            first = node;
            count += 1;
        }
        // Count the edges coming into each listed node.
        let mut node = first;
        while node != NONE {
            let marks = &mut self.marks;
            space.neighbors(node, |next, _| {
                if let Some(mark) = marks.get_mut(next as usize)
                    && mark.stamp == listed
                {
                    mark.a += 1;
                }
            });
            node = self.marks.get(node as usize).map_or(NONE, |mark| mark.b);
        }
        // Start with the nodes nothing comes into. The chain runs from the
        // last listed node to the first, so walk it into the queue in
        // reverse to keep the listed order among equals.
        let mut node = first;
        while node != NONE {
            if self
                .marks
                .get(node as usize)
                .is_some_and(|mark| mark.a == 0)
            {
                self.push(node)?;
            }
            node = self.marks.get(node as usize).map_or(NONE, |mark| mark.b);
        }
        self.order.as_mut_slice().reverse();
        for node in self.order.as_slice() {
            set(&mut self.marks, *node, |mark| mark.stamp = done);
        }
        let mut cursor = 0;
        while let Some(node) = self.order.get(cursor).copied() {
            cursor += 1;
            let mut result = Ok(());
            let (marks, order) = (&mut self.marks, &mut self.order);
            space.neighbors(node, |next, _| {
                let Some(mark) = marks.get_mut(next as usize) else {
                    return;
                };
                if mark.stamp != listed || result.is_err() {
                    return;
                }
                mark.a -= 1;
                if mark.a == 0 {
                    mark.stamp = done;
                    if order.push(next).is_err() {
                        result = Err(TraversalError::Full);
                    }
                }
            });
            self.check(result)?;
        }
        if self.order.len() < count {
            self.order.clear();
            self.last = Last::None;
            return Err(TraversalError::Cycle);
        }
        Ok(self.order.as_slice())
    }

    /// Starts a new search and returns its stamp.
    fn begin(&mut self, kind: Last) -> u32 {
        self.order.clear();
        self.last = kind;
        match self.stamp.checked_add(3) {
            Some(stamp) if stamp < u32::MAX - 3 => self.stamp = stamp,
            _ => {
                // Out of stamps: forget every old search.
                for mark in self.marks.iter_mut() {
                    mark.stamp = 0;
                }
                self.stamp = 3;
            }
        }
        self.stamp
    }

    /// The mark of `node`, growing dynamic storage when needed.
    #[inline]
    fn mark(&mut self, node: u32) -> Result<&mut Mark, TraversalError> {
        if grow_to(&mut self.marks, node).is_none() {
            self.fail();
            return Err(out_of_bounds::<SM>());
        }
        self.marks
            .get_mut(node as usize)
            .ok_or(TraversalError::OutOfBounds)
    }

    /// Appends `node` to the order.
    #[inline]
    fn push(&mut self, node: u32) -> Result<(), TraversalError> {
        if self.order.push(node).is_err() {
            self.fail();
            return Err(TraversalError::Full);
        }
        Ok(())
    }

    /// Forgets a search that failed part way.
    #[inline]
    fn check(&mut self, result: Result<(), TraversalError>) -> Result<(), TraversalError> {
        if result.is_err() {
            self.fail();
        }
        result
    }

    #[inline]
    fn fail(&mut self) {
        self.order.clear();
        self.last = Last::None;
    }
}

/// The error for a node beyond the marks: with storage that can grow, the
/// only reason is that the allocator refused.
#[inline]
fn out_of_bounds<SM: Storage<Mark>>() -> TraversalError {
    if <SM::Buffer as Buffer<Mark>>::GROWS {
        TraversalError::Full
    } else {
        TraversalError::OutOfBounds
    }
}

/// The mark of `node`, growing dynamic storage when needed.
#[inline]
fn grow_to<SM: Storage<Mark>>(marks: &mut Vector<Mark, SM>, node: u32) -> Option<&mut Mark> {
    let index = node as usize;
    if index >= marks.len() && <SM::Buffer as Buffer<Mark>>::GROWS {
        let wanted = index.checked_add(1)?;
        marks.reserve(wanted - marks.len()).ok()?;
        let capacity = marks.capacity();
        // Cannot fail: the room was just reserved.
        let _ = marks.resize_with(capacity, || UNSEEN);
    }
    marks.get_mut(index)
}

/// Changes the mark of `node`; does nothing for `NONE`.
#[inline]
fn set<SM: Storage<Mark>>(marks: &mut Vector<Mark, SM>, node: u32, change: impl FnOnce(&mut Mark)) {
    if let Some(mark) = marks.get_mut(node as usize) {
        change(mark);
    }
}
