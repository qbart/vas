use core::cell::Cell;
use std::error::Error;
use std::vec::Vec;
use vas::grid::Moves;
use vas::path::{Node, Open};
use vas::traversal::Mark;
use vas::{Arena, Dynamic, Full, Grid, Inline, Memory, PathFinder, Traversal};

type Test = Result<(), Box<dyn Error>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tile {
    Floor,
    Wall,
    Mud,
}

/// Parses rows of `.` (floor), `#` (wall) and `~` (mud) into a grid.
fn level<const N: usize>(rows: &[&str]) -> Result<Grid<Tile, Inline<N>>, Full<()>> {
    let width = rows.first().map_or(0, |row| row.len()) as u32;
    let height = rows.len() as u32;
    Grid::from_fn(Memory::new(), width, height, |x, y| {
        match rows[y as usize].as_bytes()[x as usize] {
            b'#' => Tile::Wall,
            b'~' => Tile::Mud,
            _ => Tile::Floor,
        }
    })
}

fn walk(_: &Tile, to: &Tile) -> Option<f32> {
    match to {
        Tile::Floor => Some(1.0),
        Tile::Mud => Some(5.0),
        Tile::Wall => None,
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn new_fills_every_cell() -> Test {
    let mut memory = Memory::<u8, 12>::new();
    let grid = Grid::new(&mut memory, 4, 3, 7)?;
    assert_eq!((grid.width(), grid.height(), grid.len()), (4, 3, 12));
    assert!(!grid.is_empty());
    assert!(grid.as_slice().iter().all(|cell| *cell == 7));
    Ok(())
}

#[test]
fn new_reports_too_little_memory() {
    let mut memory = Memory::<u8, 11>::new();
    assert_eq!(Grid::new(&mut memory, 4, 3, 0).err(), Some(Full(())));
    let mut memory = Memory::<u8, 11>::new();
    assert_eq!(
        Grid::from_fn(&mut memory, 4, 3, |_, _| 0).err(),
        Some(Full(()))
    );
    // The cell count must fit a u32 node index.
    let mut memory = Memory::<u8, 1>::new();
    assert_eq!(Grid::new(&mut memory, u32::MAX, 2, 0).err(), Some(Full(())));
}

#[test]
fn empty_grids_are_allowed() -> Test {
    let grid = Grid::<u8, Inline<0>>::new(Memory::new(), 0, 5, 0)?;
    assert!(grid.is_empty());
    assert_eq!(grid.get(0, 0), None);
    assert_eq!(grid.rows().count(), 0);
    Ok(())
}

#[test]
fn cells_are_row_major() -> Test {
    let mut grid = Grid::<u32, Inline<6>>::from_fn(Memory::new(), 3, 2, |x, y| y * 10 + x)?;
    assert_eq!(grid.as_slice(), &[0, 1, 2, 10, 11, 12]);
    assert_eq!(grid.get(2, 1), Some(&12));
    assert_eq!(grid.get(3, 0), None);
    assert_eq!(grid.get(0, 2), None);
    assert_eq!(grid.row(1), Some(&[10, 11, 12][..]));
    assert_eq!(grid.row(2), None);
    assert!(grid.rows().eq([&[0, 1, 2][..], &[10, 11, 12][..]]));

    *grid.get_mut(1, 0).ok_or("inside")? = 99;
    grid.row_mut(1).ok_or("inside")?.fill(5);
    assert_eq!(grid.as_slice(), &[0, 99, 2, 5, 5, 5]);
    for row in grid.rows_mut() {
        row.reverse();
    }
    assert_eq!(grid.as_slice(), &[2, 99, 0, 5, 5, 5]);
    grid.as_mut_slice()[0] = 1;
    assert_eq!(grid.get(0, 0), Some(&1));
    Ok(())
}

#[test]
fn index_and_position_convert() -> Test {
    let grid = Grid::<u8, Inline<20>>::new(Memory::new(), 5, 4, 0)?;
    assert_eq!(grid.index(3, 2), Some(13));
    assert_eq!(grid.index(5, 0), None);
    assert_eq!(grid.index(0, 4), None);
    assert_eq!(grid.position(13), Some((3, 2)));
    assert_eq!(grid.position(20), None);
    assert!(grid.contains(4, 3));
    assert!(!grid.contains(4, 4));
    for index in 0..20 {
        let back = grid.position(index).and_then(|(x, y)| grid.index(x, y));
        assert_eq!(back, Some(index));
    }
    Ok(())
}

#[test]
fn iter_yields_positions() -> Test {
    let mut grid = Grid::<u32, Inline<6>>::from_fn(Memory::new(), 3, 2, |x, y| y * 10 + x)?;
    assert!(grid.iter().all(|(x, y, cell)| *cell == y * 10 + x));
    assert_eq!(grid.iter().len(), 6);
    assert_eq!(grid.iter().count(), 6);
    for (x, y, cell) in grid.iter_mut() {
        *cell = x + y;
    }
    assert_eq!(grid.as_slice(), &[0, 1, 2, 1, 2, 3]);
    Ok(())
}

#[test]
fn fill_and_fill_rect_clip_to_the_grid() -> Test {
    let mut grid = Grid::<u8, Inline<16>>::new(Memory::new(), 4, 4, 0)?;
    grid.fill_rect(1, 1, 2, 2, 1);
    grid.fill_rect(3, 3, 10, 10, 2);
    grid.fill_rect(9, 0, 1, 1, 3);
    grid.fill_rect(0, 0, u32::MAX, 0, 4);
    grid.fill_rect(u32::MAX, u32::MAX, u32::MAX, u32::MAX, 5);
    #[rustfmt::skip]
    let expected = [
        0, 0, 0, 0,
        0, 1, 1, 0,
        0, 1, 1, 0,
        0, 0, 0, 2,
    ];
    assert_eq!(grid.as_slice(), &expected);
    grid.fill(9);
    assert!(grid.as_slice().iter().all(|cell| *cell == 9));
    Ok(())
}

#[test]
fn offset_never_underflows() -> Test {
    let grid = Grid::<u8, Inline<9>>::new(Memory::new(), 3, 3, 0)?;
    assert_eq!(grid.offset(0, 0, -1, 0), None);
    assert_eq!(grid.offset(0, 0, 0, -1), None);
    assert_eq!(grid.offset(2, 2, 1, 0), None);
    assert_eq!(grid.offset(1, 1, -1, 1), Some((0, 2)));
    assert_eq!(grid.offset(0, 0, i32::MIN, i32::MAX), None);
    assert_eq!(grid.offset(u32::MAX, 0, 1, 0), None);
    Ok(())
}

#[test]
fn neighbors_stay_inside() -> Test {
    let grid = Grid::<u8, Inline<9>>::new(Memory::new(), 3, 3, 0)?;
    let corner: Vec<_> = grid.neighbors(0, 0, Moves::Four).collect();
    assert_eq!(corner, [(1, 0), (0, 1)]);
    let corner: Vec<_> = grid.neighbors(0, 0, Moves::Eight).collect();
    assert_eq!(corner, [(1, 0), (0, 1), (1, 1)]);
    assert_eq!(grid.neighbors(1, 1, Moves::Four).count(), 4);
    assert_eq!(grid.neighbors(1, 1, Moves::Eight).count(), 8);
    assert_eq!(grid.neighbors(2, 2, Moves::Eight).count(), 3);
    // A position outside has no neighbours.
    assert_eq!(grid.neighbors(3, 0, Moves::Eight).count(), 0);
    Ok(())
}

#[test]
fn path_on_a_grid() -> Test {
    let grid = level::<35>(&[
        ".....", //
        ".###.", //
        ".#...", //
        ".#.#.", //
        "...#.", //
        "####.", //
        ".....",
    ])?;
    let mut nodes = Memory::<Node, 35>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    let space = grid.space(Moves::Four, walk);
    let start = grid.index(0, 4).ok_or("inside")?;
    let goal = grid.index(0, 6).ok_or("inside")?;
    // Right, up through the gap, right, down the right side, back along
    // the bottom: 2 + 2 + 2 + 4 + 4.
    assert_eq!(finder.find(&space, start, goal), Ok(14.0));
    let path: Vec<_> = finder.path_rev().filter_map(|n| grid.position(n)).collect();
    assert_eq!(path.len(), 15);
    assert!(path.contains(&(2, 2)));
    assert_eq!(path.first(), Some(&(0, 6)));
    assert_eq!(path.last(), Some(&(0, 4)));
    assert!(
        path.iter()
            .all(|&(x, y)| grid.get(x, y) != Some(&Tile::Wall))
    );
    Ok(())
}

#[test]
fn costs_come_from_the_cells() -> Test {
    let grid = level::<15>(&[
        "..~..", //
        ".....", //
        ".....",
    ])?;
    let mut nodes = Memory::<Node, 15>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    let start = grid.index(0, 0).ok_or("inside")?;
    let goal = grid.index(4, 0).ok_or("inside")?;
    // Through the mud costs 1 + 5 + 1 + 1 = 8; around it 6.
    assert_eq!(
        finder.find(&grid.space(Moves::Four, walk), start, goal),
        Ok(6.0)
    );
    Ok(())
}

#[test]
fn diagonal_steps_cost_more_and_do_not_cut_corners() -> Test {
    let field = level::<9>(&["...", "...", "..."])?;
    let mut nodes = Memory::<Node, 9>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    let cost = finder.find(&field.space(Moves::Eight, walk), 0, 8)?;
    assert!(near(cost, 2.0 * core::f32::consts::SQRT_2));

    // A wall beside a diagonal step blocks it.
    let corner = level::<4>(&[".#", ".."])?;
    let space = corner.space(Moves::Eight, walk);
    assert_eq!(finder.find(&space, 0, 3), Ok(2.0));
    // Unless corners may be cut.
    let cost = finder.find(&space.cutting_corners(), 0, 3)?;
    assert!(near(cost, core::f32::consts::SQRT_2));
    Ok(())
}

#[test]
fn min_cost_keeps_a_star_exact() -> Test {
    // Steps cost 0.5, so the default heuristic (1 per step) would
    // overestimate; min_cost lowers it.
    let grid = level::<25>(&[".....", ".###.", ".....", ".###.", "....."])?;
    let cheap = |_: &Tile, to: &Tile| (*to != Tile::Wall).then_some(0.5);
    let mut nodes = Memory::<Node, 25>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    for moves in [Moves::Four, Moves::Eight] {
        let space = grid.space(moves, cheap).min_cost(0.5);
        let astar = finder.find(&space, 0, 24)?;
        let dijkstra = finder.find(&space.min_cost(0.0), 0, 24)?;
        assert!(near(astar, dijkstra));
    }
    Ok(())
}

#[test]
fn traversal_counts_steps_on_a_grid() -> Test {
    let grid = level::<16>(&["....", ".##.", "...#", "#..."])?;
    let mut marks = Memory::<Mark, 16>::new();
    let mut order = Memory::<u32, 16>::new();
    let mut traversal = Traversal::new(&mut marks, &mut order);
    let space = grid.space(Moves::Four, walk);
    traversal.breadth_first(&space, 0, u32::MAX)?;
    let steps = |x, y| grid.index(x, y).and_then(|node| traversal.steps(node));
    assert_eq!(steps(3, 0), Some(3));
    assert_eq!(steps(3, 3), Some(6));
    assert_eq!(steps(1, 1), None);
    assert_eq!(steps(0, 3), None);
    Ok(())
}

#[test]
fn dynamic_storage_takes_what_it_needs() -> Test {
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    let grid = Grid::new(Dynamic(&arena), 100, 50, 3u16)?;
    assert_eq!(grid.len(), 5000);
    assert_eq!(grid.get(99, 49), Some(&3));
    // Too big for the arena.
    assert!(Grid::new(Dynamic(&arena), 100, 100, 3u16).is_err());
    Ok(())
}

#[test]
fn clone_and_drop_each_cell_once() -> Test {
    struct Counted<'a> {
        made: &'a Cell<usize>,
        dropped: &'a Cell<usize>,
    }
    impl Clone for Counted<'_> {
        fn clone(&self) -> Self {
            self.made.set(self.made.get() + 1);
            Self {
                made: self.made,
                dropped: self.dropped,
            }
        }
    }
    impl Drop for Counted<'_> {
        fn drop(&mut self) {
            self.dropped.set(self.dropped.get() + 1);
        }
    }
    // The fill value counts as made.
    let (made, dropped) = (Cell::new(1), Cell::new(0));
    {
        let fill = Counted {
            made: &made,
            dropped: &dropped,
        };
        let grid = Grid::<_, Inline<6>>::new(Memory::new(), 3, 2, fill)?;
        let copy = grid.clone();
        assert_eq!(copy.len(), 6);
    }
    assert_eq!(made.get(), dropped.get());
    assert!(made.get() >= 12);
    Ok(())
}

#[test]
fn debug_shows_rows() -> Test {
    let grid = Grid::<u8, Inline<4>>::from_fn(Memory::new(), 2, 2, |x, y| (x + 2 * y) as u8)?;
    assert_eq!(format!("{grid:?}"), "[[0, 1], [2, 3]]");
    Ok(())
}

#[test]
fn predecessors_are_the_neighbors_reversed() -> Test {
    use vas::path::Space;
    let mut rng = vas::Rng::new(9);
    let grid = Grid::<u8, Inline<64>>::from_fn(Memory::new(), 8, 8, |_, _| rng.u32_below(5) as u8)?;
    // Depends on both cells, and 0 is a wall.
    let cost = |from: &u8, to: &u8| (*to != 0).then(|| f32::from(*to) + f32::from(*from) * 0.25);
    for moves in [Moves::Four, Moves::Eight] {
        let plain = grid.space(moves, cost);
        for space in [plain.clone(), plain.cutting_corners()] {
            for node in 0..64 {
                let mut forward = Vec::new();
                for from in 0..64 {
                    space.neighbors(from, |to, step| {
                        if to == node {
                            forward.push((from, step.to_bits()));
                        }
                    });
                }
                let mut backward = Vec::new();
                space.predecessors(node, |from, step| backward.push((from, step.to_bits())));
                forward.sort_unstable();
                backward.sort_unstable();
                assert_eq!(forward, backward, "{moves:?} into {node}");
            }
        }
    }
    Ok(())
}
