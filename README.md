# vas

vas is a collection of data containers, structures and algorithms for game development in Rust focused on speed and memory control.

- `no_std`, zero dependencies
- never allocates on its own: every container works on memory you provide, or grows through an allocator you choose (the optional `alloc` feature adds heap blocks and the global allocator)
- never panics: fallible operations return `Result` / `Option`

```rust
use vas::{Memory, Vector};

let mut memory = Memory::<f32, 1024>::new();
let mut v = Vector::new(&mut memory);

assert!(v.push(1.0).is_ok());
assert!(v.extend_from_slice(&[2.0, 3.0]).is_ok());
assert_eq!(v.as_slice(), &[1.0, 2.0, 3.0]); // one contiguous block
```

Memory:

- `Memory`: an inline block for one container
- `KB`, `MB`, `GB`: units for sizes, as in `Arena::from_heap(MB(20))`
- `Arena`: one big block shared by many containers and values
- `Arena::from_heap(bytes)`, `HeapBlock`: that block from the heap (`alloc` feature, off by default)
- `Allocator`: the trait behind growing containers; implemented by `Arena` and, with `alloc`, `Global`

Containers so far. All but `Bvh` and `KdTree` take a storage parameter (see
`docs/storage.md`): `Pool<T, Inline<64>>` holds
its own memory and is a plain struct field, `Pool<T, Borrowed<'a>>` uses
memory you provide (a `Memory` block or an `Arena` piece).
`Pool<T, Dynamic<A>>` grows through the allocator `A`; with the `alloc`
feature `Pool<T, Dynamic>` grows on the heap.

- `Vector`: contiguous vector
- `Pool`: store with generational handles
- `Map`: unordered map
- `OrderedMap`: map sorted by key (sorted block; small or mostly read)
- `TreeMap`: map sorted by key (red-black tree; large and changing)
- `Deque`: double-ended queue (ring buffer)
- `Heap`: priority queue
- `Bitset`: set of bits
- `Tree`: hierarchy (parent / children / siblings)
- `BinaryTree`: binary tree
- `Graph`: directed graph
- `StaticGraph`: read-only directed graph, built once
- `Grid`: tile map, one value per cell, searchable with `PathFinder` and `Traversal`
- `SpatialGrid`: uniform grid for box and radius queries (2D, 3D, ...)
- `Quadtree` / `Octree`: adaptive space subdivision for box and radius queries
- `Bvh`: bounding volume hierarchy for box, point and ray queries
- `KdTree`: nearest-neighbour search

## Complexity

n is the number of elements, c the capacity, k the number of results,
d the depth of a node, h the height of a tree. "Amortised" applies to
`Dynamic` storage only, where an insert sometimes grows the block; with
fixed storage the cost is the same without the word.

| Structure | Insert | Remove | Lookup | Iterate | Use it for |
|---|---|---|---|---|---|
| `Vector` | push O(1) amortised, insert at i O(n) | pop, `swap_remove` O(1); remove at i O(n) | by index O(1); `binary_search` on sorted O(log n) | O(n), contiguous | particles, draw lists, anything walked every frame |
| `Pool` | O(1) | O(1) | by handle O(1), stale handles detected | O(c) | entities, bullets, sounds: anything referred to by id |
| `Map` | O(1) average | O(1) average | O(1) average | O(c) | id to entity, name to asset, tile to state |
| `OrderedMap` | O(n) (shifts) | O(n) (shifts); `pop_last` O(1) | O(log n); nearest key, range start O(log n) | O(n), contiguous | keyframes, timed events, small sorted tables read every frame |
| `TreeMap` | O(log n) | O(log n) | O(log n); nearest key, range start O(log n) | O(n), follows links | large sorted sets that change often: event queues with cancel, score boards |
| `Deque` | push either end O(1) amortised | pop either end O(1) | by index O(1) | O(n) | input buffers, command queues, last-N history (`push_overwrite`) |
| `Heap` | O(log n) | pop top O(log n) | top O(1) | O(n), heap order | priority queues: scheduling, AI tasks, event timers |
| `Bitset` | set bit O(1) | clear bit O(1) | test bit O(1); first set / clear O(c / 64) | O(c / 64) | visibility flags, fog of war, component masks |
| `Tree` | O(1) | node and subtree O(subtree) | by id O(1); depth O(d) | O(n) | scene graphs, UI hierarchies, skeletons |
| `BinaryTree` | O(1) at a free child | node and subtree O(subtree) | by id O(1); depth O(d) | pre / in / post order O(n) | decision trees, expression trees, BSP |
| `Graph` | node, edge O(1) | edge O(1); node O(edges at it) | node, edge O(1); `find_edge` O(out-degree) | O(c) | road networks, quest and dialogue graphs, editable navigation |
| `StaticGraph` | built once, O(nodes + edges) | no | neighbours O(1) | O(nodes + edges) | baked navigation meshes, tech trees, fixed level graphs |
| `Grid` | set a cell O(1); created full, O(width × height) | no (fixed size) | by `(x, y)` or index O(1); neighbours O(1) | O(n), contiguous rows | tile maps, walls and floors, height and influence maps; path search on tiles |
| `SpatialGrid` | O(1) | O(1); move O(1) | box / radius query O(cells covered + k) | O(c) | many similar-sized moving objects: units, bullets, collision candidates |
| `Quadtree` / `Octree` | O(h) | O(h) | box / radius query O(log n + k) typical | O(c) | unevenly spread objects: open worlds, clustered props |
| `Bvh` | build O(n log n), `refit` O(n) | rebuild | box / point / ray query O(log n + k) typical | O(n) | static or slowly moving shapes: level geometry, ray picking |
| `KdTree` | build O(n log n) | rebuild | nearest O(log n) typical; radius O(log n + k) | O(n) | nearest target, nearest cover point, point clouds |

| Algorithm | Cost | Use it for |
|---|---|---|
| `Rng` | O(1) per number | loot rolls, procedural generation, shuffles; seedable and replayable |
| `PathFinder` | A* / Dijkstra, O((nodes + edges) log nodes) worst case | unit movement on tile grids (`Grid::space`), navigation graphs |
| `grid::line` (Bresenham) | O(1) per cell | bullets, lasers, drawing on tiles; the same cells both ways |
| `Grid::line_of_sight` | O(distance) | can this guard see that cell; symmetric |
| `Grid::field_of_view` (symmetric shadowcasting) | O(cells within the radius), no memory | fog of war, roguelike vision, light radius; a sees b exactly when b sees a |
| `Traversal` breadth first | O(nodes + edges reached) | flood fill, tiles within N moves, fewest steps |
| `Traversal` depth first | O(nodes + edges reached), no recursion | reachability, dependency walks, maze generation |
| `Traversal` topological order | O(nodes + edges) | build orders, tech trees, system update order |
| `sort::unstable` (quicksort) | O(n log n) worst case, O(n) sorted, no memory | draw-call order, depth sorting, sorting before `dedup` or binary search; same result on every machine |
| `sort::stable`, `stable_in` (merge sort) | O(n log n) comparisons; moves O(n log² n) without scratch, O(n log n) with n / 2 | ordering by one field while keeping an earlier order: UI lists, sorting by layer after sorting by name |

Algorithms on the structures (see [docs/algorithms.md](docs/algorithms.md)):

| Structure | Algorithm | Cost |
|---|---|---|
| `Vector` | `retain`, `dedup` | O(n) |
| `Grid` | `space` (searched by `PathFinder` and `Traversal`, costs as above); `neighbors`; `fill_rect` | O(1) per neighbour; O(area) |
| `Vector`, any slice | `sort_unstable` / `sort::unstable` (quicksort, no memory, never panics) | O(n log n) worst case; O(n) when sorted or reversed; O(n log k) for k distinct keys |
| `Vector`, any slice | `sort_stable` / `sort::stable` (merge sort, no memory) | O(n log n) comparisons, O(n log² n) moves; O(n) when sorted |
| `Vector`, any slice | `sort_stable_in` / `sort::stable_in` (scratch from the caller) | O(n log n) with n / 2 scratch |
| `Vector` | slice `binary_search`, `select_nth_unstable` | O(log n), O(n) |
| `Deque` | `retain`; `make_contiguous` | O(n); O(capacity) when wrapped |
| `Pool`, `Map` | `retain` | O(capacity) |
| `OrderedMap`, `Heap` | `retain` | O(n) |
| `TreeMap` | `retain` | O(n log n) |
| `Tree` | `post_order`, `breadth_first_into` | O(subtree) |
| `Tree`, `BinaryTree` | `common_ancestor` | O(depth) |
| `BinaryTree` | `level_order_into`, `height` | O(subtree) |


Algorithms so far:

- `Rng`: seedable pseudo-random number generator
- `PathFinder`: A* / Dijkstra shortest path
- `Traversal`: breadth first, depth first, topological order over graphs and grids
- `grid::line`, `Grid::line_of_sight`, `Grid::field_of_view`: lines and sight on tile grids
- `sort`: quicksort and stable merge sort on any slice; never panic, no allocation

Runnable samples for every structure are in [examples](examples); run one
with `cargo run --example containers` (or `arena`, `hierarchy`, `pathfinding`,
`spatial`). Specs are in [docs](docs), measurements in
[docs/benchmarks.md](docs/benchmarks.md), progress in [ROADMAP.md](ROADMAP.md).
