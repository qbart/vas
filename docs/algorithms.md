# Algorithms

Algorithms live on the structure they fit, as methods, and only where they
make sense for that structure: a vector sorts, a tree walks level by level,
a graph has a topological order. Nothing is shared through a general
"collection" trait. Each one works in memory the structure already has, or
in memory the caller passes in.

| Structure | Algorithms | Spec |
|---|---|---|
| `Vector` | `retain`, `retain_mut`, `dedup*`; vas's own `sort_unstable*` (quicksort) and `sort_stable*` (merge sort), in place, no memory; slice algorithms through `Deref`: `binary_search`, `select_nth_unstable`, `reverse`, `rotate`, `contains` | [vector](vector.md), [sort](sort.md) |
| any slice | `vas::sort::{unstable, stable, stable_in}` and their `_by` / `_by_key` forms | [sort](sort.md) |
| `Deque` | `retain`, `retain_mut`, `make_contiguous` (then `vas::sort` or any slice algorithm) | [deque](deque.md) |
| `Pool` | `retain` | [pool](pool.md) |
| `Map` | `retain` | [map](map.md) |
| `OrderedMap` | `retain`; lookups, nearest key and ranges are binary searches | [ordered map](ordered_map.md) |
| `TreeMap` | `retain`; nearest key and ranges | [tree map](tree_map.md) |
| `Heap` | `retain` (rebuilds the heap in O(n)) | [heap](heap.md) |
| `Bitset` | `union_with`, `intersect_with`, `difference_with`, `first_set`, `first_clear`, `count` | [bitset](bitset.md) |
| `Tree` | depth first (`descendants`), `post_order`, `breadth_first_into`, `common_ancestor`, `ancestors`, `depth` | [tree](tree.md) |
| `BinaryTree` | `pre_order`, `in_order`, `post_order`, `level_order_into`, `common_ancestor`, `height`, `depth` | [binary tree](binary_tree.md) |
| `Grid` | `line_of_sight`, `field_of_view` (symmetric shadowcasting), `grid::line` (Bresenham); `neighbors` (4 or 8), `offset`, `fill_rect`; `space(moves, cost)` makes it a `Space` for the row below | [grid](grid.md) |
| `Graph`, `StaticGraph`, `Grid`, any `Space` | `Traversal`: breadth first, depth first, topological order; `PathFinder`: A* and Dijkstra | [traversal](traversal.md), [path](path.md) |
| `SpatialGrid`, `Quadtree` / `Octree`, `Bvh`, `KdTree` | box, radius, ray and nearest queries | their specs |

Not provided, on purpose: sorting a deque in place without
`make_contiguous` (a ring is not a slice), sorting a map or a pool (no
order to keep), a general `find` (`iter().find` and `iter().position`
already do it on everything that iterates), and hierarchical path search
(clustering depends on the game).

## Retain

Every structure that removes elements has `retain`. `keep` is called once
per element; elements it rejects are removed and dropped, so their handles
and ids go stale. A panic in `keep` leaves the structure valid.

| Structure | Cost | Call order |
|---|---|---|
| `Vector`, `Deque`, `OrderedMap` | O(n) | element order |
| `TreeMap` | O(n log n) | key order |
| `Pool` | O(capacity) | slot order |
| `Map` | O(capacity) | no particular order |
| `Heap` | O(n) | no particular order |

`Map::retain` marks the rejected slots in one pass and closes the holes in
a second, because closing a hole right away can pull an entry that was
already visited (from the start of the block, around the end) into a slot
that was not.

## Sorting

`vas::sort` has a quicksort (`unstable*`) and a merge sort (`stable*`,
`stable_in`), vas's own rather than `core`'s: they never panic, even on a
comparison that is not a total order, and the unstable one puts equal
elements in the same order on every compiler and target. The quicksort is
as fast as `core`'s; the stable sort is 1.5x to 4.5x slower than std's,
which allocates. See [sort](sort.md).
