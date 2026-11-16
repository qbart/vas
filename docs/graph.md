# Graph

`Graph<N, E, SN, SE = SN>` is a fixed-capacity directed graph, with [storage parameters](storage.md#storage-parameter): nodes carrying a value `N`, connected by edges carrying a value `E`.
Nodes and edges can be added and removed at any time. See
[storage](storage.md) and [errors](errors.md).

Use it for waypoint networks, dependency graphs, state machines, dialogue
graphs and anything edited at run time.

## Layout

Index-based, built on two [`Pool`](pool.md)s: one for nodes, one for edges.

- A node stores its value and the heads of two lists: the edges leaving it
  and the edges entering it (8 bytes of links).
- An edge stores its value, its two end nodes and its place in both lists
  (24 bytes of links). The lists are doubly linked, so removing an edge is
  O(1).
- `NodeId<N>` and `EdgeId<E>` are generational handles: stale once removed.

Edges are directed. For a two-way connection add an edge each way. Parallel
edges and self-loops are allowed.

## Storage

Two blocks, sized independently:

```rust
use vas::graph::{EdgeSlot, NodeSlot};
use vas::{Graph, Memory};

let mut nodes = Memory::<NodeSlot<Waypoint>, 256>::new();
let mut edges = Memory::<EdgeSlot<f32>, 1024>::new();
let mut graph = Graph::new(&mut nodes, &mut edges);
```

## API

| Operation | Signature | Notes |
|---|---|---|
| sizes | `node_count`, `edge_count`, `node_capacity`, `edge_capacity` | O(1) |
| add node | `add_node(N) -> Result<NodeId<N>, Full<N>>` | O(1) |
| add edge | `add_edge(from, to, E) -> Result<EdgeId<E>, InsertError<E>>` | O(1); `Full(E)` or `Stale(E)` |
| remove edge | `remove_edge(id) -> Option<E>` | O(1) |
| remove node | `remove_node(id) -> Option<N>` | also removes **every edge at that node**; O(edges at the node) |
| values | `node`, `node_mut`, `edge`, `edge_mut`, `contains_node`, `contains_edge` | `None` / `false` for a stale id |
| ends | `endpoints(edge) -> Option<(NodeId<N>, NodeId<N>)>` | `(from, to)` |
| neighbours | `outgoing(node)`, `incoming(node)` | `(edge, other node)`, most recently added first |
| search | `find_edge(from, to) -> Option<EdgeId<E>>` | O(edges leaving `from`) |
| everything | `nodes()`, `edges()` | `(id, value)` in no particular order |
| index | `node_at(u32) -> Option<NodeId<N>>` | inverse of `NodeId::index()` |
| clear | `clear()` | drops everything, all ids go stale |

## Path search

When the edge value has a cost (`graph::Cost`, implemented for `f32`), the
graph is a `path::Space` and can be handed to a [`PathFinder`](path.md):

```rust
let cost = finder.find(&graph, start.index(), goal.index())?;
```

Nodes are addressed by `NodeId::index()`, so the finder needs one
`path::Node` per unit of `graph.node_capacity()`. Turn the indices of the
found path back into ids with `graph.node_at(index)`. A graph has no
positions, so there is no heuristic and the search is Dijkstra's algorithm.

## Guarantees

- No allocation, no panics, no `unsafe` of its own.
- An edge never outlives either of its nodes.
- Values still in the graph are dropped when it is dropped.

## Storage

There are two blocks of memory, so there are two storage parameters: the
first for the nodes, the second for the edges. The second defaults to
the first. See [storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `Graph<N, E, Inline<64>, Inline<256>>` | inside the container: no lifetime, a plain struct field. The two numbers are the capacities of the two blocks |
| `Graph<N, E, Borrowed<'a>>` | both blocks provided by the caller: `&mut Memory<..>`, arena pieces or any `&mut [MaybeUninit<..>]` |

`Graph::inline()` and `default()` create it; `inline()` is `const`. The two blocks may also use different kinds of storage. In
a turbofish write both: `::<.., _, _>`.
