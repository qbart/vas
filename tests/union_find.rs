use std::error::Error;
use std::vec::Vec;
use vas::union_find::Link;
use vas::{Arena, Dynamic, Full, Inline, Memory, OutOfBounds, Rng, UnionFind};

type Test = Result<(), Box<dyn Error>>;

#[test]
fn starts_with_singletons() -> Test {
    let mut memory = Memory::<Link, 8>::new();
    let mut sets = UnionFind::new(&mut memory);
    assert!(sets.is_empty());
    assert_eq!(sets.capacity(), 8);
    sets.reset(5)?;
    assert_eq!((sets.len(), sets.set_count()), (5, 5));
    for element in 0..5 {
        assert_eq!(sets.find(element), Some(element));
        assert_eq!(sets.set_size(element), Some(1));
    }
    assert_eq!(sets.find(5), None);
    assert_eq!(sets.add(), Ok(5));
    assert_eq!(sets.len(), 6);
    assert_eq!(sets.reset(9), Err(Full(())));
    assert_eq!(sets.len(), 6);
    Ok(())
}

#[test]
fn union_joins_sets() -> Test {
    let mut sets = UnionFind::<Inline<10>>::inline();
    sets.reset(10)?;
    assert_eq!(sets.union(1, 2), Ok(true));
    assert_eq!(sets.union(3, 4), Ok(true));
    assert_eq!(sets.union(2, 4), Ok(true));
    assert_eq!(sets.union(1, 3), Ok(false));
    assert_eq!(sets.set_count(), 7);
    assert!(sets.same(1, 4));
    assert!(!sets.same(1, 5));
    assert!(!sets.same(1, 10));
    assert_eq!(sets.set_size(3), Some(4));
    assert_eq!(sets.find(4), sets.find(1));
    assert_eq!(sets.root(4), sets.find(1));
    assert_eq!(sets.union(0, 10), Err(OutOfBounds));
    assert_eq!(sets.union(10, 0), Err(OutOfBounds));
    // An element joined with itself changes nothing.
    assert_eq!(sets.union(7, 7), Ok(false));
    sets.clear();
    assert!(sets.is_empty());
    assert_eq!(sets.set_count(), 0);
    Ok(())
}

/// Random unions against a brute-force labelling.
#[test]
fn matches_a_labelling() -> Test {
    let n = if cfg!(miri) { 40 } else { 300 };
    let mut rng = Rng::new(17);
    let mut sets = UnionFind::<Inline<300>>::inline();
    sets.reset(n)?;
    let mut label: Vec<usize> = (0..n).collect();
    for step in 0..n * 2 {
        let a = rng.usize_below(n);
        let b = rng.usize_below(n);
        let joined = sets.union(a as u32, b as u32)?;
        let (from, to) = (label[a], label[b]);
        assert_eq!(joined, from != to);
        for entry in &mut label {
            if *entry == from {
                *entry = to;
            }
        }
        if step % 37 == 0 {
            let mut distinct = label.clone();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(sets.set_count(), distinct.len());
            for _ in 0..50 {
                let (a, b) = (rng.usize_below(n), rng.usize_below(n));
                assert_eq!(sets.same(a as u32, b as u32), label[a] == label[b]);
                let size = label.iter().filter(|l| **l == label[a]).count();
                assert_eq!(sets.set_size(a as u32), Some(size as u32));
            }
        }
    }
    Ok(())
}

/// Joining in rounds (pairs, pairs of pairs, ...) is the worst case for
/// naive union; every element still finds the one root.
#[test]
fn rounds_of_unions_end_in_one_set() -> Test {
    let n: u32 = if cfg!(miri) { 256 } else { 4096 };
    let mut sets = UnionFind::<Inline<4096>>::inline();
    sets.reset(n as usize)?;
    // Join neighbours in rounds: pairs, then pairs of pairs, ...
    let mut width = 1;
    while width < n {
        for start in (0..n).step_by(2 * width as usize) {
            if start + width < n {
                sets.union(start, start + width)?;
            }
        }
        width *= 2;
    }
    assert_eq!(sets.set_count(), 1);
    let root = sets.root(0);
    assert!(root.is_some());
    assert!((0..n).all(|e| sets.root(e) == root));
    assert_eq!(sets.set_size(n - 1), Some(n));
    Ok(())
}

#[test]
fn kruskal_spanning_tree() -> Test {
    // (cost, a, b) on 5 nodes.
    let mut edges = [
        (4, 0, 1),
        (1, 1, 2),
        (3, 0, 2),
        (2, 2, 3),
        (7, 3, 4),
        (5, 1, 4),
        (6, 0, 4),
    ];
    edges.sort_unstable();
    let mut sets = UnionFind::<Inline<5>>::inline();
    sets.reset(5)?;
    let mut total = 0;
    for (cost, a, b) in edges {
        if sets.union(a, b)? {
            total += cost;
        }
    }
    assert_eq!(total, 1 + 2 + 3 + 5);
    assert_eq!(sets.set_count(), 1);
    Ok(())
}

#[test]
fn dynamic_storage_grows() -> Test {
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut sets = UnionFind::new(Dynamic(&arena));
    for _ in 0..1000 {
        sets.add()?;
    }
    for element in 1..1000 {
        sets.union(element - 1, element)?;
    }
    assert_eq!(sets.set_count(), 1);
    assert_eq!(sets.set_size(999), Some(1000));
    sets.reset(3000)?;
    assert_eq!(sets.set_count(), 3000);
    Ok(())
}

#[test]
fn clone_is_independent() -> Test {
    let mut sets = UnionFind::<Inline<4>>::inline();
    sets.reset(4)?;
    let copy = sets.clone();
    sets.union(0, 1)?;
    assert_eq!(copy.set_count(), 4);
    assert_eq!(sets.set_count(), 3);
    assert_eq!(format!("{copy:?}"), "UnionFind { len: 4, sets: 4 }");
    Ok(())
}
