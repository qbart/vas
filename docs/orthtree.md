# Quadtree and octree

`Orthtree<'a, T, const D: usize>` is a tree that splits space into 2^D equal
parts wherever points are crowded, over user-provided memory. `Quadtree<T>`
is the alias for `D = 2` and `Octree<T>` for `D = 3`; both are the same code.
See [storage](storage.md), [errors](errors.md) and [aabb](aabb.md).

It answers "which points are in this box / within this distance". Choose it
over the [spatial grid](spatial_grid.md) when points are unevenly spread:
dense areas get subdivided, empty areas cost nothing.

## Layout

Index-based, two blocks:

- `nodes`: 16 bytes each. The 2^D children of a split node are stored next
  to each other, so a node needs only the index of the first. Node regions
  are not stored; they are recomputed while walking down.
- `entries`: a [`Pool`](pool.md) of points; each stores its value, its
  position, its leaf and its neighbours in the leaf's list.

Behaviour:

- A leaf is **split** once it holds more than `leaf_size` entries (default
  8), down to `max_depth` levels (default 16, at most 32).
- A split node is **merged** back into a leaf when its subtree drops to half
  of `leaf_size`, and its nodes are reused.
- When the node memory is used up, leaves simply hold more entries. Nothing
  fails; crowded areas just get slower.
- Many points at the same position stop splitting at `max_depth`.

Each entry is a **point**. For objects with a size, insert their centre and
widen the query by the largest object radius.

## Storage

```rust
use vas::orthtree::{Node, Slot};
use vas::{Aabb, Memory, Quadtree};

let world = Aabb::new([0.0, 0.0], [1000.0, 1000.0]);
let mut nodes = Memory::<Node, 1025>::new();           // 1 + 4 per split
let mut entries = Memory::<Slot<Entity, 2>, 4096>::new();
let mut tree = Quadtree::new(world, &mut nodes, &mut entries)?;
```

`node_count()` reports how many nodes a workload actually uses. `NewError`
is `Full` (no node memory) or `Invalid` (non-finite or empty bounds, or `D`
not in 1..=8).

## Outside the bounds

Positions outside the bounds are allowed and still found by any query that
covers them: the outermost children are treated as unbounded outwards. A NaN
position is stored but never matches a query.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(bounds, nodes, entries)`, `with_limits(bounds, leaf_size, max_depth, nodes, entries)` | `Result<_, NewError>`; O(nodes) |
| sizes | `len`, `capacity`, `is_empty`, `is_full`, `bounds`, `node_count` | O(1) |
| insert | `insert(position, T) -> Result<EntryId<T, D>, Full<T>>` | O(depth) |
| remove | `remove(id) -> Option<T>` | O(depth) |
| move | `set_position(id, position) -> bool` | `false` for a stale id |
| value | `get`, `get_mut`, `position` | `None` for a stale id |
| box query | `query(&Aabb<D>, \|id, value\| ..)` | exact; boundary included |
| radius query | `query_radius(center, radius, \|id, value\| ..)` | exact; boundary included |
| everything | `iter()` | `(id, position, value)` in no particular order |
| clear | `clear()` | collapses to the root |

Queries report through a callback, so they need no memory for results.

## Guarantees

- No allocation, no panics.
- Query results are exact: every matching entry is visited once, and no
  other.
- Recursion is bounded by `max_depth` (at most 32 levels).
