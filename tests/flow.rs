use std::error::Error;
use std::vec::Vec;
use vas::flow::FlowError;
use vas::grid::Moves;
use vas::path::{Node, Open, Space};
use vas::{Arena, Dynamic, FlowField, Full, Grid, Inline, Memory, PathFinder};

type Test = Result<(), Box<dyn Error>>;

/// Parses rows of `.` (floor, 1), `~` (mud, 4) and `#` (wall).
fn level<const N: usize>(rows: &[&str]) -> Result<Grid<u8, Inline<N>>, Full<()>> {
    let width = rows.first().map_or(0, |row| row.len()) as u32;
    Grid::from_fn(Memory::new(), width, rows.len() as u32, |x, y| {
        rows[y as usize].as_bytes()[x as usize]
    })
}

fn walk(_: &u8, to: &u8) -> Option<f32> {
    match to {
        b'#' => None,
        b'~' => Some(4.0),
        _ => Some(1.0),
    }
}

#[test]
fn open_field_cost_is_the_distance() -> Test {
    let grid = Grid::<u8, Inline<35>>::new(Memory::new(), 7, 5, b'.')?;
    let mut nodes = Memory::<Node, 35>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    let goal = grid.index(3, 2).ok_or("inside")?;
    field.build(&grid.space(Moves::Four, walk), &[goal], f32::INFINITY)?;
    for (x, y, _) in grid.iter() {
        let node = grid.index(x, y).ok_or("inside")?;
        let distance = (x.abs_diff(3) + y.abs_diff(2)) as f32;
        assert_eq!(field.cost(node), Some(distance));
    }
    assert_eq!(field.next(goal), None);
    Ok(())
}

#[test]
fn following_next_goes_downhill_to_a_goal() -> Test {
    let grid = level::<48>(&[
        "........", //
        ".####.#.", //
        "....#.#.", //
        "###.#...", //
        "........", //
        ".######.",
    ])?;
    let mut nodes = Memory::<Node, 48>::new();
    let mut open = Memory::<Open, 128>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    let goal = grid.index(0, 2).ok_or("inside")?;
    field.build(&grid.space(Moves::Eight, walk), &[goal], f32::INFINITY)?;
    for (x, y, cell) in grid.iter() {
        let node = grid.index(x, y).ok_or("inside")?;
        if *cell == b'#' {
            // Walls cannot be entered; leaving one is allowed.
            assert!(
                field
                    .route(node)
                    .skip(1)
                    .all(|n| grid.as_slice()[n as usize] != b'#')
            );
        }
        let route: Vec<u32> = field.route(node).collect();
        assert_eq!(route.first(), Some(&node));
        assert_eq!(route.last(), Some(&goal));
        for pair in route.windows(2) {
            let (here, there) = (field.cost(pair[0]), field.cost(pair[1]));
            assert!(here > there, "downhill at {pair:?}");
        }
    }
    Ok(())
}

#[test]
fn nearest_of_several_goals() -> Test {
    let grid = Grid::<u8, Inline<20>>::new(Memory::new(), 10, 2, b'.')?;
    let mut nodes = Memory::<Node, 20>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    let goals = [0, 9, 19];
    field.build(&grid.space(Moves::Four, walk), &goals, f32::INFINITY)?;
    for goal in goals {
        assert_eq!(field.cost(goal), Some(0.0));
        assert_eq!(field.route(goal).count(), 1);
    }
    let at = |x, y| grid.index(x, y).and_then(|node| field.cost(node));
    assert_eq!(at(4, 0), Some(4.0));
    assert_eq!(at(6, 0), Some(3.0));
    assert_eq!(at(8, 1), Some(1.0));
    assert_eq!(at(0, 1), Some(1.0));
    Ok(())
}

/// With costs on the cell entered, a step costs something else each way.
/// The field is built from the goals out, yet its routes and costs match
/// A* from each cell.
#[test]
fn routes_are_as_cheap_as_a_star() -> Test {
    let grid = level::<64>(&[
        "..~~....", //
        ".#~~.##.", //
        ".#......", //
        ".#~~~~#.", //
        "..~..~#.", //
        "#.~#.~..", //
        "..~#....", //
        "........",
    ])?;
    for moves in [Moves::Four, Moves::Eight] {
        let space = grid.space(moves, walk);
        let mut nodes = Memory::<Node, 64>::new();
        let mut open = Memory::<Open, 256>::new();
        let mut field = FlowField::new(&mut nodes, &mut open);
        let goal = grid.index(7, 0).ok_or("inside")?;
        field.build(&space, &[goal], f32::INFINITY)?;

        let mut finder_nodes = Memory::<Node, 64>::new();
        let mut finder_open = Memory::<Open, 256>::new();
        let mut finder = PathFinder::new(&mut finder_nodes, &mut finder_open);
        for start in 0..64 {
            let Ok(best) = finder.find(&space, start, goal) else {
                assert_eq!(field.cost(start), None);
                continue;
            };
            // The cost of walking the route: what each step costs in the
            // space, from the cell before it.
            let route: Vec<u32> = field.route(start).collect();
            let mut walked = 0.0;
            for pair in route.windows(2) {
                let mut step = None;
                space.neighbors(pair[0], |next, cost| {
                    if next == pair[1] {
                        step = Some(cost);
                    }
                });
                walked += step.ok_or("route steps are moves")?;
            }
            assert!((walked - best).abs() < 1e-3, "{moves:?} from {start}");
            let cost = field.cost(start).ok_or("reached")?;
            assert!((cost - best).abs() < 1e-3);
        }
    }
    Ok(())
}

#[test]
fn max_cost_stops_the_field() -> Test {
    let grid = Grid::<u8, Inline<10>>::new(Memory::new(), 10, 1, b'.')?;
    let mut nodes = Memory::<Node, 10>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    field.build(&grid.space(Moves::Four, walk), &[0], 3.0)?;
    assert_eq!(field.cost(3), Some(3.0));
    assert_eq!(field.cost(4), None);
    assert_eq!(field.next(4), None);
    assert_eq!(field.route(4).count(), 0);
    Ok(())
}

#[test]
fn rebuilding_forgets_the_old_field() -> Test {
    let grid = Grid::<u8, Inline<10>>::new(Memory::new(), 10, 1, b'.')?;
    let mut nodes = Memory::<Node, 10>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    let space = grid.space(Moves::Four, walk);
    field.build(&space, &[0], f32::INFINITY)?;
    assert_eq!(field.cost(9), Some(9.0));
    field.build(&space, &[9], 2.0)?;
    assert_eq!(field.cost(9), Some(0.0));
    assert_eq!(field.cost(0), None);
    // No goals: nothing is reached.
    field.build(&space, &[], f32::INFINITY)?;
    assert_eq!(field.cost(9), None);
    Ok(())
}

#[test]
fn errors_are_reported() -> Test {
    let grid = Grid::<u8, Inline<10>>::new(Memory::new(), 10, 1, b'.')?;
    let space = grid.space(Moves::Four, walk);
    // Room for fewer nodes than the grid has.
    let mut nodes = Memory::<Node, 5>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    assert_eq!(
        field.build(&space, &[7], f32::INFINITY),
        Err(FlowError::OutOfBounds)
    );
    assert_eq!(
        field.build(&space, &[0], f32::INFINITY),
        Err(FlowError::OutOfBounds)
    );
    assert_eq!(field.cost(0), None);
    // An open list too small for the goals.
    let mut nodes = Memory::<Node, 10>::new();
    let mut open = Memory::<Open, 1>::new();
    let mut field = FlowField::new(&mut nodes, &mut open);
    assert_eq!(
        field.build(&space, &[0, 9], f32::INFINITY),
        Err(FlowError::Full)
    );
    // A failed build leaves no field behind.
    assert_eq!(field.cost(0), None);
    Ok(())
}

#[test]
fn dynamic_storage_grows() -> Test {
    let grid = Grid::<u8, Inline<400>>::new(Memory::new(), 20, 20, b'.')?;
    let mut block = Memory::<u8, 16384>::new();
    let arena = Arena::new(&mut block);
    let mut field = FlowField::new(Dynamic(&arena), Dynamic(&arena));
    field.build(&grid.space(Moves::Eight, walk), &[0], f32::INFINITY)?;
    assert_eq!(field.node_capacity(), 400);
    assert_eq!(field.route(399).count(), 20);
    Ok(())
}

#[test]
fn clone_keeps_the_field() -> Test {
    let grid = Grid::<u8, Inline<9>>::new(Memory::new(), 3, 3, b'.')?;
    let mut field = FlowField::<Inline<9>, Inline<32>>::new(Memory::new(), Memory::new());
    field.build(&grid.space(Moves::Four, walk), &[4], f32::INFINITY)?;
    let copy = field.clone();
    field.build(&grid.space(Moves::Four, walk), &[0], f32::INFINITY)?;
    assert_eq!(copy.cost(0), Some(2.0));
    assert_eq!(copy.next(1), Some(4));
    assert_eq!(field.cost(0), Some(0.0));
    Ok(())
}
