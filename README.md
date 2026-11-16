# vas

vas is a collection of data containers, structures and algorithms for game development in Rust focused on speed and memory control.

- `no_std`, zero dependencies
- never allocates: every container works on memory you provide (the optional `alloc` feature adds heap blocks you ask for by size)
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
- `Arena`: one big block shared by many containers and values
- `Arena::from_heap(bytes)`, `HeapBlock`: that block from the heap (`alloc` feature, off by default)

Containers so far:

- `Vector`: fixed-capacity contiguous vector
- `Pool`: fixed-capacity store with generational handles
- `Map`: fixed-capacity unordered map
- `OrderedMap`: fixed-capacity map sorted by key
- `Deque`: fixed-capacity double-ended queue (ring buffer)
- `Heap`: fixed-capacity priority queue
- `Bitset`: fixed-capacity set of bits
- `Tree`: fixed-capacity hierarchy (parent / children / siblings)
- `BinaryTree`: fixed-capacity binary tree
- `Graph`: fixed-capacity directed graph
- `StaticGraph`: read-only directed graph, built once
- `SpatialGrid`: uniform grid for box and radius queries (2D, 3D, ...)
- `Quadtree` / `Octree`: adaptive space subdivision for box and radius queries
- `Bvh`: bounding volume hierarchy for box, point and ray queries
- `KdTree`: nearest-neighbour search

Algorithms so far:

- `Rng`: seedable pseudo-random number generator
- `PathFinder`: A* / Dijkstra shortest path

Runnable samples for every structure are in [examples](examples); run one
with `cargo run --example containers` (or `arena`, `hierarchy`, `pathfinding`,
`spatial`). Specs are in [docs](docs), measurements in
[docs/benchmarks.md](docs/benchmarks.md), progress in [ROADMAP.md](ROADMAP.md).
