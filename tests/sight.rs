use std::collections::BTreeSet;
use std::error::Error;
use std::vec::Vec;
use vas::grid::line;
use vas::{Full, Grid, Inline, Memory, Rng};

type Test = Result<(), Box<dyn Error>>;
type Cells = BTreeSet<(u32, u32)>;

/// Parses rows of `.` (floor) and `#` (wall); `true` is a wall.
fn level<const N: usize>(rows: &[&str]) -> Result<Grid<bool, Inline<N>>, Full<()>> {
    let width = rows.first().map_or(0, |row| row.len()) as u32;
    Grid::from_fn(Memory::new(), width, rows.len() as u32, |x, y| {
        rows[y as usize].as_bytes()[x as usize] == b'#'
    })
}

fn random_level<const N: usize>(
    rng: &mut Rng,
    width: u32,
    height: u32,
) -> Result<Grid<bool, Inline<N>>, Full<()>> {
    Grid::from_fn(Memory::new(), width, height, |_, _| rng.u32_below(4) == 0)
}

fn seen<const N: usize>(
    grid: &Grid<bool, Inline<N>>,
    x: u32,
    y: u32,
    radius: u32,
) -> (Cells, usize) {
    let mut cells = Cells::new();
    let mut calls = 0;
    grid.field_of_view(
        x,
        y,
        radius,
        |wall| *wall,
        |x, y| {
            cells.insert((x, y));
            calls += 1;
        },
    );
    (cells, calls)
}

#[test]
fn line_goes_from_start_to_end_one_step_at_a_time() {
    let mut rng = Rng::new(7);
    for _ in 0..if cfg!(miri) { 100 } else { 2000 } {
        let from = (rng.u32_below(40), rng.u32_below(40));
        let to = (rng.u32_below(40), rng.u32_below(40));
        let cells: Vec<_> = line(from, to).collect();
        let steps = from.0.abs_diff(to.0).max(from.1.abs_diff(to.1));
        assert_eq!(cells.len(), steps as usize + 1);
        assert_eq!(line(from, to).size_hint(), (cells.len(), Some(cells.len())));
        assert_eq!(cells.first(), Some(&from));
        assert_eq!(cells.last(), Some(&to));
        for pair in cells.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(a.0.abs_diff(b.0) <= 1 && a.1.abs_diff(b.1) <= 1 && a != b);
        }
    }
}

#[test]
fn line_is_the_same_both_ways() {
    let mut rng = Rng::new(11);
    for _ in 0..if cfg!(miri) { 100 } else { 2000 } {
        let from = (rng.u32_below(40), rng.u32_below(40));
        let to = (rng.u32_below(40), rng.u32_below(40));
        let forward: Vec<_> = line(from, to).collect();
        let mut back: Vec<_> = line(to, from).collect();
        back.reverse();
        assert_eq!(forward, back, "{from:?} -> {to:?}");
    }
}

#[test]
fn line_shapes() {
    assert!(line((3, 3), (3, 3)).eq([(3, 3)]));
    assert!(line((0, 0), (3, 0)).eq([(0, 0), (1, 0), (2, 0), (3, 0)]));
    assert!(line((0, 2), (0, 0)).eq([(0, 2), (0, 1), (0, 0)]));
    assert!(line((0, 0), (2, 2)).eq([(0, 0), (1, 1), (2, 2)]));
    assert!(line((0, 0), (4, 2)).eq([(0, 0), (1, 0), (2, 1), (3, 1), (4, 2)]));
    // The far corners of the u32 range do not overflow.
    let size = line((0, 0), (u32::MAX, u32::MAX)).size_hint();
    assert_eq!(size, (1 << 32, Some(1 << 32)));
    let max = u32::MAX;
    assert!(line((max, 0), (max - 2, 1)).eq([(max, 0), (max - 1, 0), (max - 2, 1)]));
    assert!(line((0, max), (1, max - 2)).eq([(0, max), (0, max - 1), (1, max - 2)]));
}

#[test]
fn line_of_sight_ignores_the_end_cells() -> Test {
    let grid = level::<25>(&[
        ".....", //
        "..#..", //
        ".....", //
        "#....", //
        ".....",
    ])?;
    let wall = |wall: &bool| *wall;
    assert!(grid.line_of_sight((0, 0), (4, 0), wall));
    assert!(!grid.line_of_sight((0, 1), (4, 1), wall));
    assert!(!grid.line_of_sight((2, 0), (2, 4), wall));
    // A wall at either end can be seen.
    assert!(grid.line_of_sight((2, 1), (4, 3), wall));
    assert!(grid.line_of_sight((4, 3), (0, 3), wall));
    assert!(grid.line_of_sight((1, 1), (1, 1), wall));
    // Outside the grid nothing is in sight.
    assert!(!grid.line_of_sight((0, 0), (5, 0), wall));
    Ok(())
}

#[test]
fn line_of_sight_is_symmetric() -> Test {
    let mut rng = Rng::new(3);
    let grid = random_level::<400>(&mut rng, 20, 20)?;
    for _ in 0..if cfg!(miri) { 200 } else { 3000 } {
        let a = (rng.u32_below(20), rng.u32_below(20));
        let b = (rng.u32_below(20), rng.u32_below(20));
        let wall = |wall: &bool| *wall;
        assert_eq!(
            grid.line_of_sight(a, b, wall),
            grid.line_of_sight(b, a, wall)
        );
    }
    Ok(())
}

#[test]
fn open_field_is_a_disc() -> Test {
    let grid = Grid::<bool, Inline<441>>::new(Memory::new(), 21, 21, false)?;
    for radius in [0, 1, 2, 5, 10] {
        let (cells, calls) = seen(&grid, 10, 10, radius);
        assert_eq!(calls, cells.len(), "each cell once");
        let disc: Cells = grid
            .iter()
            .map(|(x, y, _)| (x, y))
            .filter(|&(x, y)| {
                let (dx, dy) = (u64::from(x.abs_diff(10)), u64::from(y.abs_diff(10)));
                dx * dx + dy * dy <= u64::from(radius) * u64::from(radius)
            })
            .collect();
        assert_eq!(cells, disc, "radius {radius}");
    }
    Ok(())
}

#[test]
fn walls_cast_shadows() -> Test {
    let grid = level::<49>(&[
        ".......", //
        ".......", //
        ".......", //
        "...@#..", //
        ".......", //
        ".......", //
        ".......",
    ])?;
    let (cells, _) = seen(&grid, 3, 3, 10);
    assert!(cells.contains(&(3, 3)));
    // The wall is seen, the cells right behind it are not.
    assert!(cells.contains(&(4, 3)));
    assert!(!cells.contains(&(5, 3)));
    assert!(!cells.contains(&(6, 3)));
    // Cells off to the side are.
    assert!(cells.contains(&(6, 0)));
    assert!(cells.contains(&(0, 3)));
    Ok(())
}

#[test]
fn a_closed_room_shows_only_itself() -> Test {
    let grid = level::<49>(&[
        "#####..", //
        "#...#..", //
        "#...#..", //
        "#...#..", //
        "#####..", //
        ".......", //
        ".......",
    ])?;
    let (cells, calls) = seen(&grid, 2, 2, 100);
    assert_eq!(calls, cells.len());
    let room: Cells = (0..5).flat_map(|x| (0..5).map(move |y| (x, y))).collect();
    assert_eq!(cells, room);
    Ok(())
}

#[test]
fn field_of_view_is_symmetric_between_floor_cells() -> Test {
    let mut rng = Rng::new(5);
    let count = if cfg!(miri) { 1 } else { 20 };
    for _ in 0..count {
        let size = if cfg!(miri) { 8 } else { 16 };
        let grid = random_level::<256>(&mut rng, size, size)?;
        let floors: Vec<_> = grid
            .iter()
            .filter(|(_, _, w)| !**w)
            .map(|(x, y, _)| (x, y))
            .collect();
        let mut views = Vec::new();
        for &(x, y) in &floors {
            let (cells, calls) = seen(&grid, x, y, 100);
            assert_eq!(calls, cells.len(), "each cell once");
            views.push(cells);
        }
        for (a, view_a) in floors.iter().zip(&views) {
            for (b, view_b) in floors.iter().zip(&views) {
                assert_eq!(view_a.contains(b), view_b.contains(a), "{a:?} {b:?}");
            }
        }
    }
    Ok(())
}

#[test]
fn origin_outside_sees_nothing() -> Test {
    let grid = Grid::<bool, Inline<9>>::new(Memory::new(), 3, 3, false)?;
    assert_eq!(seen(&grid, 3, 0, 5).1, 0);
    assert_eq!(seen(&grid, u32::MAX, u32::MAX, u32::MAX).1, 0);
    Ok(())
}

#[test]
fn huge_radius_is_clipped_to_the_grid() -> Test {
    let grid = Grid::<bool, Inline<12>>::new(Memory::new(), 4, 3, false)?;
    let (cells, calls) = seen(&grid, 0, 0, u32::MAX);
    assert_eq!((cells.len(), calls), (12, 12));
    Ok(())
}
