# Static graph

`StaticGraph<E, SO, SE = SO>` is a directed graph that is built once and then only
read, with [storage parameters](storage.md#storage-parameter). See [storage](storage.md) and
[errors](errors.md).

Use it for graphs that do not change while they are searched: navigation
meshes, waypoint networks, tech trees. Use [`Graph`](graph.md) when nodes or
edges are added or removed at run time.

## Layout

Compressed sparse row: all edges leaving a node are stored next to each
other.

- `offsets`: `node_count + 1` entries of `u32`; the edges of node `i` are
  `edges[offsets[i]..offsets[i + 1]]`.
- `edges`: one `static_graph::Edge<E>` per edge (`to: u32`, `value: E`).

Listing a node's neighbours is a slice lookup: no links to follow, and the
edges are contiguous in memory. Per edge it stores 4 bytes plus the value,
against 24 bytes of links in the dynamic graph.

Nodes are the indices `0..node_count` and carry no value; keep node data in
your own array, indexed the same way.

## Building

```rust
use vas::static_graph::Edge;
use vas::{Memory, StaticGraph};

let mut offsets = Memory::<u32, { NODES + 1 }>::new();
let mut edges = Memory::<Edge<f32>, EDGES>::new();

// (from, to, value), in any order
let graph = StaticGraph::build(NODES, &input, &mut offsets, &mut edges)?;
```

`build` is O(nodes + edges) and needs no memory beyond the two blocks. The
edges of each node keep the order they have in the input. `E` must be `Copy`.

`BuildError` is `Full` (a block is too small) or `OutOfBounds` (an edge
names a node that is not below the node count).

## API

| Operation | Signature | Notes |
|---|---|---|
| build | `StaticGraph::build(node_count, &[(u32, u32, E)], offsets, edges)` | `Result<StaticGraph, BuildError>` |
| sizes | `node_count()`, `edge_count()` | O(1) |
| neighbours | `neighbors(node) -> &[Edge<E>]` | O(1); empty when out of bounds |
| degree | `out_degree(node) -> usize` | O(1) |

There is no way to list the edges entering a node; add reversed edges to a
second graph if that is needed.

## Path search

When the edge value has a cost (`graph::Cost`, implemented for `f32`), the
graph is a `path::Space` and can be handed to a [`PathFinder`](path.md). There
is no heuristic, so the search is Dijkstra's algorithm.

## Guarantees

- No allocation, no panics.
- The graph is immutable once built.

## Storage

There are two blocks of memory, so there are two storage parameters: the
first for the offsets (`node_count + 1` of them), the second for the edges. The second defaults to
the first. See [storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `StaticGraph<E, Inline<65>, Inline<256>>` | inside the container: no lifetime, a plain struct field. The two numbers are the capacities of the two blocks |
| `StaticGraph<E, Borrowed<'a>>` | both blocks provided by the caller: `&mut Memory<..>`, arena pieces or any `&mut [MaybeUninit<..>]` |

Pass `Memory::new()` for both blocks to `build`. The two blocks may also use different kinds of storage. In
a turbofish write both: `::<.., _, _>`.

## Growing

With `Dynamic` storage, `build` takes exactly `node_count + 1` offsets and
`input.len()` edges from the allocator, so nothing has to be sized by hand:
`StaticGraph::build(nodes, &edges, Dynamic(&arena), Dynamic(&arena))`. It
reports `BuildError::Full` when the allocator refuses.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
