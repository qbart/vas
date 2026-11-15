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

## Planned

All structures are index-based (nodes live in user-provided blocks and refer
to each other by index or handle), never pointer-based.

- [ ] 0.9.0 — `Tree`: hierarchy (parent / children / siblings) on a pool
- [ ] 0.10.0 — `BinaryTree`: binary tree with left / right children
- [ ] 0.11.0 — `OrderedMap`: sorted keys, range queries
- [ ] 0.12.0 — `Graph`: dynamic graph (add / remove nodes and edges)
- [ ] 0.13.0 — Static graph (compressed sparse row), built once, read-only
- [ ] 0.14.0 — Spatial grid
- [ ] 0.15.0 — Quadtree
- [ ] 0.16.0 — Octree
- [ ] 0.17.0 — BVH
- [ ] 0.18.0 — k-d tree

## Later

- [ ] Red-black tree
- [ ] Allocator trait for containers that grow on their own
