# k-d tree

`KdTree<'a, T, const D: usize>` arranges a set of points so that the
**nearest** points to any position can be found quickly, over user-provided
memory. See [storage](storage.md) and [aabb](aabb.md).

Use it for "closest enemy / pickup / waypoint to here" and for the k nearest
neighbours. For box and radius queries over points that change, the
[spatial grid](spatial_grid.md) or the [quadtree](orthtree.md) fit better.

## Layout

The tree needs **no memory of its own**. It is your slice of
`(position, value)` pairs, reordered in place: the middle item of every
range splits the rest into a lower and an upper half along one axis, and the
axis cycles with depth (x, y, x, y, ... in 2D).

The tree is **built in one go**, not edited: there is no insert or remove.
Build again when the points change; it is O(n log n) and allocates nothing.

```rust
use vas::KdTree;

let tree = KdTree::build(&mut items); // items: &mut [([f32; 2], T)]
```

## API

| Operation | Signature | Notes |
|---|---|---|
| build | `KdTree::build(&mut [([f32; D], T)])` | reorders the slice; cannot fail |
| sizes | `len`, `is_empty` | |
| items | `items()`, `get(index)`, `get_mut(index)` | `get_mut` gives the value only; positions are fixed |
| nearest | `nearest(&target) -> Option<Neighbor>` | `None` when empty |
| k nearest | `nearest_k(&target, &mut Vector<Neighbor, S>)` | fills `out` nearest first; its capacity is k; any storage |
| box query | `query(&Aabb<D>, \|position, value\| ..)` | exact; boundary included |
| radius query | `query_radius(&center, radius, \|position, value\| ..)` | exact; boundary included |

`Neighbor` is `{ index, distance_squared }`. Distances are squared, so no
square root is taken; `index` goes to `get`.

When several points are equally near, `nearest` returns any one of them.

## NaN

A point with a NaN coordinate is kept in the slice but is never returned by
any search, and it does not hide other points. A NaN target finds nothing.

## Guarantees

- No allocation, no panics, no `unsafe`.
- Results are exact: the true nearest points, and every point in a queried
  region exactly once.
- Recursion is as deep as the tree, about log2(n).
