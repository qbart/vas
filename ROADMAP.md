# Roadmap

## 0.0.0 — bootstrap

- [x] Empty `no_std` crate with lints, Makefile and docs skeleton
- [x] Reserve the `vas` crate name on crates.io

## 0.1.0 — conventions and vector

- [x] Storage convention spec (`docs/storage.md`): memory is provided by the library user
- [x] Error convention spec (`docs/errors.md`): no panics, `Result` / `Option` everywhere
- [x] Fixed-capacity vector over user-provided memory, exposing its contents as a contiguous slice (e.g. to hand to a renderer)
- [x] `Memory<T, N>` inline storage block
- [x] Vector `insert`, `remove`, `resize`, `move_into`

## 0.2.0 — pool

- [x] Generational pool (`Pool`, `Handle`) over user-provided memory

## 0.3.0 — map

- [x] Unordered map (`Map`) over user-provided memory
- [x] `FastHasher`, replaceable through `BuildHasher`

## 0.4.0 — deque

- [x] Double-ended queue / ring buffer (`Deque`) over user-provided memory

## 0.5.0 — heap

- [x] Binary heap / priority queue (`Heap`) with `Max`, `Min` and custom orders

## 0.6.0 — bitset

- [x] Bit set (`Bitset`) over user-provided memory

## 0.7.0 — random

- [x] Seedable PRNG (`Rng`, xoshiro256++)

## 0.8.0 — path search

- [x] A* / Dijkstra (`PathFinder`) over a neighbours callback (`path::Space`)

## 0.9.0 — tree

- [x] `Tree`: hierarchy (parent / children / siblings) on a pool

## 0.10.0 — binary tree

- [x] `BinaryTree`: binary tree with left / right children

## 0.11.0 — ordered map

- [x] `OrderedMap`: sorted keys, nearest-key and range queries

## 0.12.0 — graph

- [x] `Graph`: dynamic directed graph (add / remove nodes and edges), searchable by `PathFinder`

## 0.13.0 — static graph

- [x] `StaticGraph`: compressed sparse row, built once, read-only

## 0.14.0 — spatial grid

- [x] `Aabb`: axis-aligned box in any dimension
- [x] `SpatialGrid`: uniform grid with box and radius queries

## 0.15.0 — quadtree and octree

- [x] `Quadtree` / `Octree`: one implementation (`Orthtree`) for any dimension

## 0.16.0 — bvh

- [x] `Bvh`: bounding volume hierarchy with box, point and ray queries, and refit

## 0.17.0 — k-d tree

- [x] `KdTree`: nearest and k-nearest neighbours, box and radius queries

## 0.18.0 — arena

- [x] `Arena`: one block shared by many containers and values
- [x] `StaticBlock`: a `'static` block, so containers need no lifetime parameter

## 0.19.0 — heap blocks

- [x] `alloc` feature (off by default): `Arena::from_heap`, `arena::HeapBlock`

## 0.20.0 — size units

- [x] `KB`, `MB`, `GB` constants: `Arena::from_heap(20 * MB)`

## 0.21.0 — size units as functions

- [x] `KB(n)`, `MB(n)`, `GB(n)` replace the constants: `Arena::from_heap(MB(20))`

## 0.22.0 — storage parameter on vector

- [x] `Vector<T, S>`: `Borrowed<'a>` (caller's memory) or `Inline<N>` (own memory, no lifetime)
- [x] `vas::storage`: `Storage`, `Buffer`, `IntoBuffer`
- [x] `move_into` changes the kind of storage; `path_into` and `nearest_k` accept any storage

## 0.23.0 — storage parameter on single-buffer containers

- [x] `Pool`, `Map`, `Deque`, `Heap`, `OrderedMap`, `Bitset`, `Tree`, `BinaryTree` take `S`: `Pool<T, Inline<64>>`, `Map<K, V, Borrowed<'a>>`
- [x] `inline()` (const) and `Default` for inline storage, so a struct can own all its containers
- [x] `Map`'s hasher parameter moved after the storage and renamed `H`
- [x] `alloc` feature: `&mut Vec<MaybeUninit<T>>` and `&mut Box<[MaybeUninit<T>]>` accepted as borrowed storage

## 0.24.0 — storage parameter on two-buffer containers

- [x] `Graph`, `StaticGraph`, `SpatialGrid`, `Quadtree`/`Octree`, `PathFinder` take two storage parameters, the second defaulting to the first
- [x] `Graph::inline()` (const) and `Default`
- [x] `orthtree`, `spatial_grid` and `path` no longer contain `unsafe`

## 0.25.0 — dynamic storage

- [x] `Allocator` trait (`allocate` / `free` / `grow`), implemented by the user
- [x] `Arena` is an allocator: the last piece grows in place and is reclaimed when freed
- [x] `Global` allocator behind the `alloc` feature
- [x] `Dynamic<A>` storage (`A` defaults to `Global` with `alloc`)
- [x] `Vector` grows with dynamic storage; `Vector::reserve`

## 0.26.0 — every container grows

- [x] `Pool`, `Deque`, `Map`, `Bitset`, `Heap`, `OrderedMap`, `Tree`, `BinaryTree`, `Graph` grow with dynamic storage
- [x] `StaticGraph`, `SpatialGrid`, `Quadtree`/`Octree`, `PathFinder` take what they need from the allocator
- [x] `reserve` on `Pool`, `Map`, `Deque`, `Heap`, `OrderedMap`, `PathFinder`
- [x] `Default` for containers with `Dynamic<A>` storage when `A: Default`
- [x] `Map` no longer walks its slots on drop when keys and values need no drop

## 0.27.0 — denser map

- [x] `Map` keeps one tag byte per slot in a block of its own and scans 8 slots at a time; `Slot<K, V>` is exactly a key and a value
- [x] `Map::new(slots, tags)` takes two blocks (breaking); `Dynamic` is passed twice
- [x] Level with `std::HashMap` (same hasher) on inserts at 50,000 keys, hits 1.5x slower, misses 1.5x faster; usable up to 90% full
- [x] `Buffer::replace` can be cancelled by its `transfer` closure
- [x] `cargo bench -- <text>` runs only the matching cases; the benchmark warms the CPU up first
- [x] CI for Forgejo (`.forgejo/workflows/ci.yml`)

## 0.28.0 — clone

- [x] `Clone` for every container with inline storage: `Vector`, `Deque`, `Pool`, `Map`, `Bitset`, `Heap`, `OrderedMap`, `Tree`, `BinaryTree`, `Graph`, `StaticGraph`, `SpatialGrid`, `Quadtree`/`Octree`, `PathFinder`
- [x] Handles and ids of the original work on the copy

## 0.29.0 — tree map

- [x] `TreeMap`: ordered map as a red-black tree, O(log n) insert and remove, with the API of `OrderedMap` (nearest key, ranges, double-ended iteration)
- [x] Nodes stay packed (a removed slot is filled by the last node); no free list
- [x] All three storage kinds, `Clone` with inline storage
- [x] Level to 1.2x slower than `std::BTreeMap` on inserts and removes, 1.5x faster on lookups at 16k entries
- [x] `make bench` measures remove+insert and iteration for the ordered maps

## 0.30.0 — algorithms on the structures

- [x] `Vector`: `retain`, `retain_mut`, `dedup`, `dedup_by`, `dedup_by_key`; stable in-place sort `sort_stable`, `sort_stable_by`, `sort_stable_by_key` (no memory) and `sort_stable_in` (scratch memory of any size from the caller)
- [x] `Deque`: `retain`, `retain_mut`, `make_contiguous`
- [x] `retain` on `Pool`, `Map`, `OrderedMap`, `TreeMap`, `Heap`
- [x] `Tree`: `post_order`, `breadth_first_into`, `common_ancestor`
- [x] `BinaryTree`: `level_order_into`, `common_ancestor`, `height`
- [x] `Traversal`: breadth first (with step counts and a step limit), depth first (recursive order, no recursion, no stack memory), topological order (Kahn) over any `Space`
- [x] `docs/algorithms.md`: which algorithm lives on which structure, and why some are left out
- [x] Complexity table in `README.md`, kept up to date by rule 13

## 0.31.0 — vas's own sorts

- [x] `vas::sort`: `unstable`, `unstable_by`, `unstable_by_key` (quicksort) and `stable`, `stable_by`, `stable_by_key`, `stable_in` (merge sort) on any slice
- [x] `Vector::sort_unstable*` are vas's quicksort, not the slice's
- [x] Never panic on a comparison that is not a total order; same order of equal elements on every compiler and target (pinned by a test)
- [x] Quicksort: branch-free Lomuto partition, pseudo-median pivot, equal-pivot skipping, heapsort after 2 log₂ n partitions
- [x] Short slices sorted without branches: sorting network of four, merges from both ends with a 512-byte stack buffer
- [x] As fast as `core`'s `sort_unstable`; the stable sort gains from the same short-slice sort (30 to 22 ns per element at 100k with scratch)
- [x] Sort benchmarks rotate between 8 inputs, so the branch predictor cannot learn one

## Quality

- [x] Benchmarks against std (`make bench`, `docs/benchmarks.md`)
- [x] Benchmarks against other crates (`make bench-crates`, unpublished `bench/` crate)
- [x] Usage examples, compiled and run by `make check` (`examples/`)
- [x] CI: `make check` and Miri on every push (`.github/workflows/ci.yml`, `.forgejo/workflows/ci.yml`)
- [x] `Deque` against `heapless::Deque`: the 1.6x gap was a cold-CPU measuring error; warmed up it is 1.0 against 1.1 ns (`make bench-crates` now warms up too)

## Later

- [ ] Faster stable sort: `sort_stable_in` is 2x to 4.5x slower than std's driftsort at 100,000 elements (detecting existing runs would help)

- [ ] Growable containers
  - [x] `Allocator` trait (`allocate` / `grow` / `free`) implemented by the user (0.25.0)
  - [x] Storage parameter on `Vector` (0.22.0): `Vector<T, S>` with
        `Borrowed<'a>` and `Inline<N>`
  - [x] Storage parameter on the single-buffer containers (0.23.0): `Pool`,
        `Map`, `Deque`, `Heap`, `OrderedMap`, `Bitset`, `Tree`, `BinaryTree`
  - [x] Storage parameter on the containers with two blocks of memory
        (0.24.0): `Graph`, `StaticGraph`, `SpatialGrid`, `Quadtree`/`Octree`,
        `PathFinder`. `Bvh` and `KdTree` stay views over the user's slice.
  - [x] Growing storage `Dynamic<A>` on `Vector` (0.25.0)
  - [x] Growing on the single-buffer containers (0.26.0): `Pool`, `Map`,
        `Deque`, `Heap`, `OrderedMap`, `Bitset`, `Tree`, `BinaryTree`
  - [x] Growing on the two-buffer containers (0.26.0): `Graph`,
        `StaticGraph`, `SpatialGrid`, `Quadtree`/`Octree`, `PathFinder`
  - [ ] Growing `Map` is 2x slower to fill than `std::HashMap` at 50,000
        keys (faster at 2,000): it grows at 3/4 full, std at 7/8, so it
        ends up with twice the slots at that size
