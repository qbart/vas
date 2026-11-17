# Benchmarks

There are two sets: against the standard library (this section onwards) and
[against other crates](#against-other-crates).

## Against the standard library

Run with `make bench` (`cargo bench --all-features -- map` runs only the
cases whose name contains `map`). vas has no dependencies, so there is no benchmarking
crate: `benches/bench.rs` is a small harness that runs each case 15 times
and reports the fastest run, in nanoseconds per operation.

Treat the numbers as a rough guide, not a precise measurement:

- one machine (AMD Ryzen AI 9 HX 370, Linux, rustc 1.99.0), release build;
- no statistics; repeated runs differ by 10-30% on some cases;
- synthetic workloads with random `u32` keys or uniformly spread points.

### Summary

| Structure | Against | Result |
|---|---|---|
| `Vector` | `Vec` | same speed |
| `Deque` | `VecDeque` | slightly faster |
| `Heap` | `BinaryHeap` | same speed |
| `Pool` | `Vec<Option<Box<T>>>` | about 9x faster (no allocation per value) |
| `Map`, 2k keys | `HashMap` with the same hasher | inserts level, lookups 1.4x to 1.9x slower (under 1 ns) |
| `Map`, 50k keys | `HashMap` with the same hasher | inserts level, hits 1.5x slower, misses 1.5x faster |
| `Map` | `HashMap` with its default hasher | about 2x faster at everything |
| `OrderedMap` lookups | `BTreeMap` | slightly slower when small, 3x faster at 16k |
| `OrderedMap` inserts | `BTreeMap` | slower, and it gets worse with size (4x at 16k) |
| `Bitset` | `Vec<bool>` | 2x slower per operation, 8x less memory |
| `SpatialGrid` radius query | brute force | about 50x faster |
| `Quadtree` radius query | brute force | about 13x faster |
| `KdTree` nearest | brute force | about 19x faster |

### What the numbers say

**Map.** `vas::Map` keeps one byte per slot (a tag made of hash bits) in
its own block and scans it 8 slots at a time; `std`'s `HashMap` does the
same 16 at a time with SIMD. With the same hasher the two are close:
inserts are level, and lookups differ by about 1 ns either way. Without
`FastHasher`, `std` is about twice as slow, so the hasher matters more than
the table.

Fill level matters for keys that are **not** in the map: at 75% full a miss
costs up to 3x as much as at 50% (50k keys), at 90% up to 8x. Hits barely
change up to 90%. Leave a quarter of the slots free if misses are common.

Until 0.26 the map kept a 4-byte tag next to every key and value and
walked slot by slot. At 50k keys that layout took 4.8 ns per insert, 4.7
per hit and 10.5 per miss at 50% full, and 10 / 40 ns at 90% full; it was
faster only on small, half-empty maps (1.7 ns per insert and 1.4 per hit
at 2k keys). Removing is the one operation that got slower at 2k keys
(2.5 to 3.6 ns), because the entries after the removed one are hashed
again.

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

### Results

| Case | Implementation | ns / op |
|---|---|---:|
| push 100k u64 | vas::Vector | 0.8 |
| push 100k u64 | vas::Vector (growing, `Dynamic`) | 1.0 |
| push 100k u64 | std Vec (reserved) | 0.8 |
| push 100k u64 | std Vec (growing) | 0.8 |
| queue 100k push+pop | vas::Deque | 2.2 |
| queue 100k push+pop | std VecDeque | 1.6 |
| heap 50k push then pop | vas::Heap | 18.0 |
| heap 50k push then pop | std BinaryHeap | 17.2 |
| map 2000 keys: insert | vas::Map (50% full) | 2.5 |
| map 2000 keys: insert | vas::Map (growing, `Dynamic`) | 8.1 |
| map 2000 keys: insert | std HashMap + vas FastHasher (growing) | 10.0 |
| map 2000 keys: insert | std HashMap (SipHash) | 5.5 |
| map 2000 keys: insert | std HashMap + vas FastHasher | 2.6 |
| map 2000 keys: hits | vas::Map (50% full) | 1.9 |
| map 2000 keys: misses | vas::Map (50% full) | 1.9 |
| map 2000 keys: hits | vas::Map (75% full) | 2.0 |
| map 2000 keys: misses | vas::Map (75% full) | 2.3 |
| map 2000 keys: hits | vas::Map (90% full) | 2.3 |
| map 2000 keys: misses | vas::Map (90% full) | 4.9 |
| map 2000 keys: hits | std HashMap (SipHash) | 3.8 |
| map 2000 keys: misses | std HashMap (SipHash) | 3.6 |
| map 2000 keys: hits | std HashMap + vas FastHasher | 1.4 |
| map 2000 keys: misses | std HashMap + vas FastHasher | 1.0 |
| map 2000 keys: insert then remove | vas::Map (50% full) | 3.6 |
| map 2000 keys: insert then remove | std HashMap + vas FastHasher | 2.8 |
| map 50000 keys: insert | vas::Map (50% full) | 2.9 |
| map 50000 keys: insert | vas::Map (growing, `Dynamic`) | 16.3 |
| map 50000 keys: insert | std HashMap + vas FastHasher (growing) | 8.1 |
| map 50000 keys: insert | std HashMap (SipHash) | 6.2 |
| map 50000 keys: insert | std HashMap + vas FastHasher | 3.0 |
| map 50000 keys: hits | vas::Map (50% full) | 2.5 |
| map 50000 keys: misses | vas::Map (50% full) | 2.2 |
| map 50000 keys: hits | vas::Map (75% full) | 2.5 |
| map 50000 keys: misses | vas::Map (75% full) | 6.9 |
| map 50000 keys: hits | vas::Map (90% full) | 3.5 |
| map 50000 keys: misses | vas::Map (90% full) | 17.6 |
| map 50000 keys: hits | std HashMap (SipHash) | 4.9 |
| map 50000 keys: misses | std HashMap (SipHash) | 5.5 |
| map 50000 keys: hits | std HashMap + vas FastHasher | 1.7 |
| map 50000 keys: misses | std HashMap + vas FastHasher | 3.3 |
| map 50000 keys: insert then remove | vas::Map (50% full) | 8.4 |
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

## Against other crates

Run with `make bench-crates`. The comparison lives in `bench/`, a separate
crate that is never published and is not part of the `vas` package, so vas
itself keeps zero dependencies. Same harness, same machine, same caveats.

Every case computes a checksum, and vas and the other crate agree on it in
every case that is comparable (the two random generators use different
algorithms, so theirs differ).

### Summary

| Structure | Against | Result |
|---|---|---|
| `Vector` | `arrayvec`, `heapless` | same speed |
| `Deque` | `heapless::Deque` | about 1.6x slower |
| `Heap` | `heapless::BinaryHeap` | level to slightly slower (within run-to-run noise) |
| `Map`, 2k keys | `hashbrown` | inserts level, lookups 1.4x to 2x slower (under 1 ns) |
| `Map`, 50k keys | `hashbrown` | inserts level, hits 1.7x slower, misses 1.6x faster |
| `Map` | `heapless::FnvIndexMap` | level to 1.3x faster at 2k keys, 4x to 8x faster at 50k |
| `Pool` | `slotmap`, `slab` | level with `slab`, slightly faster than `slotmap` |
| `Bitset` | `fixedbitset` | same speed |
| `Rng` | `fastrand` | slightly slower (0.5 vs 0.4 ns per `u64`; 0.9 vs 0.6 ns for a bounded value) |
| `KdTree` nearest | `rstar::RTree` | about 3x faster, and 2.5x faster to build |
| `SpatialGrid` radius query | `rstar::RTree` | about 4.5x faster |
| `PathFinder` | `pathfinding::astar` | about 1.2x faster |

Notes:

- The map here has 131072 slots for 50k keys (38% full), where the std
  comparison above uses 100k slots (50% full).
- `heapless::Deque` is the one clear loss. The likely cause is that vas
  checks bounds on every slot access where `heapless` does not; this has not
  been confirmed by profiling.
- `rstar` is a general R-tree that also handles shapes with a size; a k-d
  tree and a grid are specialised for points, so beating it on point queries
  is expected.
- `pathfinding::astar` allocates its own bookkeeping on every call;
  `PathFinder` reuses the caller's memory.

### Results

| Case | Implementation | ns / op |
|---|---|---:|
| push 100k u64 | vas::Vector | 0.8 |
| push 100k u64 | arrayvec::ArrayVec | 0.8 |
| push 100k u64 | heapless::Vec | 0.8 |
| queue 100k push+pop | vas::Deque | 2.2 |
| queue 100k push+pop | heapless::Deque | 1.4 |
| heap 50k push then pop | vas::Heap | 20.4 |
| heap 50k push then pop | heapless::BinaryHeap | 17.0 |
| map 2000 keys: insert | vas::Map | 2.5 |
| map 2000 keys: hits | vas::Map | 1.9 |
| map 2000 keys: misses | vas::Map | 1.9 |
| map 2000 keys: insert | hashbrown::HashMap (default hasher) | 2.4 |
| map 2000 keys: hits | hashbrown::HashMap (default hasher) | 1.3 |
| map 2000 keys: misses | hashbrown::HashMap (default hasher) | 0.9 |
| map 2000 keys: insert | hashbrown::HashMap + vas FastHasher | 2.5 |
| map 2000 keys: hits | hashbrown::HashMap + vas FastHasher | 1.4 |
| map 2000 keys: misses | hashbrown::HashMap + vas FastHasher | 1.1 |
| map 2000 keys: insert | heapless::FnvIndexMap | 2.6 |
| map 2000 keys: hits | heapless::FnvIndexMap | 2.5 |
| map 2000 keys: misses | heapless::FnvIndexMap | 1.9 |
| map 50000 keys: insert | vas::Map | 2.9 |
| map 50000 keys: hits | vas::Map | 2.9 |
| map 50000 keys: misses | vas::Map | 2.0 |
| map 50000 keys: insert | hashbrown::HashMap (default hasher) | 2.9 |
| map 50000 keys: hits | hashbrown::HashMap (default hasher) | 1.6 |
| map 50000 keys: misses | hashbrown::HashMap (default hasher) | 3.2 |
| map 50000 keys: insert | hashbrown::HashMap + vas FastHasher | 2.7 |
| map 50000 keys: hits | hashbrown::HashMap + vas FastHasher | 1.7 |
| map 50000 keys: misses | hashbrown::HashMap + vas FastHasher | 3.2 |
| map 50000 keys: insert | heapless::FnvIndexMap | 11.4 |
| map 50000 keys: hits | heapless::FnvIndexMap | 15.3 |
| map 50000 keys: misses | heapless::FnvIndexMap | 16.1 |
| pool 50k insert+get+remove | vas::Pool | 0.5 |
| pool 50k insert+get+remove | slotmap::SlotMap | 0.7 |
| pool 50k insert+get+remove | slab::Slab (no stale detection) | 0.5 |
| bitset 100k insert+test | vas::Bitset | 0.4 |
| bitset 100k insert+test | fixedbitset::FixedBitSet | 0.4 |
| rng 1M u64 | vas::Rng | 0.5 |
| rng 1M u64 | fastrand::Rng | 0.4 |
| rng 1M in 0..6 | vas::Rng | 0.9 |
| rng 1M in 0..6 | fastrand::Rng | 0.6 |
| 10k points: build | vas::KdTree | 38.4 |
| 10k points: build | rstar::RTree (bulk load) | 96.0 |
| 10k points: 1k radius queries | vas::SpatialGrid 32x32 | 63.3 |
| 10k points: 1k radius queries | vas::KdTree | 170.5 |
| 10k points: 1k radius queries | rstar::RTree | 296.4 |
| 10k points: 1k nearest | vas::KdTree | 130.3 |
| 10k points: 1k nearest | rstar::RTree | 442.2 |
| A* 128x128 grid, corner to corner | vas::PathFinder | 34996.0 |
| A* 128x128 grid, corner to corner | pathfinding::astar | 41999.0 |
