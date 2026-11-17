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
