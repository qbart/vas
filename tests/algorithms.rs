use core::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use vas::heap::Min;
use vas::{
    BinaryTree, Deque, Heap, Inline, Map, Memory, OrderedMap, Pool, Rng, Tree, TreeMap, Vector,
};

/// Counts drops, to check that removed elements are dropped exactly once.
struct Tracked<'a>(u32, &'a Cell<u32>);

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.1.set(self.1.get() + 1);
    }
}

/// Sizes above this are skipped under Miri, which is slow.
const MIRI_LIMIT: usize = 300;

fn too_slow(count: usize) -> bool {
    cfg!(miri) && count > MIRI_LIMIT
}

fn random(count: usize, below: u32, seed: u64) -> std::vec::Vec<u32> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| rng.u32_below(below)).collect()
}

// Vector ---------------------------------------------------------------

#[test]
fn vector_retain_keeps_order() {
    let mut v = Vector::<u32, Inline<16>>::inline();
    assert!(v.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]).is_ok());
    v.retain(|x| x % 3 != 0);
    assert_eq!(v.as_slice(), &[1, 2, 4, 5, 7, 8]);
    v.retain_mut(|x| {
        *x *= 10;
        *x > 20
    });
    assert_eq!(v.as_slice(), &[40, 50, 70, 80]);
    v.retain(|_| false);
    assert!(v.is_empty());
    v.retain(|_| true);
    assert!(v.is_empty());
}

#[test]
fn vector_retain_drops_each_removed_element_once() {
    let drops = Cell::new(0);
    {
        let mut v = Vector::<Tracked<'_>, Inline<16>>::inline();
        for i in 0..10 {
            assert!(v.push(Tracked(i, &drops)).is_ok());
        }
        v.retain(|t| t.0 % 2 == 0);
        assert_eq!(drops.get(), 5);
        assert!(v.iter().map(|t| t.0).eq([0, 2, 4, 6, 8]));
    }
    assert_eq!(drops.get(), 10);
}

#[test]
fn vector_dedup() {
    let mut v = Vector::<u32, Inline<16>>::inline();
    assert!(v.extend_from_slice(&[1, 1, 2, 3, 3, 3, 1, 4, 4]).is_ok());
    v.dedup();
    assert_eq!(v.as_slice(), &[1, 2, 3, 1, 4]);

    let mut v = Vector::<i32, Inline<16>>::inline();
    assert!(v.extend_from_slice(&[10, 11, 20, 25, 31, -30]).is_ok());
    v.dedup_by_key(|x| *x / 10);
    assert_eq!(v.as_slice(), &[10, 20, 31, -30]);
    v.dedup_by(|a, b| a.abs() == b.abs());
    assert_eq!(v.as_slice(), &[10, 20, 31, -30]);
    v.dedup_by(|a, b| (*a - *b).abs() < 15);
    assert_eq!(v.as_slice(), &[10, 31, -30]);

    let mut empty = Vector::<u8, Inline<0>>::inline();
    empty.dedup();
    assert!(empty.is_empty());
}

#[test]
fn vector_dedup_drops_each_removed_element_once() {
    let drops = Cell::new(0);
    {
        let mut v = Vector::<Tracked<'_>, Inline<16>>::inline();
        for i in [1, 1, 1, 2, 3, 3] {
            assert!(v.push(Tracked(i, &drops)).is_ok());
        }
        v.dedup_by_key(|t| t.0);
        assert_eq!(drops.get(), 3);
        assert!(v.iter().map(|t| t.0).eq([1, 2, 3]));
    }
    assert_eq!(drops.get(), 6);
}

#[test]
fn vector_stable_sort_matches_std() {
    let mut memory = Memory::<(u32, u32), 3000>::new();
    for (count, below) in [
        (0, 1),
        (1, 5),
        (2, 2),
        (17, 4),
        (100, 10),
        (1000, 50),
        (2999, 100_000),
    ] {
        if too_slow(count) {
            continue;
        }
        let input = random(count, below, count as u64);
        let mut v = Vector::new(&mut memory);
        for (position, key) in input.iter().enumerate() {
            assert!(v.push((*key, position as u32)).is_ok());
        }
        let mut expected: std::vec::Vec<(u32, u32)> = v.as_slice().to_vec();
        expected.sort_by_key(|pair| pair.0);
        v.sort_stable_by_key(|pair| pair.0);
        assert_eq!(v.as_slice(), expected.as_slice(), "{count} elements");
    }
}

#[test]
fn vector_stable_sort_variants() {
    let mut v = Vector::<i32, Inline<16>>::inline();
    assert!(v.extend_from_slice(&[5, -3, 9, 0, -3, 2, 8, 1]).is_ok());
    v.sort_stable();
    assert_eq!(v.as_slice(), &[-3, -3, 0, 1, 2, 5, 8, 9]);
    v.sort_stable_by(|a, b| b.cmp(a));
    assert_eq!(v.as_slice(), &[9, 8, 5, 2, 1, 0, -3, -3]);
    // Equal keys keep their order.
    v.sort_stable_by_key(|x| x.abs() % 2);
    assert_eq!(v.as_slice(), &[8, 2, 0, 9, 5, 1, -3, -3]);
}

#[test]
fn vector_sorted_and_reversed_input() {
    let mut memory = Memory::<u32, 2048>::new();
    let mut v = Vector::new(&mut memory);
    let count = if cfg!(miri) { 256 } else { 2048 };
    assert!(
        v.extend_from_slice(&(0..count).rev().collect::<std::vec::Vec<_>>())
            .is_ok()
    );
    v.sort_stable();
    assert!(v.iter().copied().eq(0..count));
    v.sort_stable();
    assert!(v.iter().copied().eq(0..count));
}

// Deque ----------------------------------------------------------------

#[test]
fn deque_retain_keeps_order_when_wrapped() {
    let mut d = Deque::<u32, Inline<8>>::inline();
    for i in 0..8 {
        assert!(d.push_back(i).is_ok());
    }
    for _ in 0..5 {
        let _ = d.pop_front();
    }
    for i in 8..13 {
        assert!(d.push_back(i).is_ok());
    }
    // 5..13, wrapped around the end of the block.
    d.retain(|x| x % 2 == 1);
    assert!(d.iter().copied().eq([5, 7, 9, 11]));
    assert!(d.push_front(3).is_ok());
    assert!(d.push_back(13).is_ok());
    assert!(d.iter().copied().eq([3, 5, 7, 9, 11, 13]));
}

#[test]
fn deque_make_contiguous() {
    let mut d = Deque::<u32, Inline<6>>::inline();
    for i in 0..6 {
        assert!(d.push_back(i).is_ok());
    }
    let _ = d.pop_front();
    let _ = d.pop_front();
    assert!(d.push_back(6).is_ok());
    assert!(d.push_back(7).is_ok());
    assert_ne!(d.as_slices().1.len(), 0);
    let slice = d.make_contiguous();
    assert_eq!(slice, &[2, 3, 4, 5, 6, 7]);
    slice.sort_unstable_by(|a, b| b.cmp(a));
    assert!(d.iter().copied().eq([7, 6, 5, 4, 3, 2]));
    assert_eq!(d.as_slices().1.len(), 0);
    assert_eq!(d.pop_back(), Some(2));
    assert!(d.push_front(9).is_ok());
    assert_eq!(d.front(), Some(&9));

    let mut empty = Deque::<u32, Inline<0>>::inline();
    assert!(empty.make_contiguous().is_empty());
}

// Pool -----------------------------------------------------------------

#[test]
fn pool_retain_makes_handles_stale() {
    let drops = Cell::new(0);
    let mut pool = Pool::<Tracked<'_>, Inline<16>>::inline();
    let mut handles = Vector::<_, Inline<16>>::inline();
    for i in 0..10 {
        if let Ok(handle) = pool.insert(Tracked(i, &drops)) {
            assert!(handles.push(handle).is_ok());
        }
    }
    pool.retain(|_, t| t.0 >= 4);
    assert_eq!(drops.get(), 4);
    assert_eq!(pool.len(), 6);
    for (i, handle) in handles.iter().enumerate() {
        assert_eq!(pool.contains(*handle), i >= 4);
    }
    // Freed slots are reused.
    for i in 0..4 {
        assert!(pool.insert(Tracked(100 + i, &drops)).is_ok());
    }
    assert_eq!(pool.len(), 10);
    pool.retain(|handle, t| {
        t.0 += 1;
        handle.index() % 2 == 0
    });
    assert_eq!(pool.len(), 5);
}

// Map ------------------------------------------------------------------

#[test]
fn map_retain_matches_std() {
    for (capacity, count) in [(16, 12), (64, 64), (256, 190), (1024, 1000)] {
        if too_slow(capacity) {
            continue;
        }
        let mut slots = std::vec::Vec::new();
        slots.resize_with(capacity, core::mem::MaybeUninit::uninit);
        let mut tags = std::vec::Vec::new();
        tags.resize_with(capacity, core::mem::MaybeUninit::uninit);
        let mut map = Map::new(&mut slots[..], &mut tags[..]);
        let mut model = HashMap::new();
        for key in random(count * 2, 100_000, capacity as u64) {
            if model.len() < count {
                assert_eq!(map.insert(key, key).ok(), Some(model.insert(key, key)));
            }
        }
        for round in 0..4u32 {
            let mut calls = 0;
            map.retain(|key, value| {
                calls += 1;
                *value += 1;
                (key ^ round) % 3 != 0
            });
            model.retain(|key, value| {
                *value += 1;
                (key ^ round) % 3 != 0
            });
            assert!(calls >= map.len());
            assert_eq!(map.len(), model.len());
            for (key, value) in &model {
                assert_eq!(map.get(key), Some(value), "capacity {capacity}");
            }
            // Every key that was dropped is gone and can come back.
            for key in random(50, 100_000, u64::from(round)) {
                assert_eq!(map.contains_key(&key), model.contains_key(&key));
            }
        }
        // The map keeps working after retain.
        for key in random(count, 100_000, 99) {
            if model.len() < count {
                assert_eq!(map.insert(key, 0).ok(), Some(model.insert(key, 0)));
            }
        }
        assert_eq!(map.len(), model.len());
        assert!(model.iter().all(|(key, value)| map.get(key) == Some(value)));
    }
}

#[test]
fn map_retain_visits_each_entry_once() {
    let mut map = Map::<u32, u32, Inline<64>>::inline();
    // A completely full map: no empty slot anywhere.
    let mut key = 0;
    while map.insert(key, 0).is_ok() {
        key += 1;
    }
    assert!(map.is_full());
    map.retain(|key, visits| {
        *visits += 1;
        key % 4 != 1
    });
    assert!(map.iter().all(|(_, visits)| *visits == 1));
    assert_eq!(map.len(), 48);
    map.retain(|_, visits| {
        *visits += 1;
        true
    });
    assert!(map.iter().all(|(_, visits)| *visits == 2));
}

#[test]
fn map_retain_drops_each_removed_entry_once() {
    let drops = Cell::new(0);
    {
        let mut map = Map::<u32, Tracked<'_>, Inline<32>>::inline();
        for key in 0..20 {
            assert!(map.insert(key, Tracked(key, &drops)).is_ok());
        }
        map.retain(|key, _| key % 2 == 0);
        assert_eq!(drops.get(), 10);
        assert_eq!(map.len(), 10);
    }
    assert_eq!(drops.get(), 20);
}

// Ordered maps ---------------------------------------------------------

#[test]
fn ordered_maps_retain() {
    let mut ordered = OrderedMap::<u32, u32, Inline<64>>::inline();
    let mut tree = TreeMap::<u32, u32, Inline<64>>::inline();
    let mut model = BTreeMap::new();
    for key in random(60, 200, 3) {
        let _ = ordered.insert(key, key);
        let _ = tree.insert(key, key);
        model.insert(key, key);
    }
    let keep = |key: &u32, value: &mut u32| {
        *value += 1;
        !key.is_multiple_of(3)
    };
    ordered.retain(keep);
    tree.retain(keep);
    model.retain(|key, value| keep(key, value));
    assert!(ordered.iter().eq(model.iter()));
    assert!(tree.iter().eq(model.iter()));
    assert!(tree.iter().rev().eq(model.iter().rev()));
    tree.retain(|_, _| false);
    assert!(tree.is_empty());
    assert!(tree.insert(1, 1).is_ok());
}

#[test]
fn tree_map_retain_drops_each_removed_entry_once() {
    let drops = Cell::new(0);
    {
        let mut tree = TreeMap::<u32, Tracked<'_>, Inline<64>>::inline();
        for key in 0..50 {
            assert!(tree.insert(key * 7 % 50, Tracked(key, &drops)).is_ok());
        }
        let mut seen = 0;
        tree.retain(|key, _| {
            seen += 1;
            key % 5 == 0
        });
        assert_eq!(seen, 50);
        assert_eq!(drops.get(), 40);
        assert!(tree.iter().map(|(key, _)| *key).eq((0..50).step_by(5)));
    }
    assert_eq!(drops.get(), 50);
}

// Heap -----------------------------------------------------------------

#[test]
fn heap_retain_keeps_the_heap_order() {
    let mut heap = Heap::<u32, Inline<256>, Min>::default();
    for key in random(200, 1000, 8) {
        assert!(heap.push(key).is_ok());
    }
    heap.retain(|x| x % 2 == 0);
    let mut expected: std::vec::Vec<u32> = random(200, 1000, 8)
        .into_iter()
        .filter(|x| x % 2 == 0)
        .collect();
    expected.sort_unstable();
    let mut drained = std::vec::Vec::new();
    while let Some(top) = heap.pop() {
        drained.push(top);
    }
    assert_eq!(drained, expected);
}

// Tree -----------------------------------------------------------------

/// root
/// ├── a
/// │   ├── a1
/// │   └── a2
/// │       └── a21
/// └── b
///     └── b1
type Sample = (
    Tree<&'static str, Inline<16>>,
    [vas::tree::NodeId<&'static str>; 7],
);

fn sample_tree() -> Option<Sample> {
    let mut tree = Tree::inline();
    let root = tree.insert_root("root").ok()?;
    let a = tree.insert_child(root, "a").ok()?;
    let a1 = tree.insert_child(a, "a1").ok()?;
    let a2 = tree.insert_child(a, "a2").ok()?;
    let a21 = tree.insert_child(a2, "a21").ok()?;
    let b = tree.insert_child(root, "b").ok()?;
    let b1 = tree.insert_child(b, "b1").ok()?;
    Some((tree, [root, a, a1, a2, a21, b, b1]))
}

#[test]
fn tree_post_order_puts_children_first() {
    let Some((tree, [root, a, a1, a2, a21, b, b1])) = sample_tree() else {
        return;
    };
    assert!(tree.post_order(root).eq([a1, a21, a2, a, b1, b, root]));
    assert!(tree.post_order(a).eq([a1, a21, a2, a]));
    assert!(tree.post_order(b1).eq([b1]));
    let names: std::vec::Vec<_> = tree
        .post_order(root)
        .filter_map(|id| tree.get(id).copied())
        .collect();
    assert_eq!(names, ["a1", "a21", "a2", "a", "b1", "b", "root"]);
}

#[test]
fn tree_breadth_first_lists_levels() {
    let Some((tree, [root, a, a1, a2, a21, b, b1])) = sample_tree() else {
        return;
    };
    let mut out = Vector::<_, Inline<16>>::inline();
    assert_eq!(tree.breadth_first_into(root, &mut out), Ok(()));
    assert_eq!(out.as_slice(), &[root, a, b, a1, a2, b1, a21]);

    // Appends; a subtree works too.
    assert_eq!(tree.breadth_first_into(a2, &mut out), Ok(()));
    assert_eq!(out.len(), 9);
    assert_eq!(&out[7..], &[a2, a21]);

    // Too little room: unchanged.
    let mut small = Vector::<_, Inline<4>>::inline();
    assert!(small.push(b1).is_ok());
    assert!(tree.breadth_first_into(root, &mut small).is_err());
    assert_eq!(small.as_slice(), &[b1]);
}

#[test]
fn tree_common_ancestor() {
    let Some((mut tree, [root, a, a1, a2, a21, b, b1])) = sample_tree() else {
        return;
    };
    assert_eq!(tree.common_ancestor(a1, a21), Some(a));
    assert_eq!(tree.common_ancestor(a21, b1), Some(root));
    assert_eq!(tree.common_ancestor(a21, a2), Some(a2));
    assert_eq!(tree.common_ancestor(b, b), Some(b));
    let Ok(other) = tree.insert_root("other") else {
        return;
    };
    assert_eq!(tree.common_ancestor(other, a1), None);
    assert!(tree.remove(b).is_some());
    assert_eq!(tree.common_ancestor(b1, a1), None);
}

// Binary tree ----------------------------------------------------------

#[test]
fn binary_tree_level_order_and_common_ancestor() {
    let mut tree = BinaryTree::<u32, Inline<16>>::inline();
    let Ok(root) = tree.set_root(1) else {
        return;
    };
    let (Ok(l), Ok(r)) = (tree.insert_left(root, 2), tree.insert_right(root, 3)) else {
        return;
    };
    let (Ok(ll), Ok(lr), Ok(rr)) = (
        tree.insert_left(l, 4),
        tree.insert_right(l, 5),
        tree.insert_right(r, 7),
    ) else {
        return;
    };
    let Ok(lrl) = tree.insert_left(lr, 10) else {
        return;
    };
    let mut out = Vector::<_, Inline<16>>::inline();
    assert_eq!(tree.level_order_into(root, &mut out), Ok(()));
    assert_eq!(out.as_slice(), &[root, l, r, ll, lr, rr, lrl]);
    out.clear();
    assert_eq!(tree.level_order_into(l, &mut out), Ok(()));
    assert_eq!(out.as_slice(), &[l, ll, lr, lrl]);

    assert_eq!(tree.common_ancestor(ll, lrl), Some(l));
    assert_eq!(tree.common_ancestor(lrl, rr), Some(root));
    assert_eq!(tree.common_ancestor(lr, lrl), Some(lr));
    assert_eq!(tree.height(root), Some(3));
    assert_eq!(tree.height(rr), Some(0));
    assert_eq!(tree.height(r), Some(1));
}

#[test]
fn deep_trees_need_no_recursion() {
    let mut tree = BinaryTree::<u32, Inline<4096>>::inline();
    let Ok(mut node) = tree.set_root(0) else {
        return;
    };
    let count = if cfg!(miri) { 256 } else { 4096 };
    for i in 1..count {
        let Ok(next) = tree.insert_right(node, i) else {
            return;
        };
        node = next;
    }
    let root = tree.root();
    assert_eq!(
        root.and_then(|root| tree.height(root)),
        Some(count as usize - 1)
    );
    let mut out = Vector::<_, Inline<4096>>::inline();
    assert!(root.is_some_and(|root| tree.level_order_into(root, &mut out).is_ok()));
    assert_eq!(out.len(), count as usize);
}

#[test]
fn vector_stable_sort_with_scratch_matches_std() {
    let mut memory = Memory::<(u32, u32), 3000>::new();
    for scratch_len in [0usize, 1, 7, 64, 1500] {
        let mut scratch = Memory::<(u32, u32), 1500>::new();
        for (count, below) in [(0, 1), (2, 2), (33, 4), (1000, 50), (2999, 100_000)] {
            if too_slow(count) {
                continue;
            }
            let input = random(count, below, (count + scratch_len) as u64);
            let mut v = Vector::new(&mut memory);
            for (position, key) in input.iter().enumerate() {
                assert!(v.push((*key, position as u32)).is_ok());
            }
            let mut expected: std::vec::Vec<(u32, u32)> = v.as_slice().to_vec();
            expected.sort_by_key(|pair| pair.0);
            let scratch = scratch.get_mut(..scratch_len).unwrap_or_default();
            v.sort_stable_in(scratch, |a, b| a.0.cmp(&b.0));
            assert_eq!(
                v.as_slice(),
                expected.as_slice(),
                "{count} with {scratch_len}"
            );
        }
    }
}

#[test]
fn stable_sort_keeps_every_element_when_compare_panics() {
    let drops = Cell::new(0);
    let mut scratch = Memory::<Tracked<'_>, 128>::new();
    let mut expected = random(128, 1000, 11);
    expected.sort_unstable();
    let mut panicked = 0;
    // No scratch, part of it, and room for every element.
    for scratch_len in [0usize, 64, 128] {
        // Fail at many points: in the insertion runs and in the merges.
        let step = if cfg!(miri) { 250 } else { 50 };
        for limit in (50..1500).step_by(step) {
            drops.set(0);
            let mut v = Vector::<Tracked<'_>, Inline<128>>::inline();
            for key in random(128, 1000, 11) {
                assert!(v.push(Tracked(key, &drops)).is_ok());
            }
            let mut calls = 0;
            let scratch = scratch.get_mut(..scratch_len).unwrap_or_default();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                v.sort_stable_in(scratch, |a, b| {
                    calls += 1;
                    assert!(calls < limit, "comparison failed");
                    a.0.cmp(&b.0)
                });
            }));
            panicked += usize::from(result.is_err());
            // No element was lost or duplicated.
            assert_eq!(drops.get(), 0);
            let mut keys: std::vec::Vec<u32> = v.iter().map(|t| t.0).collect();
            keys.sort_unstable();
            assert_eq!(keys, expected);
            drop(v);
            assert_eq!(drops.get(), 128);
        }
    }
    assert!(panicked > if cfg!(miri) { 9 } else { 45 });
}
