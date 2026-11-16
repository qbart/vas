use std::vec::Vec;
use vas::kd_tree::Neighbor;
use vas::{Aabb, KdTree, Memory, Rng, Vector};

type Item = ([f32; 2], u32);

fn distance_squared(a: &[f32; 2], b: &[f32; 2]) -> f32 {
    (a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])
}

fn sorted(mut values: Vec<u32>) -> Vec<u32> {
    values.sort_unstable();
    values
}

#[test]
fn empty_tree() {
    let mut items: [Item; 0] = [];
    let tree = KdTree::build(&mut items);
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);
    assert_eq!(tree.nearest(&[0.0, 0.0]), None);
    let mut memory = Memory::<Neighbor, 4>::new();
    let mut out = Vector::new(&mut memory);
    tree.nearest_k(&[0.0, 0.0], &mut out);
    assert!(out.is_empty());
    let mut count = 0;
    tree.query(&Aabb::new([0.0, 0.0], [1.0, 1.0]), |_, _| count += 1);
    tree.query_radius(&[0.0, 0.0], 5.0, |_, _| count += 1);
    assert_eq!(count, 0);
}

#[test]
fn nearest_neighbour() {
    let mut items = [
        ([0.0, 0.0], 1u32),
        ([10.0, 0.0], 2),
        ([0.0, 10.0], 3),
        ([10.0, 10.0], 4),
        ([5.0, 5.0], 5),
    ];
    let mut tree = KdTree::build(&mut items);
    assert_eq!(tree.len(), 5);
    let nearest = tree.nearest(&[6.0, 6.0]);
    assert_eq!(nearest.map(|n| n.distance_squared), Some(2.0));
    assert_eq!(
        nearest.and_then(|n| tree.get(n.index)).map(|item| *item.1),
        Some(5)
    );
    let nearest = tree.nearest(&[9.0, 1.0]);
    assert_eq!(
        nearest.and_then(|n| tree.get(n.index)).map(|item| *item.1),
        Some(2)
    );
    // An exact hit has distance zero.
    assert_eq!(
        tree.nearest(&[0.0, 10.0]).map(|n| n.distance_squared),
        Some(0.0)
    );
    assert_eq!(tree.get(99), None);
    if let Some(index) = nearest.map(|n| n.index) {
        if let Some(value) = tree.get_mut(index) {
            *value = 20;
        }
        assert_eq!(tree.get(index).map(|item| *item.1), Some(20));
    }
}

#[test]
fn k_nearest_are_sorted_by_distance() {
    let mut items: Vec<Item> = (0..10).map(|i| ([i as f32, 0.0], i)).collect();
    let tree = KdTree::build(&mut items);
    let mut memory = Memory::<Neighbor, 3>::new();
    let mut out = Vector::new(&mut memory);
    tree.nearest_k(&[4.2, 0.0], &mut out);
    let values: Vec<u32> = out
        .iter()
        .filter_map(|n| tree.get(n.index))
        .map(|item| *item.1)
        .collect();
    assert_eq!(values, [4, 5, 3]);
    assert!(out.windows(2).all(|pair| match pair {
        [a, b] => a.distance_squared <= b.distance_squared,
        _ => true,
    }));
    // Asking for more than there are returns them all; old contents are
    // replaced.
    let mut memory = Memory::<Neighbor, 32>::new();
    let mut out = Vector::new(&mut memory);
    assert!(
        out.push(Neighbor {
            index: 99,
            distance_squared: 0.0
        })
        .is_ok()
    );
    tree.nearest_k(&[0.0, 0.0], &mut out);
    assert_eq!(out.len(), 10);
    assert_eq!(out.first().map(|n| n.distance_squared), Some(0.0));
    // No room, no results.
    let mut memory = Memory::<Neighbor, 0>::new();
    let mut out = Vector::new(&mut memory);
    tree.nearest_k(&[0.0, 0.0], &mut out);
    assert!(out.is_empty());
}

#[test]
fn box_and_radius_queries() {
    let mut items: Vec<Item> = (0..25)
        .map(|i| ([(i % 5) as f32, (i / 5) as f32], i))
        .collect();
    let tree = KdTree::build(&mut items);
    let mut got = Vec::new();
    tree.query(&Aabb::new([1.0, 1.0], [2.0, 3.0]), |_, value| {
        got.push(*value)
    });
    assert_eq!(sorted(got), [6, 7, 11, 12, 16, 17]);
    let mut got = Vec::new();
    tree.query_radius(&[2.0, 2.0], 1.0, |position, value| {
        assert!(distance_squared(position, &[2.0, 2.0]) <= 1.0);
        got.push(*value);
    });
    assert_eq!(sorted(got), [7, 11, 12, 13, 17]);
    let mut none = 0;
    tree.query_radius(&[2.0, 2.0], -1.0, |_, _| none += 1);
    assert_eq!(none, 0);
}

#[test]
fn duplicates_and_three_dimensions() {
    let mut items = [
        ([1.0, 1.0], 1u32),
        ([1.0, 1.0], 2),
        ([1.0, 1.0], 3),
        ([1.0, 1.0], 4),
    ];
    let tree = KdTree::build(&mut items);
    let mut got = Vec::new();
    tree.query(&Aabb::point([1.0, 1.0]), |_, value| got.push(*value));
    assert_eq!(sorted(got), [1, 2, 3, 4]);
    assert_eq!(
        tree.nearest(&[0.0, 1.0]).map(|n| n.distance_squared),
        Some(1.0)
    );

    let mut items = [
        ([0.0, 0.0, 0.0], 1u32),
        ([0.0, 0.0, 5.0], 2),
        ([3.0, 4.0, 0.0], 3),
    ];
    let tree = KdTree::build(&mut items);
    let nearest = tree.nearest(&[0.0, 0.0, 4.0]);
    assert_eq!(
        nearest.and_then(|n| tree.get(n.index)).map(|item| *item.1),
        Some(2)
    );
    assert_eq!(
        tree.nearest(&[3.0, 4.0, 1.0]).map(|n| n.distance_squared),
        Some(1.0)
    );
}

#[test]
fn nan_positions_are_never_returned_and_hide_nothing() {
    let mut items = [
        ([f32::NAN, 0.0], 100u32),
        ([1.0, 1.0], 1),
        ([0.0, f32::NAN], 101),
        ([2.0, 2.0], 2),
        ([f32::NAN, f32::NAN], 102),
        ([3.0, 3.0], 3),
        ([-1.0, -1.0], 4),
    ];
    let tree = KdTree::build(&mut items);
    for (target, expected) in [
        ([1.1, 1.0], 1),
        ([2.5, 2.4], 2),
        ([9.0, 9.0], 3),
        ([-5.0, 0.0], 4),
    ] {
        let nearest = tree.nearest(&target);
        assert_eq!(
            nearest.and_then(|n| tree.get(n.index)).map(|item| *item.1),
            Some(expected)
        );
    }
    let mut got = Vec::new();
    tree.query(&Aabb::new([-1e9, -1e9], [1e9, 1e9]), |_, value| {
        got.push(*value)
    });
    assert_eq!(sorted(got), [1, 2, 3, 4]);
    let mut memory = Memory::<Neighbor, 8>::new();
    let mut out = Vector::new(&mut memory);
    tree.nearest_k(&[0.0, 0.0], &mut out);
    assert_eq!(out.len(), 4);
    assert_eq!(tree.nearest(&[f32::NAN, 0.0]), None);
}

#[test]
fn matches_brute_force() {
    let mut rng = Rng::new(1357);
    let rounds = if cfg!(miri) { 3 } else { 80 };
    for _ in 0..rounds {
        let count = rng.usize_below(150);
        let mut items: Vec<Item> = (0..count)
            .map(|index| {
                // Half spread out, half clustered, with some exact duplicates.
                let position = if rng.bool() {
                    [rng.f32_range(0.0, 100.0), rng.f32_range(0.0, 100.0)]
                } else {
                    [rng.i32_range(40, 44) as f32, rng.i32_range(40, 44) as f32]
                };
                (position, index as u32)
            })
            .collect();
        let reference = items.clone();
        let tree = KdTree::build(&mut items);
        assert_eq!(
            sorted(tree.items().iter().map(|item| item.1).collect()),
            (0..count as u32).collect::<Vec<_>>()
        );

        for _ in 0..20 {
            let target = [rng.f32_range(-20.0, 120.0), rng.f32_range(-20.0, 120.0)];
            let mut distances: Vec<f32> = reference
                .iter()
                .map(|item| distance_squared(&item.0, &target))
                .collect();
            distances.sort_by(f32::total_cmp);

            // Ties can pick a different item, so compare distances.
            let nearest = tree.nearest(&target);
            assert_eq!(
                nearest.map(|n| n.distance_squared),
                distances.first().copied()
            );
            if let Some(found) = nearest {
                let position = tree.get(found.index).map(|item| *item.0);
                assert_eq!(
                    position.map(|p| distance_squared(&p, &target)),
                    Some(found.distance_squared)
                );
            }

            let mut memory = Memory::<Neighbor, 7>::new();
            let mut out = Vector::new(&mut memory);
            tree.nearest_k(&target, &mut out);
            let got: Vec<f32> = out.iter().map(|n| n.distance_squared).collect();
            let expected: Vec<f32> = distances.iter().copied().take(7).collect();
            assert_eq!(got, expected);

            let other = [rng.f32_range(-20.0, 120.0), rng.f32_range(-20.0, 120.0)];
            let area = Aabb::new(target, other);
            let mut got = Vec::new();
            tree.query(&area, |_, value| got.push(*value));
            let expected = reference
                .iter()
                .filter(|item| area.contains_point(&item.0))
                .map(|item| item.1);
            assert_eq!(sorted(got), sorted(expected.collect()));

            let radius = rng.f32_range(0.0, 30.0);
            let mut got = Vec::new();
            tree.query_radius(&target, radius, |_, value| got.push(*value));
            let expected = reference
                .iter()
                .filter(|item| distance_squared(&item.0, &target) <= radius * radius)
                .map(|item| item.1);
            assert_eq!(sorted(got), sorted(expected.collect()));
        }
    }
}
