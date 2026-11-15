use core::cell::Cell;
use std::vec::Vec;
use vas::spatial_grid::{EntryId, NewError, Slot};
use vas::{Aabb, Full, Memory, Rng, SpatialGrid};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn world() -> Aabb<2> {
    Aabb::new([0.0, 0.0], [100.0, 100.0])
}

fn found(grid: &SpatialGrid<'_, u32, 2>, area: &Aabb<2>) -> Vec<u32> {
    let mut out = Vec::new();
    grid.query(area, |_, value| out.push(*value));
    out.sort_unstable();
    out
}

#[test]
fn new_checks_its_arguments() {
    let mut cells = Memory::<u32, 16>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let grid = SpatialGrid::<u32, 2>::new(world(), [4, 4], &mut cells, &mut entries);
    assert!(grid.is_ok_and(|grid| grid.is_empty() && grid.capacity() == 4));

    let mut cells = Memory::<u32, 15>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let grid = SpatialGrid::<u32, 2>::new(world(), [4, 4], &mut cells, &mut entries);
    assert_eq!(grid.err(), Some(NewError::Full));

    let mut cells = Memory::<u32, 16>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let grid = SpatialGrid::<u32, 2>::new(world(), [4, 0], &mut cells, &mut entries);
    assert_eq!(grid.err(), Some(NewError::Invalid));

    let mut cells = Memory::<u32, 16>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let flat = Aabb::new([0.0, 0.0], [100.0, 0.0]);
    let grid = SpatialGrid::<u32, 2>::new(flat, [4, 4], &mut cells, &mut entries);
    assert_eq!(grid.err(), Some(NewError::Invalid));

    let mut cells = Memory::<u32, 16>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let endless = Aabb::new([0.0, 0.0], [f32::INFINITY, 1.0]);
    let grid = SpatialGrid::<u32, 2>::new(endless, [4, 4], &mut cells, &mut entries);
    assert_eq!(grid.err(), Some(NewError::Invalid));
}

#[test]
fn insert_query_remove() {
    let mut cells = Memory::<u32, 100>::new();
    let mut entries = Memory::<Slot<u32, 2>, 8>::new();
    let grid = SpatialGrid::new(world(), [10, 10], &mut cells, &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        let a = grid.insert([5.0, 5.0], 1).ok();
        let b = grid.insert([15.0, 5.0], 2).ok();
        let c = grid.insert([95.0, 95.0], 3).ok();
        assert!(a.is_some() && b.is_some() && c.is_some());
        assert_eq!(grid.len(), 3);
        assert_eq!(found(&grid, &Aabb::new([0.0, 0.0], [20.0, 10.0])), [1, 2]);
        assert_eq!(found(&grid, &Aabb::new([0.0, 0.0], [10.0, 10.0])), [1]);
        assert_eq!(found(&grid, &Aabb::new([50.0, 50.0], [60.0, 60.0])), []);
        assert_eq!(found(&grid, &world()), [1, 2, 3]);
        // The filter is exact, not per cell.
        assert_eq!(found(&grid, &Aabb::new([6.0, 0.0], [9.0, 9.0])), []);
        if let (Some(a), Some(b)) = (a, b) {
            assert_eq!(grid.get(a), Some(&1));
            assert_eq!(grid.position(a), Some([5.0, 5.0]));
            if let Some(value) = grid.get_mut(a) {
                *value = 10;
            }
            assert_eq!(grid.remove(b), Some(2));
            assert_eq!(grid.remove(b), None);
            assert_eq!(grid.get(b), None);
            assert_eq!(grid.position(b), None);
            assert_eq!(found(&grid, &world()), [3, 10]);
        }
    }
}

#[test]
fn radius_query_is_exact() {
    let mut cells = Memory::<u32, 100>::new();
    let mut entries = Memory::<Slot<u32, 2>, 8>::new();
    let grid = SpatialGrid::new(world(), [10, 10], &mut cells, &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        assert!(grid.insert([50.0, 50.0], 1).is_ok());
        assert!(grid.insert([53.0, 54.0], 2).is_ok()); // distance 5
        assert!(grid.insert([54.0, 54.0], 3).is_ok()); // inside the box, outside the circle
        let mut out = Vec::new();
        grid.query_radius([50.0, 50.0], 5.0, |_, value| out.push(*value));
        out.sort_unstable();
        assert_eq!(out, [1, 2]);
        let mut none = 0;
        grid.query_radius([50.0, 50.0], -1.0, |_, _| none += 1);
        assert_eq!(none, 0);
    }
}

#[test]
fn positions_outside_the_bounds_still_work() {
    let mut cells = Memory::<u32, 16>::new();
    let mut entries = Memory::<Slot<u32, 2>, 8>::new();
    let grid = SpatialGrid::new(world(), [4, 4], &mut cells, &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        assert!(grid.insert([-50.0, 500.0], 1).is_ok());
        assert!(grid.insert([100.0, 100.0], 2).is_ok());
        assert!(grid.insert([f32::NAN, 10.0], 3).is_ok());
        assert_eq!(
            found(&grid, &Aabb::new([-60.0, 400.0], [-40.0, 600.0])),
            [1]
        );
        assert_eq!(found(&grid, &Aabb::new([99.0, 99.0], [101.0, 101.0])), [2]);
        // A NaN position is stored but never matches a query.
        assert_eq!(found(&grid, &Aabb::new([-1e9, -1e9], [1e9, 1e9])), [1, 2]);
        assert_eq!(grid.len(), 3);
    }
}

#[test]
fn set_position_moves_between_cells() {
    let mut cells = Memory::<u32, 16>::new();
    let mut entries = Memory::<Slot<u32, 2>, 4>::new();
    let grid = SpatialGrid::new(world(), [4, 4], &mut cells, &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        let id = grid.insert([10.0, 10.0], 7).ok();
        assert!(grid.insert([12.0, 12.0], 8).is_ok());
        assert!(id.is_some());
        if let Some(id) = id {
            assert!(grid.set_position(id, [90.0, 90.0]));
            assert_eq!(grid.position(id), Some([90.0, 90.0]));
            assert_eq!(found(&grid, &Aabb::new([0.0, 0.0], [25.0, 25.0])), [8]);
            assert_eq!(found(&grid, &Aabb::new([75.0, 75.0], [100.0, 100.0])), [7]);
            // Moving within the same cell.
            assert!(grid.set_position(id, [91.0, 91.0]));
            assert_eq!(found(&grid, &Aabb::new([90.5, 90.5], [100.0, 100.0])), [7]);
            assert_eq!(grid.remove(id), Some(7));
            assert!(!grid.set_position(id, [1.0, 1.0]));
        }
    }
}

#[test]
fn full_clear_and_drops() {
    let drops = Cell::new(0);
    let mut cells = Memory::<u32, 4>::new();
    let mut entries = Memory::<Slot<Counted, 2>, 2>::new();
    let grid = SpatialGrid::new(world(), [2, 2], &mut cells, &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        assert!(grid.insert([1.0, 1.0], Counted(&drops)).is_ok());
        assert!(grid.insert([99.0, 99.0], Counted(&drops)).is_ok());
        assert!(grid.is_full());
        assert!(matches!(
            grid.insert([5.0, 5.0], Counted(&drops)),
            Err(Full(_))
        ));
        assert_eq!(drops.get(), 1);
        grid.clear();
        assert_eq!(drops.get(), 3);
        assert!(grid.is_empty());
        let mut count = 0;
        grid.query(&world(), |_, _| count += 1);
        assert_eq!(count, 0);
        assert!(grid.insert([1.0, 1.0], Counted(&drops)).is_ok());
    }
    assert_eq!(drops.get(), 4);
}

#[test]
fn three_dimensions() {
    let mut cells = Memory::<u32, 27>::new();
    let mut entries = Memory::<Slot<u32, 3>, 8>::new();
    let bounds = Aabb::new([0.0; 3], [30.0; 3]);
    let grid = SpatialGrid::new(bounds, [3, 3, 3], &mut cells, &mut entries);
    assert!(grid.is_ok());
    if let Ok(mut grid) = grid {
        assert!(grid.insert([5.0, 5.0, 5.0], 1).is_ok());
        assert!(grid.insert([5.0, 5.0, 25.0], 2).is_ok());
        assert!(grid.insert([25.0, 25.0, 25.0], 3).is_ok());
        let mut out = Vec::new();
        grid.query(
            &Aabb::new([0.0, 0.0, 20.0], [30.0, 30.0, 30.0]),
            |_, value| out.push(*value),
        );
        out.sort_unstable();
        assert_eq!(out, [2, 3]);
        let mut near = Vec::new();
        grid.query_radius([5.0, 5.0, 8.0], 4.0, |_, value| near.push(*value));
        assert_eq!(near, [1]);
    }
}

#[test]
fn matches_brute_force() {
    const CAPACITY: usize = 64;
    let mut cells = Memory::<u32, 64>::new();
    let mut entries = Memory::<Slot<usize, 2>, CAPACITY>::new();
    let grid = SpatialGrid::new(world(), [8, 8], &mut cells, &mut entries);
    assert!(grid.is_ok());
    let Ok(mut grid) = grid else {
        return;
    };
    // Model: (id, position, alive), value = index.
    let mut model: Vec<(EntryId<usize, 2>, [f32; 2], bool)> = Vec::new();
    let mut rng = Rng::new(9001);
    let steps = if cfg!(miri) { 120 } else { 3000 };
    let mut random_point =
        |rng: &mut Rng| [rng.f32_range(-20.0, 120.0), rng.f32_range(-20.0, 120.0)];
    for _ in 0..steps {
        let live: Vec<usize> = (0..model.len())
            .filter(|key| model.get(*key).is_some_and(|entry| entry.2))
            .collect();
        match (rng.u32_below(4), rng.pick(&live).copied()) {
            (0 | 1, _) | (_, None) => {
                let position = random_point(&mut rng);
                match grid.insert(position, model.len()) {
                    Ok(id) => model.push((id, position, true)),
                    Err(Full(_)) => assert_eq!(live.len(), CAPACITY),
                }
            }
            (2, Some(key)) => {
                if let Some(entry) = model.get_mut(key) {
                    assert_eq!(grid.remove(entry.0), Some(key));
                    entry.2 = false;
                }
            }
            (_, Some(key)) => {
                let position = random_point(&mut rng);
                if let Some(entry) = model.get_mut(key) {
                    assert!(grid.set_position(entry.0, position));
                    entry.1 = position;
                }
            }
        }

        let area = Aabb::new(random_point(&mut rng), random_point(&mut rng));
        let mut got = Vec::new();
        grid.query(&area, |_, value| got.push(*value));
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
        grid.query_radius(center, radius, |_, value| got.push(*value));
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
        assert_eq!(grid.len(), live_count(&model));
    }
}

fn live_count(model: &[(EntryId<usize, 2>, [f32; 2], bool)]) -> usize {
    model.iter().filter(|entry| entry.2).count()
}
