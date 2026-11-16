use std::vec::Vec;
use vas::bvh::{BuildError, Node};
use vas::{Aabb, Bvh, Memory, Rng};

type Item = (Aabb<2>, u32);

fn random_items(rng: &mut Rng, count: usize) -> Vec<Item> {
    (0..count)
        .map(|index| {
            let corner = [rng.f32_range(0.0, 100.0), rng.f32_range(0.0, 100.0)];
            let size = [rng.f32_range(0.0, 8.0), rng.f32_range(0.0, 8.0)];
            let aabb = Aabb::new(corner, [corner[0] + size[0], corner[1] + size[1]]);
            (aabb, index as u32)
        })
        .collect()
}

fn sorted(mut values: Vec<u32>) -> Vec<u32> {
    values.sort_unstable();
    values
}

#[test]
fn empty_tree() {
    let mut items: [Item; 0] = [];
    let mut nodes = Memory::<Node<2>, 0>::new();
    let bvh = Bvh::build(&mut items, &mut nodes);
    assert!(bvh.is_ok());
    if let Ok(bvh) = bvh {
        assert!(bvh.is_empty());
        assert_eq!(bvh.len(), 0);
        assert_eq!(bvh.node_count(), 0);
        assert_eq!(bvh.bounds(), None);
        let mut count = 0;
        bvh.query(&Aabb::new([0.0, 0.0], [1.0, 1.0]), |_, _| count += 1);
        bvh.query_point(&[0.0, 0.0], |_, _| count += 1);
        bvh.query_ray(&[0.0, 0.0], &[1.0, 0.0], 10.0, |_, _, _| count += 1);
        assert_eq!(count, 0);
    }
}

#[test]
fn needs_at_least_one_node() {
    let mut items = [(Aabb::new([0.0, 0.0], [1.0, 1.0]), 1u32)];
    let mut nodes = Memory::<Node<2>, 0>::new();
    assert_eq!(
        Bvh::build(&mut items, &mut nodes).err(),
        Some(BuildError::Full)
    );
}

#[test]
fn small_tree_queries() {
    let mut items = [
        (Aabb::new([0.0, 0.0], [1.0, 1.0]), 1u32),
        (Aabb::new([5.0, 5.0], [6.0, 6.0]), 2),
        (Aabb::new([0.5, 0.5], [5.5, 5.5]), 3),
        (Aabb::new([9.0, 0.0], [10.0, 1.0]), 4),
    ];
    let mut nodes = Memory::<Node<2>, 8>::new();
    let bvh = Bvh::build_with(&mut items, &mut nodes, 1);
    assert!(bvh.is_ok());
    if let Ok(bvh) = bvh {
        assert_eq!(bvh.len(), 4);
        assert_eq!(bvh.node_count(), 7);
        assert_eq!(bvh.bounds(), Some(Aabb::new([0.0, 0.0], [10.0, 6.0])));

        let mut hits = Vec::new();
        bvh.query(&Aabb::new([0.9, 0.9], [2.0, 2.0]), |_, value| {
            hits.push(*value)
        });
        assert_eq!(sorted(hits), [1, 3]);

        let mut hits = Vec::new();
        bvh.query_point(&[5.2, 5.2], |_, value| hits.push(*value));
        assert_eq!(sorted(hits), [2, 3]);

        // A ray along the bottom edge hits 1, then 4.
        let mut hits = Vec::new();
        bvh.query_ray(&[-1.0, 0.25], &[1.0, 0.0], 100.0, |_, value, distance| {
            hits.push((*value, distance))
        });
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(hits, [(1, 1.0), (4, 10.0)]);
        // Cut short, it only reaches the first.
        let mut hits = Vec::new();
        bvh.query_ray(&[-1.0, 0.25], &[1.0, 0.0], 5.0, |_, value, _| {
            hits.push(*value)
        });
        assert_eq!(hits, [1]);
    }
}

#[test]
fn works_when_node_memory_is_short() {
    let mut rng = Rng::new(8);
    let mut items = random_items(&mut rng, 50);
    // One node: the whole set becomes a single leaf.
    let mut nodes = Memory::<Node<2>, 1>::new();
    let bvh = Bvh::build_with(&mut items, &mut nodes, 1);
    assert!(bvh.is_ok());
    if let Ok(bvh) = bvh {
        assert_eq!(bvh.node_count(), 1);
        let mut count = 0;
        bvh.query(&Aabb::new([-1.0, -1.0], [200.0, 200.0]), |_, _| count += 1);
        assert_eq!(count, 50);
    }
}

#[test]
fn refit_follows_moved_items() {
    let mut items = [
        (Aabb::new([0.0, 0.0], [1.0, 1.0]), 1u32),
        (Aabb::new([5.0, 5.0], [6.0, 6.0]), 2),
        (Aabb::new([9.0, 9.0], [10.0, 10.0]), 3),
    ];
    let mut nodes = Memory::<Node<2>, 8>::new();
    let bvh = Bvh::build_with(&mut items, &mut nodes, 1);
    assert!(bvh.is_ok());
    if let Ok(mut bvh) = bvh {
        for (aabb, value) in bvh.items_mut() {
            if *value == 2 {
                *aabb = Aabb::new([50.0, 50.0], [51.0, 51.0]);
            }
        }
        bvh.refit();
        assert_eq!(bvh.bounds(), Some(Aabb::new([0.0, 0.0], [51.0, 51.0])));
        let mut hits = Vec::new();
        bvh.query_point(&[50.5, 50.5], |_, value| hits.push(*value));
        assert_eq!(hits, [2]);
        let mut hits = Vec::new();
        bvh.query_point(&[5.5, 5.5], |_, value| hits.push(*value));
        assert_eq!(hits, []);
    }
}

#[test]
fn three_dimensions() {
    let mut items = [
        (Aabb::new([0.0; 3], [1.0; 3]), 1u32),
        (Aabb::new([4.0; 3], [5.0; 3]), 2),
        (Aabb::new([0.0, 0.0, 8.0], [1.0, 1.0, 9.0]), 3),
    ];
    let mut nodes = Memory::<Node<3>, 8>::new();
    let bvh = Bvh::build_with(&mut items, &mut nodes, 1);
    assert!(bvh.is_ok());
    if let Ok(bvh) = bvh {
        let mut hits = Vec::new();
        bvh.query_ray(&[0.5, 0.5, -5.0], &[0.0, 0.0, 1.0], 100.0, |_, value, _| {
            hits.push(*value)
        });
        assert_eq!(sorted(hits), [1, 3]);
    }
}

#[test]
fn matches_brute_force() {
    let mut rng = Rng::new(2468);
    let rounds = if cfg!(miri) { 3 } else { 60 };
    for round in 0..rounds {
        let count = rng.usize_below(120);
        let mut items = random_items(&mut rng, count);
        let reference = items.clone();
        let mut nodes = Memory::<Node<2>, 256>::new();
        let leaf_size = 1 + round % 5;
        let bvh = Bvh::build_with(&mut items, &mut nodes, leaf_size);
        assert!(bvh.is_ok());
        let Ok(mut bvh) = bvh else {
            return;
        };
        assert_eq!(bvh.len(), count);
        // Building reorders the items but keeps all of them.
        assert_eq!(
            sorted(bvh.items().iter().map(|item| item.1).collect()),
            (0..count as u32).collect::<Vec<_>>()
        );

        for pass in 0..2 {
            let current: Vec<Item> = if pass == 0 {
                reference.clone()
            } else {
                // Move everything, then refit instead of rebuilding.
                for (aabb, _) in bvh.items_mut() {
                    let shift = [rng.f32_range(-20.0, 20.0), rng.f32_range(-20.0, 20.0)];
                    *aabb = Aabb::new(
                        [aabb.min[0] + shift[0], aabb.min[1] + shift[1]],
                        [aabb.max[0] + shift[0], aabb.max[1] + shift[1]],
                    );
                }
                bvh.refit();
                bvh.items().to_vec()
            };
            for _ in 0..10 {
                let a = [rng.f32_range(-20.0, 120.0), rng.f32_range(-20.0, 120.0)];
                let b = [rng.f32_range(-20.0, 120.0), rng.f32_range(-20.0, 120.0)];
                let area = Aabb::new(a, b);
                let mut got = Vec::new();
                bvh.query(&area, |_, value| got.push(*value));
                let expected = current
                    .iter()
                    .filter(|item| item.0.intersects(&area))
                    .map(|item| item.1);
                assert_eq!(sorted(got), sorted(expected.collect()));

                let mut got = Vec::new();
                bvh.query_point(&a, |_, value| got.push(*value));
                let expected = current
                    .iter()
                    .filter(|item| item.0.contains_point(&a))
                    .map(|item| item.1);
                assert_eq!(sorted(got), sorted(expected.collect()));

                let direction = [b[0] - a[0], b[1] - a[1]];
                let reach = rng.f32_range(0.0, 2.0);
                let mut got = Vec::new();
                bvh.query_ray(&a, &direction, reach, |aabb, value, distance| {
                    assert_eq!(aabb.ray(&a, &direction, reach), Some(distance));
                    got.push(*value);
                });
                let expected = current
                    .iter()
                    .filter(|item| item.0.ray(&a, &direction, reach).is_some())
                    .map(|item| item.1);
                assert_eq!(sorted(got), sorted(expected.collect()));
            }
        }
    }
}
