# Traversal

`Traversal<SM, SL = SM>` runs breadth-first search, depth-first search and
topological sorting over any [`Space`](path.md#the-space), with
[storage parameters](storage.md#storage-parameter). `Graph` and
`StaticGraph` are spaces; so is any tile grid that lists the neighbours of
a cell. Edge costs are ignored: every listed neighbour is one step. For the
cheapest path by cost use [`PathFinder`](path.md).

| Search | Returns | Cost | Use it for |
|---|---|---|---|
| `breadth_first(&space, start, max_steps)` | nodes reachable from `start`, nearest first; `steps(node)` after | O(nodes + edges reached) | flood fill, "tiles within 5 moves", fewest steps to everything |
| `depth_first(&space, start)` | nodes reachable from `start`, in the order a recursive search visits them | O(nodes + edges reached) | reachability, walking dependencies, maze generation |
| `topological_order(&space, nodes)` | `nodes` ordered so every edge goes forward, or `Cycle` | O(nodes + edges) | build orders, tech trees, update order of systems |

## Memory

Two blocks:

| Block | Size | Notes |
|---|---|---|
| `Memory::<traversal::Mark, N>` | one per node of the space (12 bytes each) | initialised once in `new`, O(N) |
| `Memory::<u32, M>` | the result order, one per node a search may reach | also the queue of breadth-first and topological searches |

Nothing else is used. Depth-first search keeps its stack as a linked list
through the marks, so it needs no stack memory and does not recurse; a
node that is reached again before it is visited is moved, not pushed twice.
A traversal is reusable: each search is stamped, so starting a new one does
not clear the marks.

With `Dynamic` storage the marks grow to the highest node a search
touches, and the order list grows as needed.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `Traversal::new(marks, order)` | O(nodes) |
| create dynamic | `Traversal::<Dynamic<A>, Dynamic<B>>::default()` | when both allocators are `Default` |
| sizes | `node_capacity()` | nodes there are marks for |
| reserve | `reserve(nodes) -> Result<(), Full<()>>` | grows dynamic marks; only checks on fixed storage |
| breadth first | `breadth_first(&space, start, max_steps) -> Result<&[u32], TraversalError>` | `u32::MAX` for no limit |
| depth first | `depth_first(&space, start) -> Result<&[u32], TraversalError>` | pre-order, neighbours in the order the space lists them |
| topological | `topological_order(&space, nodes: impl IntoIterator<Item = u32>) -> Result<&[u32], TraversalError>` | edges to unlisted nodes are ignored; a node listed twice counts once |
| last result | `order() -> &[u32]` | empty after a failed search |
| reached | `reached(node) -> bool` | by the last search |
| steps | `steps(node) -> Option<u32>` | fewest steps from the start; last search must be `breadth_first` |
| clone | `clone()` | inline storage only |

`TraversalError` is `Full` (the order list ran out of room, or the
allocator refused), `OutOfBounds` (a node beyond the marks) or `Cycle`.

For a `Graph`, list the live nodes with
`graph.nodes().map(|(id, _)| id.index())` and turn results back into ids
with `graph.node_at(index)`.

## Guarantees

- No allocation of its own, no panics, no recursion, no `unsafe`.
- A failed search reports why and leaves an empty order.
- Breadth first: each node's `steps` is the fewest edges on any path from
  the start.
- Topological order: for every edge between two listed nodes, the node it
  leaves comes first; among nodes with no order between them, the listed
  order is kept where possible.

## Tests

`tests/traversal.rs`: orders on small graphs, flood fill on a tile grid,
depth first against a recursive search on random graphs, topological order
with and without cycles, errors, an editable `Graph`, dynamic growth, and
10,000 reused searches.
