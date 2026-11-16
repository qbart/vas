# Path search

`PathFinder<'a>` finds the cheapest path between two nodes with A* (or
Dijkstra's algorithm when no heuristic is given), over user-provided memory.
See [storage](storage.md) and [errors](errors.md).

## The space

The search does not need a graph structure. Implement `path::Space` for
whatever you are searching:

```rust
impl Space for Level {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        // call visit(neighbor, cost) for every step that can be taken
    }

    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        // optional: estimated remaining cost, never an overestimate
    }
}
```

- Nodes are dense `u32` indices in `0..node_count`, e.g. `y * width + x`.
- Edges are directed: list a step in both nodes to make it two-way.
- Costs must be finite and not negative; other costs are ignored.
- The heuristic defaults to zero (Dijkstra). A heuristic that overestimates
  makes the search faster but the path may not be the cheapest.

## Memory

Two blocks:

| Block | Size | Notes |
|---|---|---|
| `Memory::<path::Node, N>` | one per node of the space (12 bytes each) | initialised once in `new`, O(N) |
| `Memory::<path::Open, M>` | the open list (12 bytes each) | one entry per improvement found; worst case is the number of edges |

A finder is reusable: each search is stamped, so starting a new one does not
clear the node memory.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `PathFinder::new(nodes, open)` | O(nodes) |
| search | `find(&space, start, goal) -> Result<f32, PathError>` | returns the path cost |
| path, backwards | `path_rev()` | iterator from goal to start; no extra memory |
| path, forwards | `path_into(&mut Vector<u32, S>) -> Result<usize, Full<()>>` | appends start to goal; unchanged when it does not fit; any storage |
| cost | `cost(node) -> Option<f32>` | cheapest known cost of a node reached by the last search |

`PathError` is `NoPath`, `Full` (open list out of room) or `OutOfBounds`
(start, goal or a neighbour is not below the node count).

## Guarantees

- No allocation, no panics.
- With a heuristic that never overestimates, the returned path is a cheapest
  one.
- Running out of open-list memory is reported as `Full`; it never yields a
  wrong path.
