//! Benchmarks of vas against other crates. Run with `make bench-crates`.
//!
//! Each case is run several times and the fastest run is reported, in
//! nanoseconds per operation. See `docs/benchmarks.md` for the results.

use std::hint::black_box;
use std::mem::MaybeUninit;
use std::time::Instant;

use vas::hash::FastState;
use vas::path::Space;
use vas::{
    Aabb, Bitset, Deque, Heap, KdTree, LruCache, Map, PathFinder, Pool, Rng, SpatialGrid, Vector,
};

const RUNS: usize = 15;

/// Heap memory for a vas container, obtained by the benchmark.
fn block<T>(capacity: usize) -> Vec<MaybeUninit<T>> {
    let mut memory = Vec::with_capacity(capacity);
    memory.resize_with(capacity, MaybeUninit::uninit);
    memory
}

/// Runs `work` several times and prints the fastest run per operation.
///
/// `cargo run --release -- queue` only runs the cases whose name contains
/// `queue`.
fn bench(group: &str, name: &str, operations: usize, mut work: impl FnMut() -> u64) {
    let filter = std::env::args().skip(1).find(|arg| !arg.starts_with('-'));
    if filter.is_some_and(|filter| !group.contains(&filter)) {
        return;
    }
    let mut best = f64::INFINITY;
    let mut check = 0;
    for _ in 0..RUNS {
        let start = Instant::now();
        check = black_box(work());
        best = best.min(start.elapsed().as_nanos() as f64);
    }
    let per_operation = best / operations.max(1) as f64;
    println!("| {group} | {name} | {per_operation:.1} | {check} |");
}

fn keys(count: usize, seed: u64) -> Vec<u32> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| rng.u32()).collect()
}

fn vector() {
    const N: usize = 100_000;
    let group = "push 100k u64";
    let mut memory = block::<u64>(N);
    bench(group, "vas::Vector", N, || {
        let mut v = Vector::new(&mut memory[..]);
        for i in 0..N as u64 {
            let _ = v.push(black_box(i));
        }
        v.len() as u64
    });
    let mut v = Box::new(arrayvec::ArrayVec::<u64, N>::new());
    bench(group, "arrayvec::ArrayVec", N, || {
        v.clear();
        for i in 0..N as u64 {
            let _ = v.try_push(black_box(i));
        }
        v.len() as u64
    });
    let mut v = Box::new(heapless::Vec::<u64, N>::new());
    bench(group, "heapless::Vec", N, || {
        v.clear();
        for i in 0..N as u64 {
            let _ = v.push(black_box(i));
        }
        v.len() as u64
    });
}

fn deque() {
    const N: usize = 100_000;
    let group = "queue 100k push+pop";
    let mut memory = block::<u64>(1024);
    bench(group, "vas::Deque", N, || {
        let mut d = Deque::new(&mut memory[..]);
        let mut sum = 0;
        for i in 0..N as u64 {
            let _ = d.push_back(black_box(i));
            if i % 2 == 1 {
                sum += d.pop_front().unwrap_or(0) + d.pop_front().unwrap_or(0);
            }
        }
        sum
    });
    let mut d = Box::new(heapless::Deque::<u64, 1024>::new());
    bench(group, "heapless::Deque", N, || {
        d.clear();
        let mut sum = 0;
        for i in 0..N as u64 {
            let _ = d.push_back(black_box(i));
            if i % 2 == 1 {
                sum += d.pop_front().unwrap_or(0) + d.pop_front().unwrap_or(0);
            }
        }
        sum
    });
}

fn heap() {
    const N: usize = 50_000;
    let group = "heap 50k push then pop";
    let input = keys(N, 1);
    let mut memory = block::<u32>(N);
    bench(group, "vas::Heap", 2 * N, || {
        let mut heap = Heap::new(&mut memory[..]);
        for key in &input {
            let _ = heap.push(*key);
        }
        let mut sum = 0u64;
        while let Some(top) = heap.pop() {
            sum = sum.wrapping_add(top as u64);
        }
        sum
    });
    let mut heap = Box::new(heapless::BinaryHeap::<u32, heapless::binary_heap::Max, N>::new());
    bench(group, "heapless::BinaryHeap", 2 * N, || {
        heap.clear();
        for key in &input {
            let _ = heap.push(*key);
        }
        let mut sum = 0u64;
        while let Some(top) = heap.pop() {
            sum = sum.wrapping_add(top as u64);
        }
        sum
    });
}

fn map<const N: usize, const SLOTS: usize>() {
    let input = keys(N, 2);
    let misses = keys(N, 3);
    let insert = format!("map {N} keys: insert");
    let hits = format!("map {N} keys: hits");
    let miss = format!("map {N} keys: misses");

    let mut memory = block::<vas::map::Slot<u32, u32>>(SLOTS);
    let mut tags = block::<u8>(SLOTS);
    bench(&insert, "vas::Map", N, || {
        let mut map = Map::new(&mut memory[..], &mut tags[..]);
        for key in &input {
            let _ = map.insert(*key, *key);
        }
        map.len() as u64
    });
    let mut map = Map::new(&mut memory[..], &mut tags[..]);
    for key in &input {
        let _ = map.insert(*key, *key);
    }
    bench(&hits, "vas::Map", N, || {
        input
            .iter()
            .filter_map(|key| map.get(key))
            .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
    });
    bench(&miss, "vas::Map", N, || {
        misses.iter().filter(|key| map.contains_key(*key)).count() as u64
    });

    // hashbrown: the table behind std's HashMap, with its own default hasher
    // and with the vas hasher.
    bench(&insert, "hashbrown::HashMap (default hasher)", N, || {
        let mut map = hashbrown::HashMap::with_capacity(N);
        for key in &input {
            map.insert(*key, *key);
        }
        map.len() as u64
    });
    let mut brown = hashbrown::HashMap::with_capacity(N);
    for key in &input {
        brown.insert(*key, *key);
    }
    bench(&hits, "hashbrown::HashMap (default hasher)", N, || {
        input
            .iter()
            .filter_map(|key| brown.get(key))
            .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
    });
    bench(&miss, "hashbrown::HashMap (default hasher)", N, || {
        misses.iter().filter(|key| brown.contains_key(*key)).count() as u64
    });

    bench(&insert, "hashbrown::HashMap + vas FastHasher", N, || {
        let mut map = hashbrown::HashMap::with_capacity_and_hasher(N, FastState::default());
        for key in &input {
            map.insert(*key, *key);
        }
        map.len() as u64
    });
    let mut brown = hashbrown::HashMap::with_capacity_and_hasher(N, FastState::default());
    for key in &input {
        brown.insert(*key, *key);
    }
    bench(&hits, "hashbrown::HashMap + vas FastHasher", N, || {
        input
            .iter()
            .filter_map(|key| brown.get(key))
            .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
    });
    bench(&miss, "hashbrown::HashMap + vas FastHasher", N, || {
        misses.iter().filter(|key| brown.contains_key(*key)).count() as u64
    });

    // heapless: fixed capacity, same number of slots as the vas map.
    let mut fixed = Box::new(heapless::index_map::FnvIndexMap::<u32, u32, SLOTS>::new());
    bench(&insert, "heapless::FnvIndexMap", N, || {
        fixed.clear();
        for key in &input {
            let _ = fixed.insert(*key, *key);
        }
        fixed.len() as u64
    });
    bench(&hits, "heapless::FnvIndexMap", N, || {
        input
            .iter()
            .filter_map(|key| fixed.get(key))
            .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
    });
    bench(&miss, "heapless::FnvIndexMap", N, || {
        misses.iter().filter(|key| fixed.contains_key(*key)).count() as u64
    });
}

fn pool() {
    const N: usize = 50_000;
    let group = "pool 50k insert+get+remove";
    let mut memory = block::<vas::pool::Slot<u64>>(N);
    let mut handles = Vec::with_capacity(N);
    bench(group, "vas::Pool", 3 * N, || {
        let mut pool = Pool::new(&mut memory[..]);
        handles.clear();
        for i in 0..N as u64 {
            if let Ok(handle) = pool.insert(i) {
                handles.push(handle);
            }
        }
        let mut sum = 0;
        for handle in &handles {
            sum += pool.get(*handle).copied().unwrap_or(0);
        }
        for handle in &handles {
            sum += pool.remove(*handle).unwrap_or(0);
        }
        sum
    });
    let mut keys = Vec::with_capacity(N);
    bench(group, "slotmap::SlotMap", 3 * N, || {
        let mut pool = slotmap::SlotMap::with_capacity(N);
        keys.clear();
        for i in 0..N as u64 {
            keys.push(pool.insert(i));
        }
        let mut sum = 0;
        for key in &keys {
            sum += pool.get(*key).copied().unwrap_or(0);
        }
        for key in &keys {
            sum += pool.remove(*key).unwrap_or(0);
        }
        sum
    });
    let mut indices = Vec::with_capacity(N);
    bench(group, "slab::Slab (no stale detection)", 3 * N, || {
        let mut pool = slab::Slab::with_capacity(N);
        indices.clear();
        for i in 0..N as u64 {
            indices.push(pool.insert(i));
        }
        let mut sum = 0;
        for index in &indices {
            sum += pool.get(*index).copied().unwrap_or(0);
        }
        for index in &indices {
            sum += pool.try_remove(*index).unwrap_or(0);
        }
        sum
    });
}

fn bitset() {
    const BITS: usize = 1 << 16;
    let group = "bitset 100k insert+test";
    let input = keys(100_000, 5);
    let mut memory = block::<u64>(BITS / 64);
    bench(group, "vas::Bitset", 200_000, || {
        let mut bits = Bitset::new(&mut memory[..]);
        for key in &input {
            let _ = bits.insert(*key as usize % BITS);
        }
        input
            .iter()
            .filter(|key| bits.contains(**key as usize % BITS / 2))
            .count() as u64
    });
    bench(group, "fixedbitset::FixedBitSet", 200_000, || {
        let mut bits = fixedbitset::FixedBitSet::with_capacity(BITS);
        for key in &input {
            bits.insert(*key as usize % BITS);
        }
        input
            .iter()
            .filter(|key| bits.contains(**key as usize % BITS / 2))
            .count() as u64
    });
}

fn random() {
    const N: usize = 1_000_000;
    let mut rng = Rng::new(6);
    let mut fast = fastrand::Rng::with_seed(6);
    bench("rng 1M u64", "vas::Rng", N, || {
        let mut sum = 0u64;
        for _ in 0..N {
            sum = sum.wrapping_add(rng.u64());
        }
        sum
    });
    bench("rng 1M u64", "fastrand::Rng", N, || {
        let mut sum = 0u64;
        for _ in 0..N {
            sum = sum.wrapping_add(fast.u64(..));
        }
        sum
    });
    bench("rng 1M in 0..6", "vas::Rng", N, || {
        let mut sum = 0u64;
        for _ in 0..N {
            sum += rng.u32_below(6) as u64;
        }
        sum
    });
    bench("rng 1M in 0..6", "fastrand::Rng", N, || {
        let mut sum = 0u64;
        for _ in 0..N {
            sum += fast.u32(0..6) as u64;
        }
        sum
    });
}

fn spatial() {
    const N: usize = 10_000;
    const QUERIES: usize = 1_000;
    const RADIUS: f32 = 15.0;
    let mut rng = Rng::new(7);
    let points: Vec<[f32; 2]> = (0..N)
        .map(|_| [rng.f32_range(0.0, 1000.0), rng.f32_range(0.0, 1000.0)])
        .collect();
    let centers: Vec<[f32; 2]> = (0..QUERIES)
        .map(|_| [rng.f32_range(0.0, 1000.0), rng.f32_range(0.0, 1000.0)])
        .collect();
    let world = Aabb::new([0.0, 0.0], [1000.0, 1000.0]);

    let group = "10k points: build";
    let mut items: Vec<([f32; 2], u32)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (*p, i as u32))
        .collect();
    let mut scratch = items.clone();
    bench(group, "vas::KdTree", N, || {
        scratch.copy_from_slice(&items);
        KdTree::build(&mut scratch).len() as u64
    });
    bench(group, "rstar::RTree (bulk load)", N, || {
        rstar::RTree::bulk_load(points.clone()).size() as u64
    });

    let kd = KdTree::build(&mut items);
    let rtree = rstar::RTree::bulk_load(points.clone());
    let mut cells = block::<u32>(32 * 32);
    let mut entries = block::<vas::spatial_grid::Slot<u32, 2>>(N);
    let mut grid = SpatialGrid::new(world, [32, 32], &mut cells, &mut entries).ok();
    if let Some(grid) = grid.as_mut() {
        for (index, point) in points.iter().enumerate() {
            let _ = grid.insert(*point, index as u32);
        }
    }

    let group = "10k points: 1k radius queries";
    if let Some(grid) = grid.as_ref() {
        bench(group, "vas::SpatialGrid 32x32", QUERIES, || {
            let mut found = 0;
            for center in &centers {
                grid.query_radius(*center, RADIUS, |_, _| found += 1);
            }
            found
        });
    }
    bench(group, "vas::KdTree", QUERIES, || {
        let mut found = 0;
        for center in &centers {
            kd.query_radius(center, RADIUS, |_, _| found += 1);
        }
        found
    });
    bench(group, "rstar::RTree", QUERIES, || {
        let mut found = 0;
        for center in &centers {
            found += rtree
                .locate_within_distance(*center, RADIUS * RADIUS)
                .count() as u64;
        }
        found
    });

    let group = "10k points: 1k nearest";
    bench(group, "vas::KdTree", QUERIES, || {
        let mut sum = 0.0f32;
        for center in &centers {
            sum += kd
                .nearest(center)
                .map_or(0.0, |found| found.distance_squared);
        }
        sum as u64
    });
    bench(group, "rstar::RTree", QUERIES, || {
        let mut sum = 0.0f32;
        for center in &centers {
            if let Some(point) = rtree.nearest_neighbor(*center) {
                let (dx, dy) = (point[0] - center[0], point[1] - center[1]);
                sum += dx * dx + dy * dy;
            }
        }
        sum as u64
    });
}

struct Grid {
    width: u32,
    walls: Vec<bool>,
}

impl Grid {
    fn open(&self, node: u32) -> bool {
        !self.walls.get(node as usize).copied().unwrap_or(true)
    }

    fn adjacent(&self, node: u32) -> [Option<u32>; 4] {
        let (x, y) = (node % self.width, node / self.width);
        [
            (x > 0 && self.open(node - 1)).then(|| node - 1),
            (x + 1 < self.width && self.open(node + 1)).then(|| node + 1),
            (y > 0 && self.open(node - self.width)).then(|| node - self.width),
            (y + 1 < self.width && self.open(node + self.width)).then(|| node + self.width),
        ]
    }

    fn manhattan(&self, from: u32, goal: u32) -> u32 {
        (from % self.width).abs_diff(goal % self.width)
            + (from / self.width).abs_diff(goal / self.width)
    }
}

impl Space for Grid {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        for next in self.adjacent(node).into_iter().flatten() {
            visit(next, 1.0);
        }
    }

    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        self.manhattan(from, goal) as f32
    }
}

fn path() {
    const WIDTH: u32 = 128;
    let group = "A* 128x128 grid, corner to corner";
    let cells = (WIDTH * WIDTH) as usize;
    let mut rng = Rng::new(8);
    let mut grid = Grid {
        width: WIDTH,
        walls: (0..cells).map(|_| rng.chance(0.2)).collect(),
    };
    for corner in [0, cells - 1] {
        grid.walls[corner] = false;
    }
    let goal = cells as u32 - 1;
    let mut nodes = block::<vas::path::Node>(cells);
    let mut open = block::<vas::path::Open>(4 * cells);
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    bench(group, "vas::PathFinder", 1, || {
        finder.find(&grid, 0, goal).map_or(0, |cost| cost as u64)
    });
    bench(group, "pathfinding::astar", 1, || {
        let found = pathfinding::prelude::astar(
            &0u32,
            |node| {
                grid.adjacent(*node)
                    .into_iter()
                    .flatten()
                    .map(|next| (next, 1u32))
            },
            |node| grid.manhattan(*node, goal),
            |node| *node == goal,
        );
        found.map_or(0, |(_, cost)| cost as u64)
    });
}

/// A cache of 1024 entries in front of 2048 keys: about half the lookups
/// hit, every miss inserts (evicting the least recently used).
fn lru() {
    const N: usize = 100_000;
    let mut rng = Rng::new(13);
    let keys: Vec<u32> = (0..N).map(|_| rng.u32_below(2048)).collect();
    let group = "lru 1024 of 2048 keys: get, insert on a miss";
    let mut entries = block::<vas::lru::Entry<u32, u64>>(1024);
    let mut table = block::<u32>(2048);
    if let Ok(mut cache) = LruCache::new(&mut entries[..], &mut table[..], 1024) {
        bench(group, "vas::LruCache", N, || {
            let mut hits = 0;
            for key in &keys {
                if cache.get(key).is_some() {
                    hits += 1;
                } else {
                    cache.insert(*key, u64::from(*key));
                }
            }
            hits
        });
    }
    let capacity = std::num::NonZeroUsize::new(1024).unwrap_or(std::num::NonZeroUsize::MIN);
    let mut cache = lru::LruCache::with_hasher(capacity, FastState::default());
    bench(group, "lru::LruCache + vas FastHasher", N, || {
        let mut hits = 0;
        for key in &keys {
            if cache.get(key).is_some() {
                hits += 1;
            } else {
                cache.put(*key, u64::from(*key));
            }
        }
        hits
    });
}

/// Moves `0..total` from `threads` producers to `threads` consumers. Each
/// producer then pushes one `u64::MAX`, and each consumer stops at the
/// first one it takes: with a FIFO queue all values come before the last.
fn exchange(
    threads: usize,
    total: usize,
    push: impl Fn(u64) -> bool + Sync,
    pop: impl Fn() -> Option<u64> + Sync,
) -> u64 {
    let per = (total / threads) as u64;
    let sum = std::sync::atomic::AtomicU64::new(0);
    std::thread::scope(|scope| {
        for producer in 0..threads as u64 {
            let push = &push;
            scope.spawn(move || {
                for value in (producer * per..(producer + 1) * per).chain([u64::MAX]) {
                    while !push(value) {
                        std::hint::spin_loop();
                    }
                }
            });
        }
        for _ in 0..threads {
            scope.spawn(|| {
                let mut local = 0u64;
                loop {
                    match pop() {
                        Some(u64::MAX) => break,
                        Some(value) => local = local.wrapping_add(value),
                        None => std::hint::spin_loop(),
                    }
                }
                sum.fetch_add(local, std::sync::atomic::Ordering::Relaxed);
            });
        }
    });
    sum.into_inner()
}

/// The bounded MPMC queue against `crossbeam_queue::ArrayQueue`.
fn queue() {
    use vas::sync::{Queue, Slot};
    const CAPACITY: usize = 1024;
    const N: usize = 1 << 20;
    let mut slots = block::<Slot<u64>>(CAPACITY);
    let queue = Queue::new(&mut slots[..]);
    let array = crossbeam_queue::ArrayQueue::new(CAPACITY);

    let group = "queue, one thread: push then pop";
    bench(group, "vas::sync::Queue", N, || {
        (0..N as u64)
            .map(|i| {
                let _ = queue.push(i);
                queue.pop().unwrap_or(0)
            })
            .sum()
    });
    bench(group, "crossbeam ArrayQueue", N, || {
        (0..N as u64)
            .map(|i| {
                let _ = array.push(i);
                array.pop().unwrap_or(0)
            })
            .sum()
    });

    const MOVED: usize = 1 << 18;
    for threads in [1, 2, 4, 8] {
        let group = format!("queue, {threads} producers + {threads} consumers, 1024 slots");
        bench(&group, "vas::sync::Queue", MOVED, || {
            exchange(threads, MOVED, |v| queue.push(v).is_ok(), || queue.pop())
        });
        bench(&group, "crossbeam ArrayQueue", MOVED, || {
            exchange(threads, MOVED, |v| array.push(v).is_ok(), || array.pop())
        });
    }
}

fn run() {
    println!("| Case | Implementation | ns / op | check |");
    println!("|---|---|---:|---:|");
    // Let the CPU reach full speed first: the first cases otherwise read
    // up to 2x too slow.
    let start = Instant::now();
    let mut spin = 0u64;
    while start.elapsed().as_millis() < 1000 {
        spin = black_box(spin.wrapping_mul(31).wrapping_add(7));
    }
    vector();
    deque();
    heap();
    map::<2_000, 4_096>();
    map::<50_000, 131_072>();
    pool();
    bitset();
    random();
    spatial();
    path();
    lru();
    queue();
}

fn main() {
    // Some of the fixed-capacity crates keep their storage inline, which is
    // large here, so run on a thread with a big stack.
    let worker = std::thread::Builder::new().stack_size(256 << 20).spawn(run);
    match worker.map(|thread| thread.join()) {
        Ok(Ok(())) => {}
        _ => eprintln!("benchmark thread failed"),
    }
}
