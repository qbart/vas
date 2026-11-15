use std::collections::VecDeque;
use std::vec;
use std::vec::Vec;
use vas::path::{Node, Open, PathError, Space};
use vas::{Full, Memory, PathFinder, Rng, Vector};

/// A 4-connected tile grid with walls. Node index = y * width + x.
struct Grid {
    width: u32,
    height: u32,
    walls: Vec<bool>,
    use_heuristic: bool,
}

impl Grid {
    fn open(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            walls: vec![false; (width * height) as usize],
            use_heuristic: true,
        }
    }

    fn node(&self, x: u32, y: u32) -> u32 {
        y * self.width + x
    }

    fn wall(&mut self, x: u32, y: u32) {
        let index = self.node(x, y) as usize;
        if let Some(cell) = self.walls.get_mut(index) {
            *cell = true;
        }
    }

    fn is_wall(&self, node: u32) -> bool {
        self.walls.get(node as usize).copied().unwrap_or(true)
    }

    fn adjacent(&self, node: u32) -> impl Iterator<Item = u32> {
        let (x, y) = (node % self.width, node / self.width);
        let mut out = [None; 4];
        if x > 0 {
            out[0] = Some(node - 1);
        }
        if x + 1 < self.width {
            out[1] = Some(node + 1);
        }
        if y > 0 {
            out[2] = Some(node - self.width);
        }
        if y + 1 < self.height {
            out[3] = Some(node + self.width);
        }
        out.into_iter().flatten()
    }

    /// Reference answer: breadth-first search distance.
    fn bfs(&self, start: u32, goal: u32) -> Option<u32> {
        let mut distance = vec![u32::MAX; self.walls.len()];
        let mut queue = VecDeque::new();
        if let Some(d) = distance.get_mut(start as usize) {
            *d = 0;
        }
        queue.push_back(start);
        while let Some(node) = queue.pop_front() {
            let here = distance.get(node as usize).copied()?;
            if node == goal {
                return Some(here);
            }
            for next in self.adjacent(node) {
                if self.is_wall(next) {
                    continue;
                }
                if let Some(d) = distance.get_mut(next as usize)
                    && *d == u32::MAX
                {
                    *d = here + 1;
                    queue.push_back(next);
                }
            }
        }
        None
    }
}

impl Space for Grid {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        for next in self.adjacent(node) {
            if !self.is_wall(next) {
                visit(next, 1.0);
            }
        }
    }

    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        if !self.use_heuristic {
            return 0.0;
        }
        let dx = (from % self.width).abs_diff(goal % self.width);
        let dy = (from / self.width).abs_diff(goal / self.width);
        (dx + dy) as f32
    }
}

/// A small weighted graph given as an edge list.
struct Edges(&'static [(u32, u32, f32)]);

impl Space for Edges {
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        for (from, to, cost) in self.0 {
            if *from == node {
                visit(*to, *cost);
            }
        }
    }
}

#[test]
fn straight_line_on_open_grid() {
    let grid = Grid::open(5, 5);
    let mut nodes = Memory::<Node, 25>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(
        finder.find(&grid, grid.node(0, 0), grid.node(4, 0)),
        Ok(4.0)
    );
    assert!(finder.path_rev().eq([4, 3, 2, 1, 0]));
    assert_eq!(finder.cost(grid.node(4, 0)), Some(4.0));
}

#[test]
fn walks_around_a_wall() {
    let mut grid = Grid::open(5, 5);
    for y in 0..4 {
        grid.wall(2, y);
    }
    let mut nodes = Memory::<Node, 25>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    let (start, goal) = (grid.node(0, 0), grid.node(4, 0));
    assert_eq!(finder.find(&grid, start, goal), Ok(12.0));

    let mut memory = Memory::<u32, 32>::new();
    let mut path = Vector::new(&mut memory);
    assert_eq!(finder.path_into(&mut path), Ok(13));
    assert_eq!(path.first(), Some(&start));
    assert_eq!(path.last(), Some(&goal));
    for pair in path.windows(2) {
        if let [a, b] = pair {
            assert!(grid.adjacent(*a).any(|n| n == *b));
            assert!(!grid.is_wall(*b));
        }
    }
}

#[test]
fn no_path_when_goal_is_walled_off() {
    let mut grid = Grid::open(4, 4);
    for y in 0..4 {
        grid.wall(2, y);
    }
    let mut nodes = Memory::<Node, 16>::new();
    let mut open = Memory::<Open, 64>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&grid, 0, 3), Err(PathError::NoPath));
    assert_eq!(finder.path_rev().count(), 0);
    let mut memory = Memory::<u32, 8>::new();
    let mut path = Vector::new(&mut memory);
    assert_eq!(finder.path_into(&mut path), Ok(0));
    // Nodes that were reached still report their cost.
    assert_eq!(finder.cost(1), Some(1.0));
    assert_eq!(finder.cost(3), None);
}

#[test]
fn start_equals_goal() {
    let grid = Grid::open(3, 3);
    let mut nodes = Memory::<Node, 9>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&grid, 4, 4), Ok(0.0));
    assert!(finder.path_rev().eq([4]));
}

#[test]
fn out_of_bounds_nodes_are_reported() {
    let grid = Grid::open(3, 3);
    let mut nodes = Memory::<Node, 9>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&grid, 9, 0), Err(PathError::OutOfBounds));
    assert_eq!(finder.find(&grid, 0, 9), Err(PathError::OutOfBounds));
    // A neighbour outside the node memory is also an error.
    let edges = Edges(&[(0, 100, 1.0)]);
    assert_eq!(finder.find(&edges, 0, 5), Err(PathError::OutOfBounds));
}

#[test]
fn open_list_full_is_reported() {
    let grid = Grid::open(8, 8);
    let mut nodes = Memory::<Node, 64>::new();
    let mut open = Memory::<Open, 2>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&grid, 0, 63), Err(PathError::Full));
    assert_eq!(finder.path_rev().count(), 0);
}

#[test]
fn weighted_graph_picks_cheapest_route() {
    // 0 -> 1 -> 3 costs 1 + 5; 0 -> 2 -> 3 costs 2 + 1.
    let edges = Edges(&[(0, 1, 1.0), (1, 3, 5.0), (0, 2, 2.0), (2, 3, 1.0)]);
    let mut nodes = Memory::<Node, 4>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&edges, 0, 3), Ok(3.0));
    assert!(finder.path_rev().eq([3, 2, 0]));
    // Edges are directed: there is no way back.
    assert_eq!(finder.find(&edges, 3, 0), Err(PathError::NoPath));
}

#[test]
fn invalid_edge_costs_are_ignored() {
    let edges = Edges(&[
        (0, 1, -1.0),
        (0, 1, f32::NAN),
        (0, 1, f32::INFINITY),
        (0, 2, 1.0),
        (2, 1, 1.0),
    ]);
    let mut nodes = Memory::<Node, 3>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&edges, 0, 1), Ok(2.0));
}

#[test]
fn path_into_reports_full_and_leaves_vector_unchanged() {
    let grid = Grid::open(5, 1);
    let mut nodes = Memory::<Node, 5>::new();
    let mut open = Memory::<Open, 16>::new();
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    assert_eq!(finder.find(&grid, 0, 4), Ok(4.0));
    let mut memory = Memory::<u32, 4>::new();
    let mut path = Vector::new(&mut memory);
    assert_eq!(path.push(99), Ok(()));
    assert_eq!(finder.path_into(&mut path), Err(Full(())));
    assert_eq!(path.as_slice(), &[99]);
    let mut memory = Memory::<u32, 6>::new();
    let mut path = Vector::new(&mut memory);
    assert_eq!(path.push(99), Ok(()));
    assert_eq!(finder.path_into(&mut path), Ok(5));
    assert_eq!(path.as_slice(), &[99, 0, 1, 2, 3, 4]);
}

#[test]
fn matches_breadth_first_search_on_random_grids() {
    let mut rng = Rng::new(2026);
    let mut nodes = Memory::<Node, 144>::new();
    let mut open = Memory::<Open, 1024>::new();
    // One finder reused for every search.
    let mut finder = PathFinder::new(&mut nodes, &mut open);
    let rounds = if cfg!(miri) { 6 } else { 300 };
    for round in 0..rounds {
        let mut grid = Grid::open(12, 12);
        grid.use_heuristic = round % 2 == 0; // A* and Dijkstra
        for cell in grid.walls.iter_mut() {
            *cell = rng.chance(0.3);
        }
        let start = rng.u32_below(144);
        let goal = rng.u32_below(144);
        if grid.is_wall(start) || grid.is_wall(goal) {
            continue;
        }
        match grid.bfs(start, goal) {
            Some(distance) => {
                assert_eq!(finder.find(&grid, start, goal), Ok(distance as f32));
                assert_eq!(finder.path_rev().count(), distance as usize + 1);
                assert_eq!(finder.path_rev().next(), Some(goal));
                assert_eq!(finder.path_rev().last(), Some(start));
            }
            None => assert_eq!(finder.find(&grid, start, goal), Err(PathError::NoPath)),
        }
    }
}
