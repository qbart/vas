# Benchmarks

Run with `make bench`. vas has no dependencies, so there is no benchmarking
crate: `benches/bench.rs` is a small harness that runs each case 15 times
and reports the fastest run, in nanoseconds per operation.

Treat the numbers as a rough guide, not a precise measurement:

- one machine (AMD Ryzen AI 9 HX 370, Linux, rustc 1.99.0), release build;
- no statistics; repeated runs differ by 10-30% on some cases;
- synthetic workloads with random `u32` keys or uniformly spread points.

## Summary

| Structure | Against | Result |
|---|---|---|
| `Vector` | `Vec` | same speed |
| `Deque` | `VecDeque` | slightly faster |
| `Heap` | `BinaryHeap` | same speed |
| `Pool` | `Vec<Option<Box<T>>>` | about 9x faster (no allocation per value) |
| `Map`, 2k keys | `HashMap` with the same hasher | faster inserts, hits level, misses slower |
| `Map`, 50k keys | `HashMap` with the same hasher | 1.5x to 3x slower |
| `Map` | `HashMap` with its default hasher | faster inserts; hits level at 50k and faster at 2k; misses 2x slower at 50k |
| `OrderedMap` lookups | `BTreeMap` | slightly slower when small, 3x faster at 16k |
| `OrderedMap` inserts | `BTreeMap` | slower, and it gets worse with size (4x at 16k) |
| `Bitset` | `Vec<bool>` | 2x slower per operation, 8x less memory |
| `SpatialGrid` radius query | brute force | about 50x faster |
| `Quadtree` radius query | brute force | about 13x faster |
| `KdTree` nearest | brute force | about 19x faster |

## What the numbers say

**Map.** The hasher is not the weak point: `std`'s `HashMap` gets about
twice as fast when given `FastHasher` instead of its default. The table
layout is. `std`'s table keeps one byte of metadata per slot and scans it
16 at a time; `vas::Map` stores a 4-byte tag next to every key and value
and walks slot by slot. While the table fits in the CPU cache (2k keys) the
two are level. Once it does not (50k keys, 1.2 MB at half fill), the bigger
footprint costs 1.5x to 3x.

Fill level matters a lot for keys that are **not** in the map: at 90% full a
miss takes about 4x (50k keys) to 15x (2k keys) as long as at 50%. Hits slow
down about 2x. Keep the map at most half full if misses are common.

Robin Hood probing was tried as a fix. It cut misses at 90% full from 40 ns
to 16 ns, but made inserts about twice as slow and hits slightly slower at
half fill, so it was not kept.

**Heap.** The first version swapped elements pairwise and was about 50%
slower than `BinaryHeap`. It now moves a gap through the heap, as `std`
does, and is level.

**OrderedMap.** A sorted array: lookups are a binary search over contiguous
memory, which beats `BTreeMap` once the map is large. Every insert shifts
the entries after it, so building a 16k map one key at a time is 4x slower
than `BTreeMap`. This is the case the planned red-black tree is for.

**Spatial structures.** For uniformly spread points and queries about the
size of a cell, the grid is the fastest by a wide margin and the cheapest to
fill. The quadtree and the k-d tree are for when points are unevenly spread
or the nearest neighbour is wanted; this benchmark does not show them at
their best.

**Path search.** On a 128x128 grid with 20% walls, corner to corner, A* with
a Manhattan heuristic takes about 0.04 ms and Dijkstra about 0.5 ms.

## Results

| Case | Implementation | ns / op |
|---|---|---:|
| push 100k u64 | vas::Vector | 0.8 |
| push 100k u64 | std Vec (reserved) | 0.8 |
| push 100k u64 | std Vec (growing) | 0.8 |
| queue 100k push+pop | vas::Deque | 2.2 |
| queue 100k push+pop | std VecDeque | 1.6 |
| heap 50k push then pop | vas::Heap | 18.0 |
| heap 50k push then pop | std BinaryHeap | 17.2 |
| map 2000 keys: insert | vas::Map (50% full) | 1.7 |
| map 2000 keys: insert | std HashMap (SipHash) | 5.7 |
| map 2000 keys: insert | std HashMap + vas FastHasher | 2.8 |
| map 2000 keys: hits | vas::Map (50% full) | 1.4 |
| map 2000 keys: misses | vas::Map (50% full) | 1.6 |
| map 2000 keys: hits | vas::Map (90% full) | 3.5 |
| map 2000 keys: misses | vas::Map (90% full) | 24.9 |
| map 2000 keys: hits | std HashMap (SipHash) | 3.8 |
| map 2000 keys: misses | std HashMap (SipHash) | 3.4 |
| map 2000 keys: hits | std HashMap + vas FastHasher | 1.3 |
| map 2000 keys: misses | std HashMap + vas FastHasher | 1.0 |
| map 2000 keys: insert then remove | vas::Map (50% full) | 2.5 |
| map 2000 keys: insert then remove | std HashMap + vas FastHasher | 2.8 |
| map 50000 keys: insert | vas::Map (50% full) | 4.8 |
| map 50000 keys: insert | std HashMap (SipHash) | 6.6 |
| map 50000 keys: insert | std HashMap + vas FastHasher | 3.3 |
| map 50000 keys: hits | vas::Map (50% full) | 4.7 |
| map 50000 keys: misses | vas::Map (50% full) | 10.5 |
| map 50000 keys: hits | vas::Map (90% full) | 10.0 |
| map 50000 keys: misses | vas::Map (90% full) | 40.1 |
| map 50000 keys: hits | std HashMap (SipHash) | 4.7 |
| map 50000 keys: misses | std HashMap (SipHash) | 5.6 |
| map 50000 keys: hits | std HashMap + vas FastHasher | 1.6 |
| map 50000 keys: misses | std HashMap + vas FastHasher | 3.2 |
| map 50000 keys: insert then remove | vas::Map (50% full) | 8.8 |
| map 50000 keys: insert then remove | std HashMap + vas FastHasher | 5.4 |
| ordered 32: build | vas::OrderedMap | 10.6 |
| ordered 32: build | std BTreeMap | 13.4 |
| ordered 32: lookups | vas::OrderedMap | 2.5 |
| ordered 32: lookups | std BTreeMap | 2.5 |
| ordered 1024: build | vas::OrderedMap | 26.6 |
| ordered 1024: build | std BTreeMap | 14.0 |
| ordered 1024: lookups | vas::OrderedMap | 5.6 |
| ordered 1024: lookups | std BTreeMap | 5.3 |
| ordered 16384: build | vas::OrderedMap | 218.6 |
| ordered 16384: build | std BTreeMap | 51.5 |
| ordered 16384: lookups | vas::OrderedMap | 12.3 |
| ordered 16384: lookups | std BTreeMap | 40.6 |
| pool 50k insert+get+remove | vas::Pool | 0.5 |
| pool 50k insert+get+remove | std Vec<Option<Box<u64>>> | 4.4 |
| bitset 100k insert+test | vas::Bitset | 0.4 |
| bitset 100k insert+test | std Vec<bool> | 0.2 |
| rng 1M values | vas::Rng u64 | 0.5 |
| rng 1M values | vas::Rng u32_below(6) | 0.9 |
| rng 1M values | vas::Rng f32 | 0.6 |
| 10k points: 1k radius queries | brute force | 3027.9 |
| 10k points: 1k radius queries | vas::SpatialGrid 32x32 | 62.6 |
| 10k points: 1k radius queries | vas::Quadtree | 225.8 |
| 10k points: 1k radius queries | vas::KdTree | 175.1 |
| 10k points: 1k nearest | brute force | 1851.8 |
| 10k points: 1k nearest | vas::KdTree | 99.9 |
| 10k points: build | vas::KdTree | 39.5 |
| 10k points: build | vas::SpatialGrid 32x32 | 3.1 |
| 10k points: build | vas::Quadtree | 17.6 |
| 128x128 grid, corner to corner | vas::PathFinder A* | 34966.0 |
| 128x128 grid, corner to corner | vas::PathFinder Dijkstra | 549943.0 |
