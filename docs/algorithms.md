# Algorithms

Algorithms live on the structure they fit, as methods, and only where they
make sense for that structure: a vector sorts, a tree walks level by level,
a graph has a topological order. Nothing is shared through a general
"collection" trait. Each one works in memory the structure already has, or
in memory the caller passes in.

| Structure | Algorithms | Spec |
|---|---|---|
| `Vector` | `retain`, `retain_mut`, `dedup*`, `sort_stable*` (in place, no memory); every slice algorithm through `Deref`: `sort_unstable`, `binary_search`, `select_nth_unstable`, `reverse`, `rotate`, `contains` | [vector](vector.md) |
| `Deque` | `retain`, `retain_mut`, `make_contiguous` (then any slice algorithm) | [deque](deque.md) |
| `Pool` | `retain` | [pool](pool.md) |
| `Map` | `retain` | [map](map.md) |
| `OrderedMap` | `retain`; lookups, nearest key and ranges are binary searches | [ordered map](ordered_map.md) |
| `TreeMap` | `retain`; nearest key and ranges | [tree map](tree_map.md) |
| `Heap` | `retain` (rebuilds the heap in O(n)) | [heap](heap.md) |
| `Bitset` | `union_with`, `intersect_with`, `difference_with`, `first_set`, `first_clear`, `count` | [bitset](bitset.md) |
| `Tree` | depth first (`descendants`), `post_order`, `breadth_first_into`, `common_ancestor`, `ancestors`, `depth` | [tree](tree.md) |
| `BinaryTree` | `pre_order`, `in_order`, `post_order`, `level_order_into`, `common_ancestor`, `height`, `depth` | [binary tree](binary_tree.md) |
| `Graph`, `StaticGraph`, any `Space` | `Traversal`: breadth first, depth first, topological order; `PathFinder`: A* and Dijkstra | [traversal](traversal.md), [path](path.md) |
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

## Stable sort

`Vector::sort_stable*` needs no memory: runs of 16 are sorted by insertion,
then merged pairwise in place by rotations (split the longer run in the
middle, binary-search where that element belongs in the other, rotate the
two middle pieces past each other, repeat on both sides). O(n log n)
comparisons, O(n log² n) moves, O(log n) stack. Already sorted input takes
O(n).

`sort_stable_in(scratch, cmp)` takes scratch memory of any size from the
caller. A merge whose shorter run fits in it moves that run out and merges
back in one pass, with no branch on the comparison; a merge that does not
fit falls back to rotations and its pieces are tried again. With room for
half the elements every merge is buffered.

Measured on 8-byte elements with random keys (ns per element, see
[benchmarks](benchmarks.md)):

| | 1,000 | 100,000 |
|---|---:|---:|
| `sort_stable_by_key`, no memory | 32 | 109 |
| `sort_stable_in`, 256 elements of scratch | 12 | 34 |
| `sort_stable_in`, n / 2 of scratch | 11.5 | 30 |
| slice `sort_unstable_by_key` | 5 | 4 |
| std `sort_by_key` (allocates n / 2) | 4.5 | 4.5 |

std's stable sort (driftsort) is a far more elaborate algorithm; use the
slice's `sort_unstable` when equal elements may change order, it is as
fast and needs no memory.
