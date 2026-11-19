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
| `Vector::sort_unstable` (vas quicksort) | `core` `sort_unstable` | same speed |
| `sort::select_nth_unstable` (median, 100k) | `core` `select_nth_unstable` | 1.3x faster |
| `search::binary_search` (100k) | `core` `binary_search` | 1.3x faster |
| `Vector::sort_stable_in` | std `sort_by_key` | 1.5x slower at 1k, 2x to 4.5x at 100k; no allocation |
| `TreeMap` inserts and removes | `BTreeMap` | level to 1.2x slower |
| `TreeMap` lookups | `BTreeMap` | 1.6x slower at 1k, 1.5x faster at 16k |
| `TreeMap` iteration | `BTreeMap` | 3x to 5x slower (2.5 to 4.4 ns per entry) |
| `TreeMap` remove+insert, 16k | `OrderedMap` | 6x faster |
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

**Sort.** vas's quicksort is level with `core`'s. Its first version was
1.35x slower: insertion sort in the short slices mispredicts about once per
element, and a branch-free network with merges from both ends fixed that.
The sort benchmarks take 8 inputs in turn. Sorting the same input again and
again lets the branch predictor learn its comparisons, which made the
branchy version look 1.4x faster than `core` at 1k elements when it was
1.4x slower.

**Spatial structures.** For uniformly spread points and queries about the
size of a cell, the grid is the fastest by a wide margin and the cheapest to
fill. The quadtree and the k-d tree are for when points are unevenly spread
or the nearest neighbour is wanted; this benchmark does not show them at
their best.

**Path search.** On a 128x128 grid with 20% walls, corner to corner, A* with
a Manhattan heuristic takes about 0.04 ms and Dijkstra about 0.5 ms. On the
same map as a `vas::Grid`, A* through `Grid::space` is about 10% slower
than the hand-written `Space` (bounds checks through `offset`, the cost
closure); a whole-map `FlowField` takes 0.7 ms, a field of view of radius
64 from the centre 0.7 µs, and a line of sight 10 ns.

**Sparse set.** With 10,000 entities among ids below 100,000, `SparseSet`
looks a value up in 0.6 ns (an array access, no hash) against 1.4 ns for
`HashMap`, and walks its values 4x faster, as one packed slice.

### Results

| Case | Implementation | ns / op |
|---|---|---:|
| push 100k u64 | vas::Vector | 0.4 |
| push 100k u64 | vas::Vector (growing, `Dynamic`) | 0.5 |
| push 100k u64 | std Vec (reserved) | 0.4 |
| push 100k u64 | std Vec (growing) | 0.4 |
| queue 100k push+pop | vas::Deque | 1.0 |
| queue 100k push+pop | std VecDeque | 1.3 |
| heap 50k push then pop | vas::Heap | 17.8 |
| heap 50k push then pop | std BinaryHeap | 18.0 |
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
| sort 1000 (u32, u32) by key | vas::Vector sort_unstable_by_key | 5.8 |
| sort 1000 (u32, u32) by key | core slice sort_unstable_by_key | 6.0 |
| sort 1000 (u32, u32) by key | vas::Vector sort_stable_by_key | 48.8 |
| sort 1000 (u32, u32) by key | vas::Vector sort_stable_in (n/2 scratch) | 10.6 |
| sort 1000 (u32, u32) by key | vas::Vector sort_stable_in (256 scratch) | 10.9 |
| sort 1000 (u32, u32) by key | std sort_by_key (allocates) | 7.1 |
| sort 100000 (u32, u32) by key | vas::Vector sort_unstable_by_key | 4.5 |
| sort 100000 (u32, u32) by key | core slice sort_unstable_by_key | 4.8 |
| sort 100000 (u32, u32) by key | vas::Vector sort_stable_by_key | 94.0 |
| sort 100000 (u32, u32) by key | vas::Vector sort_stable_in (n/2 scratch) | 22.1 |
| sort 100000 (u32, u32) by key | vas::Vector sort_stable_in (256 scratch) | 25.8 |
| sort 100000 (u32, u32) by key | std sort_by_key (allocates) | 4.8 |
| sort 100000 (u32, u32) by key, all distinct | vas::Vector sort_unstable_by_key | 9.6 |
| sort 100000 (u32, u32) by key, all distinct | core slice sort_unstable_by_key | 9.7 |
| sort 100000 (u32, u32) by key, all distinct | vas::Vector sort_stable_by_key | 116.6 |
| sort 100000 (u32, u32) by key, all distinct | vas::Vector sort_stable_in (n/2 scratch) | 22.1 |
| sort 100000 (u32, u32) by key, all distinct | vas::Vector sort_stable_in (256 scratch) | 26.1 |
| sort 100000 (u32, u32) by key, all distinct | std sort_by_key (allocates) | 11.0 |
| binary search 100k sorted u32 | vas::search::binary_search | 11.8 |
| binary search 100k sorted u32 | core slice binary_search | 15.6 |
| median of 100k u32 | vas::sort::select_nth_unstable | 0.6 |
| median of 100k u32 | core select_nth_unstable | 0.8 |
| ordered 32: build | vas::OrderedMap | 10.6 |
| ordered 32: build | vas::TreeMap | 8.4 |
| ordered 32: build | std BTreeMap | 13.5 |
| ordered 32: lookups | vas::OrderedMap | 2.8 |
| ordered 32: lookups | vas::TreeMap | 2.5 |
| ordered 32: lookups | std BTreeMap | 2.5 |
| ordered 32: remove+insert | vas::OrderedMap | 12.1 |
| ordered 32: remove+insert | vas::TreeMap | 11.6 |
| ordered 32: remove+insert | std BTreeMap | 9.8 |
| ordered 32: iterate | vas::OrderedMap | 0.6 |
| ordered 32: iterate | vas::TreeMap | 2.5 |
| ordered 32: iterate | std BTreeMap | 0.9 |
| ordered 1024: build | vas::OrderedMap | 26.5 |
| ordered 1024: build | vas::TreeMap | 16.8 |
| ordered 1024: build | std BTreeMap | 14.1 |
| ordered 1024: lookups | vas::OrderedMap | 5.7 |
| ordered 1024: lookups | vas::TreeMap | 7.2 |
| ordered 1024: lookups | std BTreeMap | 4.5 |
| ordered 1024: remove+insert | vas::OrderedMap | 38.8 |
| ordered 1024: remove+insert | vas::TreeMap | 42.5 |
| ordered 1024: remove+insert | std BTreeMap | 40.5 |
| ordered 1024: iterate | vas::OrderedMap | 0.1 |
| ordered 1024: iterate | vas::TreeMap | 2.6 |
| ordered 1024: iterate | std BTreeMap | 0.5 |
| ordered 16384: build | vas::OrderedMap | 218.5 |
| ordered 16384: build | vas::TreeMap | 55.2 |
| ordered 16384: build | std BTreeMap | 51.8 |
| ordered 16384: lookups | vas::OrderedMap | 12.3 |
| ordered 16384: lookups | vas::TreeMap | 27.4 |
| ordered 16384: lookups | std BTreeMap | 41.7 |
| ordered 16384: remove+insert | vas::OrderedMap | 420.1 |
| ordered 16384: remove+insert | vas::TreeMap | 65.9 |
| ordered 16384: remove+insert | std BTreeMap | 56.7 |
| ordered 16384: iterate | vas::OrderedMap | 0.1 |
| ordered 16384: iterate | vas::TreeMap | 4.4 |
| ordered 16384: iterate | std BTreeMap | 0.8 |
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
| 128x128 grid, corner to corner | vas::PathFinder A* on Grid::space | 39785.0 |
| 128x128 grid, flow field to a corner | vas::FlowField build | 669769.0 |
| 128x128 grid, field of view radius 64 | vas::Grid::field_of_view | 721.0 |
| 128x128 grid, line of sight | vas::Grid::line_of_sight | 10.5 |
| components 10k of 100k ids: insert | vas::SparseSet | 1.8 |
| components 10k of 100k ids: insert | vas::Map (50% full) | 2.9 |
| components 10k of 100k ids: insert | std HashMap + vas FastHasher | 2.7 |
| components 10k of 100k ids: get | vas::SparseSet | 0.6 |
| components 10k of 100k ids: get | vas::Map (50% full) | 2.4 |
| components 10k of 100k ids: get | std HashMap + vas FastHasher | 1.4 |
| components 10k of 100k ids: iterate | vas::SparseSet | 0.1 |
| components 10k of 100k ids: iterate | vas::Map (50% full) | 0.7 |
| components 10k of 100k ids: iterate | std HashMap + vas FastHasher | 0.4 |
| lru 1024 of 2048 keys: get, insert on a miss | vas::LruCache | 20.3 |

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
| `Deque` | `heapless::Deque` | level to slightly faster |
| `Heap` | `heapless::BinaryHeap` | about 1.15x slower |
| `Map`, 2k keys | `hashbrown` | inserts level, lookups 1.4x to 2x slower (under 1 ns) |
| `Map`, 50k keys | `hashbrown` | inserts level, hits 1.7x slower, misses 1.6x faster |
| `Map` | `heapless::FnvIndexMap` | level to 1.3x faster at 2k keys, 4x to 8x faster at 50k |
| `Pool` | `slotmap`, `slab` | level with `slab`, slightly faster than `slotmap` |
| `Bitset` | `fixedbitset` | same speed |
| `Rng` | `fastrand` | slightly slower (0.5 vs 0.4 ns per `u64`; 0.9 vs 0.6 ns for a bounded value) |
| `KdTree` nearest | `rstar::RTree` | about 3x faster, and 2.5x faster to build |
| `SpatialGrid` radius query | `rstar::RTree` | about 4.5x faster |
| `PathFinder` | `pathfinding::astar` | about 1.2x faster |
| `LruCache` | `lru::LruCache` with the same hasher | 1.3x slower (20 against 15 ns per get or insert); no allocation |

Notes:

- The map here has 131072 slots for 50k keys (38% full), where the std
  comparison above uses 100k slots (50% full).
- Earlier versions of this page had `Deque` 1.6x behind `heapless::Deque`
  (2.2 against 1.4 ns). That was a measuring error: the vector, deque and
  heap cases run first, while the CPU is still speeding up, and read up to
  2x too slow. Both benchmarks now spin for a second before the first
  case.
- `rstar` is a general R-tree that also handles shapes with a size; a k-d
  tree and a grid are specialised for points, so beating it on point queries
  is expected.
- `pathfinding::astar` allocates its own bookkeeping on every call;
  `PathFinder` reuses the caller's memory.

### Results

| Case | Implementation | ns / op |
|---|---|---:|
| push 100k u64 | vas::Vector | 0.4 |
| push 100k u64 | arrayvec::ArrayVec | 0.4 |
| push 100k u64 | heapless::Vec | 0.4 |
| queue 100k push+pop | vas::Deque | 1.0 |
| queue 100k push+pop | heapless::Deque | 1.1 |
| heap 50k push then pop | vas::Heap | 17.7 |
| heap 50k push then pop | heapless::BinaryHeap | 15.1 |
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
| pool 50k insert+get+remove | slotmap::SlotMap | 0.6 |
| pool 50k insert+get+remove | slab::Slab (no stale detection) | 0.5 |
| bitset 100k insert+test | vas::Bitset | 0.3 |
| bitset 100k insert+test | fixedbitset::FixedBitSet | 0.3 |
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
| lru 1024 of 2048 keys: get, insert on a miss | vas::LruCache | 20.3 |
| lru 1024 of 2048 keys: get, insert on a miss | lru::LruCache + vas FastHasher | 15.3 |
