//! Benchmarks against the standard library. Run with `make bench`.
//!
//! No benchmarking crate is used (vas has no dependencies), so this is a
//! small harness: each case is run several times and the fastest run is
//! reported, in nanoseconds per operation.

use std::collections::{BTreeMap, BinaryHeap, HashMap, VecDeque};
use std::hash::BuildHasher;
use std::hint::black_box;
use std::mem::MaybeUninit;
use std::time::Instant;

use vas::hash::FastState;
use vas::path::Space;
use vas::{
    Aabb, Bitset, Deque, Heap, KdTree, Map, OrderedMap, PathFinder, Pool, Quadtree, Rng,
    SpatialGrid, Vector,
};

const RUNS: usize = 15;

/// Heap memory for a container, obtained from the global allocator by the
/// benchmark (the library itself never allocates).
fn block<T>(capacity: usize) -> Vec<MaybeUninit<T>> {
    let mut memory = Vec::with_capacity(capacity);
    memory.resize_with(capacity, MaybeUninit::uninit);
    memory
}

/// Runs `work` several times and prints the fastest run per operation.
fn bench(group: &str, name: &str, operations: usize, mut work: impl FnMut() -> u64) {
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
    let mut memory = block::<u64>(N);
    bench("push 100k u64", "vas::Vector", N, || {
        let mut v = Vector::new(&mut memory[..]);
        for i in 0..N as u64 {
            let _ = v.push(black_box(i));
        }
        v.len() as u64
    });
    bench("push 100k u64", "std Vec (reserved)", N, || {
        let mut v = Vec::with_capacity(N);
        for i in 0..N as u64 {
            v.push(black_box(i));
        }
        v.len() as u64
    });
    bench("push 100k u64", "std Vec (growing)", N, || {
        let mut v = Vec::new();
        for i in 0..N as u64 {
            v.push(black_box(i));
        }
        v.len() as u64
    });
}

fn deque() {
    const N: usize = 100_000;
    let mut memory = block::<u64>(1024);
    bench("queue 100k push+pop", "vas::Deque", N, || {
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
    bench("queue 100k push+pop", "std VecDeque", N, || {
        let mut d = VecDeque::with_capacity(1024);
        let mut sum = 0;
        for i in 0..N as u64 {
            d.push_back(black_box(i));
            if i % 2 == 1 {
                sum += d.pop_front().unwrap_or(0) + d.pop_front().unwrap_or(0);
            }
        }
        sum
    });
}

fn heap() {
    const N: usize = 50_000;
    let input = keys(N, 1);
    let mut memory = block::<u32>(N);
    bench("heap 50k push then pop", "vas::Heap", 2 * N, || {
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
    bench("heap 50k push then pop", "std BinaryHeap", 2 * N, || {
        let mut heap = BinaryHeap::with_capacity(N);
        for key in &input {
            heap.push(*key);
        }
        let mut sum = 0u64;
        while let Some(top) = heap.pop() {
            sum = sum.wrapping_add(top as u64);
        }
        sum
    });
}

fn map<const N: usize>() {
    let input = keys(N, 2);
    let misses = keys(N, 3);

    // Half full, as recommended.
    let mut memory = block::<vas::map::Slot<u32, u32>>(2 * N);
    bench(
        &format!("map {N} keys: insert"),
        "vas::Map (50% full)",
        N,
        || {
            let mut map = Map::new(&mut memory[..]);
            for key in &input {
                let _ = map.insert(*key, *key);
            }
            map.len() as u64
        },
    );
    bench(
        &format!("map {N} keys: insert"),
        "std HashMap (SipHash)",
        N,
        || {
            let mut map = HashMap::with_capacity(N);
            for key in &input {
                map.insert(*key, *key);
            }
            map.len() as u64
        },
    );
    bench(
        &format!("map {N} keys: insert"),
        "std HashMap + vas FastHasher",
        N,
        || {
            let mut map = HashMap::with_capacity_and_hasher(N, FastState::default());
            for key in &input {
                map.insert(*key, *key);
            }
            map.len() as u64
        },
    );

    let lookups = |name: &str, fill: usize| {
        let mut memory = block::<vas::map::Slot<u32, u32>>(fill);
        let mut map = Map::new(&mut memory[..]);
        for key in &input {
            let _ = map.insert(*key, *key);
        }
        bench(&format!("map {N} keys: hits"), name, N, || {
            input
                .iter()
                .filter_map(|key| map.get(key))
                .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
        });
        bench(&format!("map {N} keys: misses"), name, N, || {
            misses.iter().filter(|key| map.contains_key(*key)).count() as u64
        });
    };
    lookups("vas::Map (50% full)", 2 * N);
    lookups("vas::Map (90% full)", N * 10 / 9);

    fn std_lookups<S: BuildHasher>(
        name: &str,
        mut map: HashMap<u32, u32, S>,
        input: &[u32],
        misses: &[u32],
    ) {
        for key in input {
            map.insert(*key, *key);
        }
        bench(
            &format!("map {} keys: hits", input.len()),
            name,
            input.len(),
            || {
                input
                    .iter()
                    .filter_map(|key| map.get(key))
                    .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
            },
        );
        bench(
            &format!("map {} keys: misses", input.len()),
            name,
            misses.len(),
            || misses.iter().filter(|key| map.contains_key(*key)).count() as u64,
        );
    }
    std_lookups(
        "std HashMap (SipHash)",
        HashMap::with_capacity(N),
        &input,
        &misses,
    );
    std_lookups(
        "std HashMap + vas FastHasher",
        HashMap::with_capacity_and_hasher(N, FastState::default()),
        &input,
        &misses,
    );

    let mut memory = block::<vas::map::Slot<u32, u32>>(2 * N);
    bench(
        &format!("map {N} keys: insert then remove"),
        "vas::Map (50% full)",
        2 * N,
        || {
            let mut map = Map::new(&mut memory[..]);
            for key in &input {
                let _ = map.insert(*key, *key);
            }
            input.iter().filter_map(|key| map.remove(key)).count() as u64
        },
    );
    bench(
        &format!("map {N} keys: insert then remove"),
        "std HashMap + vas FastHasher",
        2 * N,
        || {
            let mut map = HashMap::with_capacity_and_hasher(N, FastState::default());
            for key in &input {
                map.insert(*key, *key);
            }
            input.iter().filter_map(|key| map.remove(key)).count() as u64
        },
    );
}

fn ordered_map() {
    for size in [32usize, 1024, 16_384] {
        let input = keys(size, 4);
        let group = format!("ordered {size}: build");
        let mut memory = block::<(u32, u32)>(size);
        bench(&group, "vas::OrderedMap", size, || {
            let mut map = OrderedMap::new(&mut memory[..]);
            for key in &input {
                let _ = map.insert(*key, *key);
            }
            map.len() as u64
        });
        bench(&group, "std BTreeMap", size, || {
            let mut map = BTreeMap::new();
            for key in &input {
                map.insert(*key, *key);
            }
            map.len() as u64
        });

        let group = format!("ordered {size}: lookups");
        let mut map = OrderedMap::new(&mut memory[..]);
        let mut std_map = BTreeMap::new();
        for key in &input {
            let _ = map.insert(*key, *key);
            std_map.insert(*key, *key);
        }
        bench(&group, "vas::OrderedMap", size, || {
            input
                .iter()
                .filter_map(|key| map.get(key))
                .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
        });
        bench(&group, "std BTreeMap", size, || {
            input
                .iter()
                .filter_map(|key| std_map.get(key))
                .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
        });
    }
}

fn pool() {
    const N: usize = 50_000;
    let mut memory = block::<vas::pool::Slot<u64>>(N);
    let mut handles = Vec::with_capacity(N);
    bench("pool 50k insert+get+remove", "vas::Pool", 3 * N, || {
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
    bench(
        "pool 50k insert+get+remove",
        "std Vec<Option<Box<u64>>>",
        3 * N,
        || {
            let mut slots: Vec<Option<Box<u64>>> = Vec::with_capacity(N);
            for i in 0..N as u64 {
                slots.push(Some(Box::new(i)));
            }
            let mut sum = 0;
            for slot in &slots {
                sum += slot.as_deref().copied().unwrap_or(0);
            }
            for slot in &mut slots {
                sum += slot.take().map_or(0, |value| *value);
            }
            sum
        },
    );
}

fn bitset() {
    const BITS: usize = 1 << 16;
    let input = keys(100_000, 5);
    let mut memory = block::<u64>(BITS / 64);
    bench("bitset 100k insert+test", "vas::Bitset", 200_000, || {
        let mut bits = Bitset::new(&mut memory[..]);
        for key in &input {
            let _ = bits.insert(*key as usize % BITS);
        }
        input
            .iter()
            .filter(|key| bits.contains(**key as usize % BITS / 2))
            .count() as u64
    });
    bench("bitset 100k insert+test", "std Vec<bool>", 200_000, || {
        let mut bits = vec![false; BITS];
        for key in &input {
            if let Some(bit) = bits.get_mut(*key as usize % BITS) {
                *bit = true;
            }
        }
        input
            .iter()
            .filter(|key| {
                bits.get(**key as usize % BITS / 2)
                    .copied()
                    .unwrap_or(false)
            })
            .count() as u64
    });
}

fn random() {
    const N: usize = 1_000_000;
    let mut rng = Rng::new(6);
    bench("rng 1M values", "vas::Rng u64", N, || {
        let mut sum = 0u64;
        for _ in 0..N {
            sum = sum.wrapping_add(rng.u64());
        }
        sum
    });
    bench("rng 1M values", "vas::Rng u32_below(6)", N, || {
        let mut sum = 0u64;
        for _ in 0..N {
            sum += rng.u32_below(6) as u64;
        }
        sum
    });
    bench("rng 1M values", "vas::Rng f32", N, || {
        let mut sum = 0.0f32;
        for _ in 0..N {
            sum += rng.f32();
        }
        sum as u64
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
    let group = "10k points: 1k radius queries";

    bench(group, "brute force", QUERIES, || {
        let mut found = 0;
        for center in &centers {
            for point in &points {
                let (dx, dy) = (point[0] - center[0], point[1] - center[1]);
                found += u64::from(dx * dx + dy * dy <= RADIUS * RADIUS);
            }
        }
        found
    });

    let mut cells = block::<u32>(32 * 32);
    let mut entries = block::<vas::spatial_grid::Slot<u32, 2>>(N);
    if let Ok(mut grid) = SpatialGrid::new(world, [32, 32], &mut cells[..], &mut entries[..]) {
        for (index, point) in points.iter().enumerate() {
            let _ = grid.insert(*point, index as u32);
        }
        bench(group, "vas::SpatialGrid 32x32", QUERIES, || {
            let mut found = 0;
            for center in &centers {
                grid.query_radius(*center, RADIUS, |_, _| found += 1);
            }
            found
        });
    }

    let mut nodes = block::<vas::orthtree::Node>(1 + 4 * 4096);
    let mut entries = block::<vas::orthtree::Slot<u32, 2>>(N);
    if let Ok(mut tree) = Quadtree::new(world, &mut nodes[..], &mut entries[..]) {
        for (index, point) in points.iter().enumerate() {
            let _ = tree.insert(*point, index as u32);
        }
        bench(group, "vas::Quadtree", QUERIES, || {
            let mut found = 0;
            for center in &centers {
                tree.query_radius(*center, RADIUS, |_, _| found += 1);
            }
            found
        });
    }

    let mut items: Vec<([f32; 2], u32)> = points
        .iter()
        .enumerate()
        .map(|(i, p)| (*p, i as u32))
        .collect();
    let tree = KdTree::build(&mut items);
    bench(group, "vas::KdTree", QUERIES, || {
        let mut found = 0;
        for center in &centers {
            tree.query_radius(center, RADIUS, |_, _| found += 1);
        }
        found
    });

    let group = "10k points: 1k nearest";
    bench(group, "brute force", QUERIES, || {
        let mut sum = 0.0f32;
        for center in &centers {
            let mut best = f32::INFINITY;
            for point in &points {
                let (dx, dy) = (point[0] - center[0], point[1] - center[1]);
                best = best.min(dx * dx + dy * dy);
            }
            sum += best;
        }
        sum as u64
    });
    bench(group, "vas::KdTree", QUERIES, || {
        let mut sum = 0.0f32;
        for center in &centers {
            sum += tree
                .nearest(center)
                .map_or(0.0, |found| found.distance_squared);
        }
        sum as u64
    });

    let group = "10k points: build";
    let mut scratch = items.clone();
    bench(group, "vas::KdTree", N, || {
        scratch.copy_from_slice(&items);
        KdTree::build(&mut scratch).len() as u64
    });
    let mut cells = block::<u32>(32 * 32);
    let mut entries = block::<vas::spatial_grid::Slot<u32, 2>>(N);
    bench(group, "vas::SpatialGrid 32x32", N, || {
        let mut count = 0;
        if let Ok(mut grid) = SpatialGrid::new(world, [32, 32], &mut cells[..], &mut entries[..]) {
            for (index, point) in points.iter().enumerate() {
                let _ = grid.insert(*point, index as u32);
            }
            count = grid.len() as u64;
        }
        count
    });
    let mut nodes = block::<vas::orthtree::Node>(1 + 4 * 4096);
    let mut entries = block::<vas::orthtree::Slot<u32, 2>>(N);
    bench(group, "vas::Quadtree", N, || {
        let mut count = 0;
        if let Ok(mut tree) = Quadtree::new(world, &mut nodes[..], &mut entries[..]) {
            for (index, point) in points.iter().enumerate() {
                let _ = tree.insert(*point, index as u32);
            }
            count = tree.len() as u64;
        }
        count
    });
}

struct Grid {
    width: u32,
    walls: Vec<bool>,
    heuristic: bool,
}

impl Space for Grid {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        let (x, y) = (node % self.width, node / self.width);
        let open = |n: u32| !self.walls.get(n as usize).copied().unwrap_or(true);
        if x > 0 && open(node - 1) {
            visit(node - 1, 1.0);
        }
        if x + 1 < self.width && open(node + 1) {
            visit(node + 1, 1.0);
        }
        if y > 0 && open(node - self.width) {
            visit(node - self.width, 1.0);
        }
        if y + 1 < self.width && open(node + self.width) {
            visit(node + self.width, 1.0);
        }
    }

    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        if !self.heuristic {
            return 0.0;
        }
        let dx = (from % self.width).abs_diff(goal % self.width);
        let dy = (from / self.width).abs_diff(goal / self.width);
        (dx + dy) as f32
    }
}

fn path() {
    const WIDTH: u32 = 128;
    let cells = (WIDTH * WIDTH) as usize;
    let mut rng = Rng::new(8);
    let mut grid = Grid {
        width: WIDTH,
        walls: (0..cells).map(|_| rng.chance(0.2)).collect(),
        heuristic: true,
    };
    for corner in [0, cells - 1] {
        if let Some(wall) = grid.walls.get_mut(corner) {
            *wall = false;
        }
    }
    let mut nodes = block::<vas::path::Node>(cells);
    let mut open = block::<vas::path::Open>(4 * cells);
    let mut finder = PathFinder::new(&mut nodes[..], &mut open[..]);
    let goal = cells as u32 - 1;
    bench(
        "128x128 grid, corner to corner",
        "vas::PathFinder A*",
        1,
        || finder.find(&grid, 0, goal).map_or(0, |cost| cost as u64),
    );
    grid.heuristic = false;
    bench(
        "128x128 grid, corner to corner",
        "vas::PathFinder Dijkstra",
        1,
        || finder.find(&grid, 0, goal).map_or(0, |cost| cost as u64),
    );
}

fn main() {
    println!("| Case | Implementation | ns / op | check |");
    println!("|---|---|---:|---:|");
    vector();
    deque();
    heap();
    map::<2_000>();
    map::<50_000>();
    ordered_map();
    pool();
    bitset();
    random();
    spatial();
    path();
}
