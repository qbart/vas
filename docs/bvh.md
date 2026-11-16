# BVH

`Bvh<'a, T, const D: usize>` is a bounding volume hierarchy: a tree of nested
boxes over a set of items that each have a bounding box, over user-provided
memory. See [storage](storage.md), [errors](errors.md) and [aabb](aabb.md).

Unlike the grid and the quadtree, items here have a **size**. Use it for
collision broad phase, picking, line of sight (ray queries) and culling.

## Layout

- `items`: your slice of `(Aabb<D>, T)` pairs. The build reorders it in
  place so that every node covers a contiguous range of it.
- `nodes`: a box, a child index and an item range each. The two children of
  a node are stored next to each other.

The tree is **built in one go**, not edited: there is no insert or remove.
For a changing set, build again; for a moving set, `refit`.

## Building

```rust
use vas::bvh::Node;
use vas::{Aabb, Bvh, Memory};

let mut nodes = Memory::<Node<3>, { 2 * COUNT - 1 }>::new();
let mut bvh = Bvh::build(&mut items, &mut nodes)?;
```

- Each node is split at the median along the axis on which the box centres
  spread the most. The tree is balanced: its depth is about log2(n).
- O(n log n), no memory beyond the two blocks.
- `build` puts up to 4 items in a leaf; `build_with(items, nodes, leaf_size)`
  chooses.
- A full tree takes at most `2n - 1` nodes. With less node memory some
  leaves hold more items: still correct, just slower. `BuildError::Full`
  only when there are items and no node at all.

The median split is simple and predictable. A surface-area heuristic would
give somewhat faster queries for very uneven scenes; it is not implemented.

## Moving items

Change the boxes through `items_mut()`, then call `refit()`: it recomputes
every node's box in O(n) and keeps the tree's shape. Queries stay exact.
They slow down gradually as items drift far from where they were at build
time; building again restores the speed.

Querying after changing a box without calling `refit()` may miss items.

## API

| Operation | Signature | Notes |
|---|---|---|
| build | `Bvh::build(items, nodes)`, `Bvh::build_with(items, nodes, leaf_size)` | `Result<_, BuildError>` |
| sizes | `len`, `is_empty`, `node_count`, `bounds` | `bounds` is `None` when empty |
| items | `items()`, `items_mut()` | in the tree's order |
| update | `refit()` | O(n) |
| box query | `query(&Aabb<D>, \|aabb, value\| ..)` | boxes that overlap or touch |
| point query | `query_point(&point, \|aabb, value\| ..)` | boxes that contain the point |
| ray query | `query_ray(&origin, &direction, max_distance, \|aabb, value, distance\| ..)` | boxes the ray crosses, with the entry distance |

Ray results come in no particular order, not nearest first, and are tested
against the item's box: do the exact shape test yourself. `direction` need
not be normalised; distances are in multiples of its length.

Queries report through a callback and use a fixed 64-entry stack, so they
need no memory for results.

## Guarantees

- No allocation, no panics.
- After a build or a refit, query results are exact with respect to the
  items' boxes.
