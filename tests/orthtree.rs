use core::cell::Cell;
use std::vec::Vec;
use vas::orthtree::{EntryId, NewError, Node, Orthtree, Slot};
use vas::{Aabb, Borrowed, Full, Memory, Octree, Quadtree, Rng};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn world() -> Aabb<2> {
    Aabb::new([0.0, 0.0], [100.0, 100.0])
}

fn found(tree: &Quadtree<u32, Borrowed<'_>>, area: &Aabb<2>) -> Vec<u32> {
    let mut out = Vec::new();
    tree.query(area, |_, value| out.push(*value));
    out.sort_unstable();
    out
}

#[test]
fn new_checks_its_arguments() {
    let mut nodes = Memory::<Node, 9>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let tree = Quadtree::<u32, _, _>::new(world(), &mut nodes, &mut entries);
    assert!(
        tree.is_ok_and(|tree| tree.is_empty() && tree.capacity() == 4 && tree.node_count() == 1)
    );

    let mut nodes = Memory::<Node, 0>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    assert_eq!(
        Quadtree::<u32, _, _>::new(world(), &mut nodes, &mut entries).err(),
        Some(NewError::Full)
    );

    let mut nodes = Memory::<Node, 9>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let flat = Aabb::new([0.0, 0.0], [0.0, 10.0]);
    assert_eq!(
        Quadtree::<u32, _, _>::new(flat, &mut nodes, &mut entries).err(),
        Some(NewError::Invalid)
    );

    let mut nodes = Memory::<Node, 9>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let endless = Aabb::new([0.0, 0.0], [f32::INFINITY, 10.0]);
    assert_eq!(
        Quadtree::<u32, _, _>::new(endless, &mut nodes, &mut entries).err(),
        Some(NewError::Invalid)
    );
}

#[test]
fn insert_query_remove() {
    let mut nodes = Memory::<Node, 64>::new();
    let mut entries = Memory::<Slot<u32, 2>, 16>::new();
    let tree = Quadtree::new(world(), &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        let a = tree.insert([5.0, 5.0], 1).ok();
        let b = tree.insert([15.0, 5.0], 2).ok();
        let c = tree.insert([95.0, 95.0], 3).ok();
        assert!(a.is_some() && b.is_some() && c.is_some());
        assert_eq!(tree.len(), 3);
        assert_eq!(found(&tree, &Aabb::new([0.0, 0.0], [20.0, 10.0])), [1, 2]);
        assert_eq!(found(&tree, &Aabb::new([0.0, 0.0], [10.0, 10.0])), [1]);
        assert_eq!(found(&tree, &Aabb::new([40.0, 40.0], [60.0, 60.0])), []);
        assert_eq!(found(&tree, &world()), [1, 2, 3]);
        if let (Some(a), Some(b)) = (a, b) {
            assert_eq!(tree.get(a), Some(&1));
            assert_eq!(tree.position(a), Some([5.0, 5.0]));
            if let Some(value) = tree.get_mut(a) {
                *value = 10;
            }
            assert_eq!(tree.remove(b), Some(2));
            assert_eq!(tree.remove(b), None);
            assert_eq!(tree.get(b), None);
            assert_eq!(found(&tree, &world()), [3, 10]);
            assert!(tree.set_position(a, [90.0, 10.0]));
            assert_eq!(found(&tree, &Aabb::new([80.0, 0.0], [100.0, 20.0])), [10]);
            assert!(!tree.set_position(b, [1.0, 1.0]));
        }
        let mut near = Vec::new();
        tree.query_radius([95.0, 90.0], 5.0, |_, value| near.push(*value));
        assert_eq!(near, [3]);
    }
}

#[test]
fn splits_when_crowded_and_merges_when_emptied() {
    let mut nodes = Memory::<Node, 128>::new();
    let mut entries = Memory::<Slot<u32, 2>, 64>::new();
    let tree = Quadtree::with_limits(world(), 4, 8, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        let mut ids = Vec::new();
        for i in 0..40u32 {
            let position = [(i % 8) as f32 * 12.0 + 1.0, (i / 8) as f32 * 19.0 + 1.0];
            if let Ok(id) = tree.insert(position, i) {
                ids.push(id);
            }
        }
        assert_eq!(tree.len(), 40);
        assert!(tree.node_count() > 1);
        assert_eq!(found(&tree, &world()).len(), 40);
        for id in ids {
            assert!(tree.remove(id).is_some());
        }
        assert!(tree.is_empty());
        // Everything merged back into the root.
        assert_eq!(tree.node_count(), 1);
        // The freed nodes are reused.
        for i in 0..40u32 {
            assert!(tree.insert([(i * 2) as f32, 50.0], i).is_ok());
        }
        assert_eq!(found(&tree, &world()).len(), 40);
    }
}

#[test]
fn identical_points_do_not_split_forever() {
    let mut nodes = Memory::<Node, 256>::new();
    let mut entries = Memory::<Slot<u32, 2>, 32>::new();
    let tree = Quadtree::with_limits(world(), 2, 6, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        for i in 0..32 {
            assert!(tree.insert([33.0, 66.0], i).is_ok());
        }
        assert_eq!(found(&tree, &Aabb::point([33.0, 66.0])).len(), 32);
        assert!(tree.node_count() <= 1 + 6 * 4);
    }
}

#[test]
fn works_when_node_memory_runs_out() {
    // Room for the root and one split only.
    let mut nodes = Memory::<Node, 5>::new();
    let mut entries = Memory::<Slot<u32, 2>, 64>::new();
    let tree = Quadtree::with_limits(world(), 2, 8, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        for i in 0..64u32 {
            assert!(
                tree.insert([(i % 8) as f32 * 12.0, (i / 8) as f32 * 12.0], i)
                    .is_ok()
            );
        }
        assert_eq!(tree.node_count(), 5);
        assert_eq!(found(&tree, &world()).len(), 64);
        assert_eq!(
            found(&tree, &Aabb::new([0.0, 0.0], [12.0, 12.0])),
            [0, 1, 8, 9]
        );
    }
}

#[test]
fn positions_outside_the_bounds_still_work() {
    let mut nodes = Memory::<Node, 64>::new();
    let mut entries = Memory::<Slot<u32, 2>, 16>::new();
    let tree = Quadtree::with_limits(world(), 1, 8, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        assert!(tree.insert([-50.0, 500.0], 1).is_ok());
        assert!(tree.insert([100.0, 100.0], 2).is_ok());
        assert!(tree.insert([f32::NAN, 10.0], 3).is_ok());
        assert!(tree.insert([10.0, 10.0], 4).is_ok());
        assert_eq!(
            found(&tree, &Aabb::new([-60.0, 400.0], [-40.0, 600.0])),
            [1]
        );
        assert_eq!(found(&tree, &Aabb::new([99.0, 99.0], [101.0, 101.0])), [2]);
        // A NaN position is stored but never matches a query.
        assert_eq!(
            found(&tree, &Aabb::new([-1e9, -1e9], [1e9, 1e9])),
            [1, 2, 4]
        );
        assert_eq!(tree.len(), 4);
    }
}

#[test]
fn full_clear_and_drops() {
    let drops = Cell::new(0);
    let mut nodes = Memory::<Node, 16>::new();
    let mut entries = Memory::<Slot<Counted, 2>, 3>::new();
    let tree = Quadtree::with_limits(world(), 1, 4, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        for x in [1.0, 60.0, 99.0] {
            assert!(tree.insert([x, x], Counted(&drops)).is_ok());
        }
        assert!(tree.is_full());
        assert!(matches!(
            tree.insert([5.0, 5.0], Counted(&drops)),
            Err(Full(_))
        ));
        assert_eq!(drops.get(), 1);
        tree.clear();
        assert_eq!(drops.get(), 4);
        assert!(tree.is_empty());
        assert_eq!(tree.node_count(), 1);
        let mut count = 0;
        tree.query(&world(), |_, _| count += 1);
        assert_eq!(count, 0);
        assert!(tree.insert([1.0, 1.0], Counted(&drops)).is_ok());
    }
    assert_eq!(drops.get(), 5);
}

#[test]
fn octree_in_three_dimensions() {
    let mut nodes = Memory::<Node, 73>::new();
    let mut entries = Memory::<Slot<u32, 3>, 32>::new();
    let bounds = Aabb::new([0.0; 3], [32.0; 3]);
    let tree = Octree::with_limits(bounds, 2, 6, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    if let Ok(mut tree) = tree {
        for i in 0..27u32 {
            let position = [
                (i % 3) as f32 * 10.0 + 1.0,
                (i / 3 % 3) as f32 * 10.0 + 1.0,
                (i / 9) as f32 * 10.0 + 1.0,
            ];
            assert!(tree.insert(position, i).is_ok());
        }
        assert!(tree.node_count() > 1);
        let mut out = Vec::new();
        tree.query(
            &Aabb::new([0.0, 0.0, 20.0], [32.0, 32.0, 32.0]),
            |_, value| out.push(*value),
        );
        out.sort_unstable();
        assert!(out.iter().copied().eq(18..27));
        let mut near = Vec::new();
        tree.query_radius([1.0, 1.0, 1.0], 10.5, |_, value| near.push(*value));
        near.sort_unstable();
        assert_eq!(near, [0, 1, 3, 9]);
    }
}

#[test]
fn matches_brute_force() {
    const CAPACITY: usize = 64;
    let mut nodes = Memory::<Node, 101>::new();
    let mut entries = Memory::<Slot<usize, 2>, CAPACITY>::new();
    let tree = Orthtree::with_limits(world(), 3, 6, &mut nodes, &mut entries);
    assert!(tree.is_ok());
    let Ok(mut tree) = tree else {
        return;
    };
    let mut model: Vec<(EntryId<usize, 2>, [f32; 2], bool)> = Vec::new();
    let mut rng = Rng::new(4242);
    let steps = if cfg!(miri) { 120 } else { 4000 };
    // Clustered points, so the tree gets deep in places.
    let random_point = |rng: &mut Rng| {
        if rng.bool() {
            [rng.f32_range(-20.0, 120.0), rng.f32_range(-20.0, 120.0)]
        } else {
            [rng.f32_range(30.0, 34.0), rng.f32_range(60.0, 64.0)]
        }
    };
    for _ in 0..steps {
        let live: Vec<usize> = (0..model.len())
            .filter(|key| model.get(*key).is_some_and(|entry| entry.2))
            .collect();
        match (rng.u32_below(4), rng.pick(&live).copied()) {
            (0 | 1, _) | (_, None) => {
                let position = random_point(&mut rng);
                match tree.insert(position, model.len()) {
                    Ok(id) => model.push((id, position, true)),
                    Err(Full(_)) => assert_eq!(live.len(), CAPACITY),
                }
            }
            (2, Some(key)) => {
                if let Some(entry) = model.get_mut(key) {
                    assert_eq!(tree.remove(entry.0), Some(key));
                    entry.2 = false;
                }
            }
            (_, Some(key)) => {
                let position = random_point(&mut rng);
                if let Some(entry) = model.get_mut(key) {
                    assert!(tree.set_position(entry.0, position));
                    entry.1 = position;
                }
            }
        }

        let area = Aabb::new(random_point(&mut rng), random_point(&mut rng));
        let mut got = Vec::new();
        tree.query(&area, |_, value| got.push(*value));
        got.sort_unstable();
        let expected: Vec<usize> = (0..model.len())
            .filter(|key| {
                model
                    .get(*key)
                    .is_some_and(|entry| entry.2 && area.contains_point(&entry.1))
            })
            .collect();
        assert_eq!(got, expected);

        let center = random_point(&mut rng);
        let radius = rng.f32_range(0.0, 40.0);
        let mut got = Vec::new();
        tree.query_radius(center, radius, |_, value| got.push(*value));
        got.sort_unstable();
        let expected: Vec<usize> = (0..model.len())
            .filter(|key| {
                model.get(*key).is_some_and(|entry| {
                    let (dx, dy) = (entry.1[0] - center[0], entry.1[1] - center[1]);
                    entry.2 && dx * dx + dy * dy <= radius * radius
                })
            })
            .collect();
        assert_eq!(got, expected);
        assert_eq!(tree.len(), model.iter().filter(|entry| entry.2).count());
    }
    // Remove everything: the tree must shrink back to its root.
    for entry in &model {
        if entry.2 {
            assert!(tree.remove(entry.0).is_some());
        }
    }
    assert_eq!(tree.node_count(), 1);
}
