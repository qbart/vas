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

## Quality

- [x] Benchmarks against std (`make bench`, `docs/benchmarks.md`)
- [x] Benchmarks against other crates (`make bench-crates`, unpublished `bench/` crate)
- [x] Usage examples, compiled and run by `make check` (`examples/`)
- [x] CI: `make check` and Miri on every push (`.github/workflows/ci.yml`)

## Later

- [ ] Denser hash map layout (separate metadata) to close the gap to std on large maps
- [ ] Red-black tree
- [ ] `Deque`: find out why it is about 1.6x slower than `heapless::Deque`
- [ ] Growable containers
  - [ ] `Allocator` trait (`allocate` / `grow` / `free`) implemented by the user
  - [ ] Open design question: how containers use the allocator
    - **Separate types** (current preference): today's fixed types stay
      exactly as they are, and a growable vector and map are added as new
      types built on them. Simple fixed case, no extra type parameter; the
      cost is more type names.
    - **One type with a storage parameter**: a single `Vector<T, S>` where
      `S` is either a borrowed block (fixed, as today) or an allocator
      (grows). One type to learn and code works for both; the cost is that
      the extra parameter appears in every signature, and `push` means two
      different things (fail when full, or grow).
  - [ ] Naming is undecided (if separate types): `DynVector` / `DynMap`,
        `GrowVector` / `GrowMap`, or the same names in their own module
        (`vas::grow::Vector`)
