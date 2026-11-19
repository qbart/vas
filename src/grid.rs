//! Dense 2D grid of cells (a tile map) over borrowed, inline or growing
//! memory.

use core::f32::consts::SQRT_2;
use core::fmt;
use core::iter::FusedIterator;
use core::slice;

use crate::path::Space;
use crate::storage::{Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// Steps to the four sides first, then the four diagonals:
/// left, right, up, down, up-left, up-right, down-left, down-right.
const STEPS: [(i32, i32); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// Which cells count as next to a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moves {
    /// The four cells sharing a side.
    Four,
    /// The four sides and the four diagonals.
    Eight,
}

impl Moves {
    #[inline]
    const fn count(self) -> u8 {
        match self {
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

/// A `width` × `height` grid of cells stored row after row in one block:
/// a tile map, a height field, per-tile state.
///
/// Positions are `(x, y)` with `x` the column and `y` the row, both from 0.
/// Each cell also has an index, `y * width + x`, which is its node id for
/// [`PathFinder`](crate::PathFinder) and [`Traversal`](crate::Traversal).
///
/// ```
/// use vas::grid::Moves;
/// use vas::path::{Node, Open};
/// use vas::{Grid, Memory, PathFinder};
///
/// let mut cells = Memory::<bool, 12>::new(); // true is a wall
/// let Ok(mut map) = Grid::new(&mut cells, 4, 3, false) else {
///     return;
/// };
/// map.fill_rect(1, 0, 1, 2, true); // a wall down from the top
/// assert_eq!(map.row(1), Some(&[false, true, false, false][..]));
///
/// let mut nodes = Memory::<Node, 12>::new();
/// let mut open = Memory::<Open, 32>::new();
/// let mut finder = PathFinder::new(&mut nodes, &mut open);
/// let space = map.space(Moves::Four, |_, wall| (!wall).then_some(1.0));
/// let (start, goal) = (map.index(0, 0), map.index(3, 0));
/// if let (Some(start), Some(goal)) = (start, goal) {
///     assert_eq!(finder.find(&space, start, goal), Ok(7.0));
/// }
/// ```
///
/// The size is fixed when the grid is created. With [`Dynamic`]
/// storage the memory comes from an allocator, exactly enough for the cells.
///
/// [`Dynamic`]: crate::Dynamic
pub struct Grid<T, S: Storage<T>> {
    // Invariant: `cells.len() == width * height`, which fits a `u32`.
    cells: Vector<T, S>,
    width: u32,
    height: u32,
}

/// Only grids with inline storage can be cloned.
impl<T: Clone, const N: usize> Clone for Grid<T, Inline<N>> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            cells: self.cells.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

/// The number of cells of a `width` × `height` grid, when it fits a `u32`
/// node index.
#[inline]
fn cell_count(width: u32, height: u32) -> Result<usize, Full<()>> {
    let count = u64::from(width) * u64::from(height);
    match u32::try_from(count) {
        Ok(count) => usize::try_from(count).map_err(|_| Full(())),
        Err(_) => Err(Full(())),
    }
}

impl<T, S: Storage<T>> Grid<T, S> {
    /// Creates a `width` × `height` grid on `buffer` with every cell a
    /// clone of `fill`.
    ///
    /// Fails with [`Full`] when the memory holds fewer than
    /// `width * height` cells (or the allocator refuses), or when that
    /// count does not fit a `u32`.
    pub fn new<B>(buffer: B, width: u32, height: u32, fill: T) -> Result<Self, Full<()>>
    where
        B: IntoBuffer<T, Storage = S>,
        T: Clone,
    {
        let count = cell_count(width, height)?;
        let mut cells = Vector::new(buffer);
        cells.resize(count, fill)?;
        Ok(Self {
            cells,
            width,
            height,
        })
    }

    /// Creates a `width` × `height` grid on `buffer` with the cell at
    /// `(x, y)` set to `cell(x, y)`, row by row.
    ///
    /// Fails like [`new`](Self::new), before calling `cell`.
    pub fn from_fn<B>(
        buffer: B,
        width: u32,
        height: u32,
        mut cell: impl FnMut(u32, u32) -> T,
    ) -> Result<Self, Full<()>>
    where
        B: IntoBuffer<T, Storage = S>,
    {
        let count = cell_count(width, height)?;
        let mut cells = Vector::new(buffer);
        cells.reserve(count)?;
        for y in 0..height {
            for x in 0..width {
                cells.push(cell(x, y)).map_err(|_| Full(()))?;
            }
        }
        Ok(Self {
            cells,
            width,
            height,
        })
    }

    /// Number of columns.
    #[inline]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Number of rows.
    #[inline]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Number of cells: `width * height`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.cells.len()
    }

    /// `true` when the grid has no cells.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// `true` when `(x, y)` is inside the grid.
    #[inline]
    pub const fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    /// The index of the cell at `(x, y)`, `y * width + x`: its node id for
    /// path search. `None` outside the grid.
    #[inline]
    pub const fn index(&self, x: u32, y: u32) -> Option<u32> {
        if self.contains(x, y) {
            // Below `width * height`, which fits a `u32`.
            Some(y * self.width + x)
        } else {
            None
        }
    }

    /// The position of the cell with index `index`. `None` past the last
    /// cell.
    #[inline]
    pub fn position(&self, index: u32) -> Option<(u32, u32)> {
        if (index as usize) < self.cells.len() {
            // A grid with cells has a width above zero.
            Some((index % self.width, index / self.width))
        } else {
            None
        }
    }

    /// The position `(x + dx, y + dy)`, when it is inside the grid. Never
    /// overflows, so `offset(0, 0, -1, 0)` is simply `None`.
    #[inline]
    pub fn offset(&self, x: u32, y: u32, dx: i32, dy: i32) -> Option<(u32, u32)> {
        let x = x.checked_add_signed(dx)?;
        let y = y.checked_add_signed(dy)?;
        self.contains(x, y).then_some((x, y))
    }

    /// The cell at `(x, y)`, or `None` outside the grid.
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> Option<&T> {
        self.cells.get(self.index(x, y)? as usize)
    }

    /// The cell at `(x, y)` mutably, or `None` outside the grid.
    #[inline]
    pub fn get_mut(&mut self, x: u32, y: u32) -> Option<&mut T> {
        let index = self.index(x, y)?;
        self.cells.get_mut(index as usize)
    }

    /// Row `y`, left to right, or `None` past the last row.
    #[inline]
    pub fn row(&self, y: u32) -> Option<&[T]> {
        let start = self.index(0, y)? as usize;
        self.cells.get(start..start + self.width as usize)
    }

    /// Row `y` mutably, or `None` past the last row.
    #[inline]
    pub fn row_mut(&mut self, y: u32) -> Option<&mut [T]> {
        let start = self.index(0, y)? as usize;
        let end = start + self.width as usize;
        self.cells.get_mut(start..end)
    }

    /// The rows, top (`y = 0`) first. A grid with no columns has no rows.
    #[inline]
    pub fn rows(&self) -> slice::ChunksExact<'_, T> {
        self.cells.chunks_exact(self.width.max(1) as usize)
    }

    /// The rows mutably, top first.
    #[inline]
    pub fn rows_mut(&mut self) -> slice::ChunksExactMut<'_, T> {
        let width = self.width.max(1) as usize;
        self.cells.chunks_exact_mut(width)
    }

    /// All cells, row after row.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self.cells.as_slice()
    }

    /// All cells mutably, row after row.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.cells.as_mut_slice()
    }

    /// Every cell with its position: `(x, y, &cell)`, row after row.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            cells: self.cells.iter(),
            at: At::new(self.width),
        }
    }

    /// Every cell mutably with its position, row after row.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let at = At::new(self.width);
        IterMut {
            cells: self.cells.iter_mut(),
            at,
        }
    }

    /// Sets every cell to a clone of `value`.
    #[inline]
    pub fn fill(&mut self, value: T)
    where
        T: Clone,
    {
        self.cells.fill(value);
    }

    /// Sets the cells of the `width` × `height` rectangle whose top-left
    /// corner is `(x, y)` to clones of `value`. The parts outside the grid
    /// are left out.
    pub fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, value: T)
    where
        T: Clone,
    {
        let right = x.saturating_add(width).min(self.width) as usize;
        let bottom = y.saturating_add(height).min(self.height);
        let left = x as usize;
        for row in y..bottom {
            if let Some(cells) = self.row_mut(row).and_then(|row| row.get_mut(left..right)) {
                cells.fill(value.clone());
            }
        }
    }

    /// The positions next to `(x, y)` that are inside the grid: the sides
    /// (left, right, up, down), then with [`Moves::Eight`] the diagonals.
    /// None when `(x, y)` itself is outside.
    #[inline]
    pub fn neighbors(&self, x: u32, y: u32, moves: Moves) -> Neighbors {
        Neighbors {
            x,
            y,
            width: self.width,
            height: self.height,
            next: 0,
            end: if self.contains(x, y) {
                moves.count()
            } else {
                0
            },
        }
    }

    /// The grid as a [`Space`] for [`PathFinder`](crate::PathFinder) and
    /// [`Traversal`](crate::Traversal): nodes are cell indices, and a step
    /// goes to a neighbour under `moves`.
    ///
    /// `cost(from, to)` gives the cost of a step to the side between two
    /// cells, or `None` when it cannot be taken (a wall). A diagonal step
    /// costs √2 times as much, and is only taken when both side steps
    /// around its corner can be; see [`Walk::cutting_corners`].
    ///
    /// The heuristic assumes a side step costs at least 1; see
    /// [`Walk::min_cost`] when steps can be cheaper.
    #[inline]
    pub fn space<C>(&self, moves: Moves, cost: C) -> Walk<'_, T, S, C>
    where
        C: Fn(&T, &T) -> Option<f32>,
    {
        Walk {
            grid: self,
            moves,
            cost,
            min_cost: 1.0,
            cut_corners: false,
        }
    }
}

impl<T: fmt::Debug, S: Storage<T>> fmt::Debug for Grid<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.rows()).finish()
    }
}

impl<'g, T, S: Storage<T>> IntoIterator for &'g Grid<T, S> {
    type Item = (u32, u32, &'g T);
    type IntoIter = Iter<'g, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'g, T, S: Storage<T>> IntoIterator for &'g mut Grid<T, S> {
    type Item = (u32, u32, &'g mut T);
    type IntoIter = IterMut<'g, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// The position of the next cell during iteration.
#[derive(Clone)]
struct At {
    x: u32,
    y: u32,
    width: u32,
}

impl At {
    #[inline]
    const fn new(width: u32) -> Self {
        Self { x: 0, y: 0, width }
    }

    /// The current position, then moves one cell along the row.
    #[inline]
    fn step(&mut self) -> (u32, u32) {
        let at = (self.x, self.y);
        self.x += 1;
        if self.x >= self.width {
            self.x = 0;
            self.y += 1;
        }
        at
    }
}

/// Iterator over the cells of a [`Grid`] with their positions. See
/// [`Grid::iter`].
#[derive(Clone)]
pub struct Iter<'g, T> {
    cells: slice::Iter<'g, T>,
    at: At,
}

impl<'g, T> Iterator for Iter<'g, T> {
    type Item = (u32, u32, &'g T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let cell = self.cells.next()?;
        let (x, y) = self.at.step();
        Some((x, y, cell))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.cells.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> FusedIterator for Iter<'_, T> {}

/// Iterator over the cells of a [`Grid`], mutably, with their positions.
/// See [`Grid::iter_mut`].
pub struct IterMut<'g, T> {
    cells: slice::IterMut<'g, T>,
    at: At,
}

impl<'g, T> Iterator for IterMut<'g, T> {
    type Item = (u32, u32, &'g mut T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let cell = self.cells.next()?;
        let (x, y) = self.at.step();
        Some((x, y, cell))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.cells.size_hint()
    }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {}
impl<T> FusedIterator for IterMut<'_, T> {}

/// Iterator over the positions next to a cell. See [`Grid::neighbors`].
#[derive(Clone)]
pub struct Neighbors {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    next: u8,
    end: u8,
}

impl Iterator for Neighbors {
    type Item = (u32, u32);

    #[inline]
    fn next(&mut self) -> Option<(u32, u32)> {
        while self.next < self.end {
            let (dx, dy) = STEPS.get(usize::from(self.next)).copied()?;
            self.next += 1;
            if let (Some(x), Some(y)) =
                (self.x.checked_add_signed(dx), self.y.checked_add_signed(dy))
                && x < self.width
                && y < self.height
            {
                return Some((x, y));
            }
        }
        None
    }
}

impl FusedIterator for Neighbors {}

/// A [`Grid`] seen as a [`Space`] to search: made by [`Grid::space`].
///
/// Nodes are cell indices (`grid.index(x, y)`); read a path back with
/// [`Grid::position`].
pub struct Walk<'g, T, S: Storage<T>, C> {
    grid: &'g Grid<T, S>,
    moves: Moves,
    cost: C,
    min_cost: f32,
    cut_corners: bool,
}

impl<T, S: Storage<T>, C: Clone> Clone for Walk<'_, T, S, C> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            grid: self.grid,
            moves: self.moves,
            cost: self.cost.clone(),
            min_cost: self.min_cost,
            cut_corners: self.cut_corners,
        }
    }
}

impl<T, S: Storage<T>, C> Walk<'_, T, S, C> {
    /// Sets the lowest cost a side step can have (1 by default). The A*
    /// heuristic counts the remaining steps at this cost; set it no higher
    /// than the cheapest step, or the path found may not be the cheapest.
    /// 0 turns A* into Dijkstra's algorithm. A negative or NaN cost counts
    /// as 0.
    #[inline]
    #[must_use]
    pub fn min_cost(mut self, cost: f32) -> Self {
        self.min_cost = cost.max(0.0);
        self
    }

    /// Lets a diagonal step pass a blocked side cell, squeezing between
    /// two walls that touch at a corner. Off by default.
    #[inline]
    #[must_use]
    pub fn cutting_corners(mut self) -> Self {
        self.cut_corners = true;
        self
    }
}

impl<T, S: Storage<T>, C> Space for Walk<'_, T, S, C>
where
    C: Fn(&T, &T) -> Option<f32>,
{
    fn neighbors<F: FnMut(u32, f32)>(&self, node: u32, mut visit: F) {
        let grid = self.grid;
        let Some((x, y)) = grid.position(node) else {
            return;
        };
        let Some(from) = grid.cells.get(node as usize) else {
            return;
        };
        // Whether the step to each side could be taken, for the corners.
        let mut open = [false; 4];
        let (sides, corners) = STEPS.split_at(4);
        for (open, &(dx, dy)) in open.iter_mut().zip(sides) {
            if let Some(index) = grid
                .offset(x, y, dx, dy)
                .and_then(|(x, y)| grid.index(x, y))
                && let Some(to) = grid.cells.get(index as usize)
                && let Some(cost) = (self.cost)(from, to)
            {
                *open = true;
                visit(index, cost);
            }
        }
        if self.moves == Moves::Four {
            return;
        }
        for &(dx, dy) in corners {
            // The side cells around this corner: left or right, up or down.
            let beside = open.get(usize::from(dx > 0)).copied().unwrap_or(false)
                && open.get(2 + usize::from(dy > 0)).copied().unwrap_or(false);
            if !(beside || self.cut_corners) {
                continue;
            }
            if let Some(index) = grid
                .offset(x, y, dx, dy)
                .and_then(|(x, y)| grid.index(x, y))
                && let Some(to) = grid.cells.get(index as usize)
                && let Some(cost) = (self.cost)(from, to)
            {
                visit(index, cost * SQRT_2);
            }
        }
    }

    #[inline]
    fn heuristic(&self, from: u32, goal: u32) -> f32 {
        let (Some((fx, fy)), Some((gx, gy))) = (self.grid.position(from), self.grid.position(goal))
        else {
            return 0.0;
        };
        let (dx, dy) = (fx.abs_diff(gx) as f32, fy.abs_diff(gy) as f32);
        let steps = match self.moves {
            Moves::Four => dx + dy,
            // Diagonal steps for the shorter side, straight for the rest.
            Moves::Eight => dx.max(dy) + (SQRT_2 - 1.0) * dx.min(dy),
        };
        steps * self.min_cost
    }
}
