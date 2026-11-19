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

use vas::grid::Moves;
use vas::hash::FastState;
use vas::path::Space;
use vas::{
    Aabb, Bitset, Deque, FlowField, Heap, KdTree, LruCache, Map, OrderedMap, PathFinder, Pool,
    Quadtree, Rng, SparseSet, SpatialGrid, TreeMap, Vector,
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
///
/// `cargo bench -- map` only runs the cases whose name contains `map`.
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
    let mut memory = block::<u64>(N);
    bench("push 100k u64", "vas::Vector", N, || {
        let mut v = Vector::new(&mut memory[..]);
        for i in 0..N as u64 {
            let _ = v.push(black_box(i));
        }
        v.len() as u64
    });
    #[cfg(feature = "alloc")]
    bench("push 100k u64", "vas::Vector (growing)", N, || {
        let mut v = Vector::<u64, vas::Dynamic>::default();
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
    let mut tags = block::<u8>(2 * N);
    bench(
        &format!("map {N} keys: insert"),
        "vas::Map (50% full)",
        N,
        || {
            let mut map = Map::new(&mut memory[..], &mut tags[..]);
            for key in &input {
                let _ = map.insert(*key, *key);
            }
            map.len() as u64
        },
    );
    #[cfg(feature = "alloc")]
    bench(
        &format!("map {N} keys: insert"),
        "vas::Map (growing)",
        N,
        || {
            let mut map = Map::<u32, u32, vas::Dynamic>::default();
            for key in &input {
                let _ = map.insert(*key, *key);
            }
            map.len() as u64
        },
    );
    #[cfg(feature = "alloc")]
    bench(
        &format!("map {N} keys: insert"),
        "std HashMap + vas FastHasher (growing)",
        N,
        || {
            let mut map = HashMap::with_hasher(FastState::default());
            for key in &input {
                map.insert(*key, *key);
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
        let mut tags = block::<u8>(fill);
        let mut map = Map::new(&mut memory[..], &mut tags[..]);
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
    lookups("vas::Map (75% full)", N * 4 / 3);
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
    let mut tags = block::<u8>(2 * N);
    bench(
        &format!("map {N} keys: insert then remove"),
        "vas::Map (50% full)",
        2 * N,
        || {
            let mut map = Map::new(&mut memory[..], &mut tags[..]);
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

fn sort() {
    // Several inputs in turn: sorting the same one again and again lets the
    // branch predictor learn it, which flatters branchy sorts.
    const INPUTS: usize = 8;
    for (size, distinct) in [(1_000usize, 1000), (100_000, 1000), (100_000, u32::MAX)] {
        let inputs: Vec<Vec<(u32, u32)>> = (0..INPUTS as u64)
            .map(|seed| {
                keys(size, 6 + seed)
                    .into_iter()
                    .enumerate()
                    .map(|(position, key)| (key % distinct, position as u32))
                    .collect()
            })
            .collect();
        let round = std::cell::Cell::new(0usize);
        let next = || {
            round.set(round.get() + 1);
            &inputs[round.get() % INPUTS]
        };
        let group = if distinct == u32::MAX {
            format!("sort {size} (u32, u32) by key, all distinct")
        } else {
            format!("sort {size} (u32, u32) by key")
        };
        let mut memory = block::<(u32, u32)>(size);
        bench(&group, "vas::Vector sort_unstable_by_key", size, || {
            let mut v = Vector::new(&mut memory[..]);
            let _ = v.extend_from_slice(next());
            v.sort_unstable_by_key(|pair| pair.0);
            v.first().map_or(0, |pair| u64::from(pair.0))
        });
        bench(&group, "core slice sort_unstable_by_key", size, || {
            let mut v = Vector::new(&mut memory[..]);
            let _ = v.extend_from_slice(next());
            v.as_mut_slice().sort_unstable_by_key(|pair| pair.0);
            v.first().map_or(0, |pair| u64::from(pair.0))
        });
        bench(&group, "vas::Vector sort_stable_by_key", size, || {
            let mut v = Vector::new(&mut memory[..]);
            let _ = v.extend_from_slice(next());
            v.sort_stable_by_key(|pair| pair.0);
            v.first().map_or(0, |pair| u64::from(pair.0))
        });
        let mut scratch = block::<(u32, u32)>(size / 2);
        bench(
            &group,
            "vas::Vector sort_stable_in (n/2 scratch)",
            size,
            || {
                let mut v = Vector::new(&mut memory[..]);
                let _ = v.extend_from_slice(next());
                v.sort_stable_in(&mut scratch, |a, b| a.0.cmp(&b.0));
                v.first().map_or(0, |pair| u64::from(pair.0))
            },
        );
        let mut small = block::<(u32, u32)>(256);
        bench(
            &group,
            "vas::Vector sort_stable_in (256 scratch)",
            size,
            || {
                let mut v = Vector::new(&mut memory[..]);
                let _ = v.extend_from_slice(next());
                v.sort_stable_in(&mut small, |a, b| a.0.cmp(&b.0));
                v.first().map_or(0, |pair| u64::from(pair.0))
            },
        );
        let mut std_v = Vec::with_capacity(size);
        bench(&group, "std sort_by_key (allocates)", size, || {
            std_v.clear();
            std_v.extend_from_slice(next());
            std_v.sort_by_key(|pair| pair.0);
            std_v.first().map_or(0, |pair| u64::from(pair.0))
        });
    }
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
        let mut tree_memory = block::<vas::tree_map::Node<u32, u32>>(size);
        bench(&group, "vas::TreeMap", size, || {
            let mut map = TreeMap::new(&mut tree_memory[..]);
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
        let mut tree = TreeMap::new(&mut tree_memory[..]);
        let mut std_map = BTreeMap::new();
        for key in &input {
            let _ = map.insert(*key, *key);
            let _ = tree.insert(*key, *key);
            std_map.insert(*key, *key);
        }
        bench(&group, "vas::OrderedMap", size, || {
            input
                .iter()
                .filter_map(|key| map.get(key))
                .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
        });
        bench(&group, "vas::TreeMap", size, || {
            input
                .iter()
                .filter_map(|key| tree.get(key))
                .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
        });
        bench(&group, "std BTreeMap", size, || {
            input
                .iter()
                .filter_map(|key| std_map.get(key))
                .fold(0u64, |sum, v| sum.wrapping_add(*v as u64))
        });

        // A full map that keeps changing: every key is taken out and a
        // new one put in.
        let group = format!("ordered {size}: remove+insert");
        let fresh = keys(size, 5);
        bench(&group, "vas::OrderedMap", 4 * size, || {
            let mut sum = 0u64;
            for (old, new) in input.iter().zip(&fresh) {
                sum += map.remove(old).unwrap_or(0) as u64;
                let _ = map.insert(*new, *new);
            }
            for (old, new) in input.iter().zip(&fresh) {
                sum += map.remove(new).unwrap_or(0) as u64;
                let _ = map.insert(*old, *old);
            }
            sum / 2
        });
        bench(&group, "vas::TreeMap", 4 * size, || {
            let mut sum = 0u64;
            for (old, new) in input.iter().zip(&fresh) {
                sum += tree.remove(old).unwrap_or(0) as u64;
                let _ = tree.insert(*new, *new);
            }
            for (old, new) in input.iter().zip(&fresh) {
                sum += tree.remove(new).unwrap_or(0) as u64;
                let _ = tree.insert(*old, *old);
            }
            sum / 2
        });
        bench(&group, "std BTreeMap", 4 * size, || {
            let mut sum = 0u64;
            for (old, new) in input.iter().zip(&fresh) {
                sum += std_map.remove(old).unwrap_or(0) as u64;
                std_map.insert(*new, *new);
            }
            for (old, new) in input.iter().zip(&fresh) {
                sum += std_map.remove(new).unwrap_or(0) as u64;
                std_map.insert(*old, *old);
            }
            sum / 2
        });

        let group = format!("ordered {size}: iterate");
        bench(&group, "vas::OrderedMap", size, || {
            map.iter()
                .fold(0u64, |sum, (_, v)| sum.wrapping_add(*v as u64))
        });
        bench(&group, "vas::TreeMap", size, || {
            tree.iter()
                .fold(0u64, |sum, (_, v)| sum.wrapping_add(*v as u64))
        });
        bench(&group, "std BTreeMap", size, || {
            std_map
                .iter()
                .fold(0u64, |sum, (_, v)| sum.wrapping_add(*v as u64))
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

    // The same map as a vas::Grid.
    let mut tiles = block::<bool>(cells);
    let walls = &grid.walls;
    let Ok(level) = vas::Grid::from_fn(&mut tiles[..], WIDTH, WIDTH, |x, y| {
        walls.get((y * WIDTH + x) as usize).copied().unwrap_or(true)
    }) else {
        return;
    };
    let walk = level.space(Moves::Four, |_, wall| (!wall).then_some(1.0));
    bench(
        "128x128 grid, corner to corner",
        "vas::PathFinder A* on Grid::space",
        1,
        || finder.find(&walk, 0, goal).map_or(0, |cost| cost as u64),
    );
    let mut field_nodes = block::<vas::path::Node>(cells);
    let mut field_open = block::<vas::path::Open>(4 * cells);
    let mut field = FlowField::new(&mut field_nodes[..], &mut field_open[..]);
    bench(
        "128x128 grid, flow field to a corner",
        "vas::FlowField build",
        1,
        || {
            let built = field.build(&walk, &[goal], f32::INFINITY);
            built.map_or(0, |()| field.cost(0).map_or(0, |cost| cost as u64))
        },
    );
    let centre = WIDTH / 2;
    bench(
        "128x128 grid, field of view radius 64",
        "vas::Grid::field_of_view",
        1,
        || {
            let mut seen = 0;
            level.field_of_view(centre, centre, 64, |wall| *wall, |_, _| seen += 1);
            seen
        },
    );
    let mut rng = Rng::new(4);
    let pairs: Vec<_> = (0..1000)
        .map(|_| {
            let a = (rng.u32_below(WIDTH), rng.u32_below(WIDTH));
            (a, (rng.u32_below(WIDTH), rng.u32_below(WIDTH)))
        })
        .collect();
    bench(
        "128x128 grid, line of sight",
        "vas::Grid::line_of_sight",
        pairs.len(),
        || {
            let clear = pairs
                .iter()
                .filter(|(a, b)| level.line_of_sight(*a, *b, |wall| *wall));
            clear.count() as u64
        },
    );
}

/// Entity ids below 100,000, 10,000 of them: a component store.
fn sparse_set() {
    const IDS: u32 = 100_000;
    const N: usize = 10_000;
    let mut rng = Rng::new(12);
    let mut input: Vec<u32> = (0..IDS).collect();
    rng.shuffle(&mut input);
    input.truncate(N);
    let mut lookups = input.clone();
    rng.shuffle(&mut lookups);

    let mut sparse = block::<u32>(IDS as usize);
    let mut ids = block::<u32>(N);
    let mut values = block::<u64>(N);
    let mut set = SparseSet::new(&mut sparse[..], &mut ids[..], &mut values[..]);
    bench(
        "components 10k of 100k ids: insert",
        "vas::SparseSet",
        N,
        || {
            set.clear();
            for id in &input {
                let _ = set.insert(*id, u64::from(*id));
            }
            set.len() as u64
        },
    );
    let mut memory = block::<vas::map::Slot<u32, u64>>(2 * N);
    let mut tags = block::<u8>(2 * N);
    let mut map = Map::new(&mut memory[..], &mut tags[..]);
    bench(
        "components 10k of 100k ids: insert",
        "vas::Map (50% full)",
        N,
        || {
            map.clear();
            for id in &input {
                let _ = map.insert(*id, u64::from(*id));
            }
            map.len() as u64
        },
    );
    let mut hash = HashMap::with_capacity_and_hasher(N, FastState::default());
    bench(
        "components 10k of 100k ids: insert",
        "std HashMap + vas FastHasher",
        N,
        || {
            hash.clear();
            for id in &input {
                hash.insert(*id, u64::from(*id));
            }
            hash.len() as u64
        },
    );

    bench(
        "components 10k of 100k ids: get",
        "vas::SparseSet",
        N,
        || lookups.iter().filter_map(|id| set.get(*id)).sum(),
    );
    bench(
        "components 10k of 100k ids: get",
        "vas::Map (50% full)",
        N,
        || lookups.iter().filter_map(|id| map.get(id)).sum(),
    );
    bench(
        "components 10k of 100k ids: get",
        "std HashMap + vas FastHasher",
        N,
        || lookups.iter().filter_map(|id| hash.get(id)).sum(),
    );

    bench(
        "components 10k of 100k ids: iterate",
        "vas::SparseSet",
        N,
        || set.values().iter().sum(),
    );
    bench(
        "components 10k of 100k ids: iterate",
        "vas::Map (50% full)",
        N,
        || map.iter().map(|(_, value)| *value).sum(),
    );
    bench(
        "components 10k of 100k ids: iterate",
        "std HashMap + vas FastHasher",
        N,
        || hash.values().sum(),
    );
}

/// A cache of 1024 entries in front of 2048 keys: about half the lookups hit.
fn lru() {
    const N: usize = 100_000;
    let mut rng = Rng::new(13);
    let keys: Vec<u32> = (0..N).map(|_| rng.u32_below(2048)).collect();
    let mut entries = block::<vas::lru::Entry<u32, u64>>(1024);
    let mut table = block::<u32>(2048);
    let Ok(mut cache) = LruCache::new(&mut entries[..], &mut table[..], 1024) else {
        return;
    };
    bench(
        "lru 1024 of 2048 keys: get, insert on a miss",
        "vas::LruCache",
        N,
        || {
            let mut hits = 0;
            for key in &keys {
                if cache.get(key).is_some() {
                    hits += 1;
                } else {
                    cache.insert(*key, u64::from(*key));
                }
            }
            hits
        },
    );
}

/// Binary search in 100k sorted keys, and the median of 100k (8 inputs in
/// turn, so the branch predictor cannot learn one).
fn search() {
    const N: usize = 100_000;
    let mut rng = Rng::new(14);
    let mut sorted: Vec<u32> = (0..N).map(|_| rng.u32()).collect();
    sorted.sort_unstable();
    let targets: Vec<u32> = (0..N).map(|_| rng.u32()).collect();
    let group = "binary search 100k sorted u32";
    bench(group, "vas::search::binary_search", N, || {
        targets
            .iter()
            .map(|t| vas::search::binary_search(&sorted, t).unwrap_or_else(|i| i) as u64)
            .sum()
    });
    bench(group, "core slice binary_search", N, || {
        targets
            .iter()
            .map(|t| sorted.binary_search(t).unwrap_or_else(|i| i) as u64)
            .sum()
    });

    let inputs: Vec<Vec<u32>> = (0..8)
        .map(|_| (0..N).map(|_| rng.u32()).collect())
        .collect();
    let mut work = inputs[0].clone();
    let turn = std::cell::Cell::new(0);
    let next = |work: &mut Vec<u32>| {
        let input = &inputs[turn.get() % inputs.len()];
        turn.set(turn.get() + 1);
        work.copy_from_slice(input);
    };
    let group = "median of 100k u32";
    bench(group, "vas::sort::select_nth_unstable", N, || {
        next(&mut work);
        vas::sort::select_nth_unstable(&mut work, N / 2).map_or(0, |(_, m, _)| u64::from(*m))
    });
    bench(group, "core select_nth_unstable", N, || {
        next(&mut work);
        u64::from(*work.select_nth_unstable(N / 2).1)
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

/// The bounded MPMC queue against a `Mutex<VecDeque>` of the same capacity.
fn queue() {
    use std::sync::Mutex;
    use vas::sync::{Queue, Slot};
    const CAPACITY: usize = 1024;
    const N: usize = 1 << 20;
    let mut slots = block::<Slot<u64>>(CAPACITY);
    let queue = Queue::new(&mut slots[..]);
    let locked = Mutex::new(VecDeque::with_capacity(CAPACITY));
    let locked_push = |value: u64| {
        locked.lock().is_ok_and(|mut q| {
            q.len() < CAPACITY && {
                q.push_back(value);
                true
            }
        })
    };
    let locked_pop = || locked.lock().ok().and_then(|mut q| q.pop_front());

    let group = "queue, one thread: push then pop";
    bench(group, "vas::sync::Queue", N, || {
        (0..N as u64)
            .map(|i| {
                let _ = queue.push(i);
                queue.pop().unwrap_or(0)
            })
            .sum()
    });
    bench(group, "Mutex<VecDeque>", N, || {
        (0..N as u64)
            .map(|i| {
                locked_push(i);
                locked_pop().unwrap_or(0)
            })
            .sum()
    });

    const MOVED: usize = 1 << 18;
    for threads in [1, 2, 4, 8] {
        let group = format!("queue, {threads} producers + {threads} consumers, 1024 slots");
        bench(&group, "vas::sync::Queue", MOVED, || {
            exchange(threads, MOVED, |v| queue.push(v).is_ok(), || queue.pop())
        });
        bench(&group, "Mutex<VecDeque>", MOVED, || {
            exchange(threads, MOVED, locked_push, locked_pop)
        });
    }
}

fn main() {
    println!("| Case | Implementation | ns / op | check |");
    println!("|---|---|---:|---:|");
    // Lets the CPU reach full speed before the first case is timed.
    let start = Instant::now();
    let mut spin = 0u64;
    while start.elapsed().as_millis() < 1000 {
        spin = black_box(spin.wrapping_mul(31).wrapping_add(7));
    }
    vector();
    deque();
    heap();
    map::<2_000>();
    map::<50_000>();
    sort();
    search();
    ordered_map();
    pool();
    bitset();
    random();
    spatial();
    path();
    sparse_set();
    lru();
    queue();
}
