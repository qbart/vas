//! Lines, line of sight and field of view on a [`Grid`].

use core::iter::FusedIterator;

use super::Grid;
use crate::storage::Storage;

/// The cells of a straight line from `from` to `to`, both included, one step
/// (side or diagonal) at a time: Bresenham's line.
///
/// The line is the same both ways: `line(a, b)` visits the cells of
/// `line(b, a)` in reverse. Where the exact line passes halfway between two
/// cells, the one with the lower coordinate is taken.
///
/// ```
/// use vas::grid::line;
///
/// assert!(line((0, 0), (4, 2)).eq([(0, 0), (1, 0), (2, 1), (3, 1), (4, 2)]));
/// ```
#[inline]
pub fn line(from: (u32, u32), to: (u32, u32)) -> Line {
    let (x0, y0) = (i64::from(from.0), i64::from(from.1));
    let (dx, dy) = (i64::from(to.0) - x0, i64::from(to.1) - y0);
    let x_major = dx.abs() >= dy.abs();
    let (major, minor) = if x_major { (dx, dy) } else { (dy, dx) };
    Line {
        x: x0,
        y: y0,
        x_major,
        major_step: major.signum(),
        minor_step: minor.signum(),
        // Exactly halfway, a line going down the minor axis moves on (to
        // the lower coordinate); one going up waits.
        tie_moves: minor < 0,
        rest: 0,
        rise: 2 * minor.abs(),
        run: major.abs(),
        left: major.unsigned_abs() + 1,
    }
}

/// Iterator over the cells of a line. See [`line()`].
#[derive(Clone, Debug)]
pub struct Line {
    x: i64,
    y: i64,
    x_major: bool,
    major_step: i64,
    minor_step: i64,
    tie_moves: bool,
    // Twice the distance, in units of 1 / run, from the cell's centre to
    // the exact line along the minor axis; within -run..=run.
    rest: i64,
    rise: i64,
    run: i64,
    // Cells not yielded yet: at most 2^32.
    left: u64,
}

impl Iterator for Line {
    type Item = (u32, u32);

    #[inline]
    fn next(&mut self) -> Option<(u32, u32)> {
        if self.left == 0 {
            return None;
        }
        // Every cell lies between the two ends, so it fits a `u32`.
        let cell = (u32::try_from(self.x).ok()?, u32::try_from(self.y).ok()?);
        self.left -= 1;
        self.rest += self.rise;
        let moves = self.rest > self.run || (self.tie_moves && self.rest == self.run);
        let minor = if moves {
            self.rest -= 2 * self.run;
            self.minor_step
        } else {
            0
        };
        if self.x_major {
            self.x += self.major_step;
            self.y += minor;
        } else {
            self.y += self.major_step;
            self.x += minor;
        }
        Some(cell)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        match usize::try_from(self.left) {
            Ok(left) => (left, Some(left)),
            Err(_) => (usize::MAX, None),
        }
    }
}

impl FusedIterator for Line {}

/// Rows further than this from the origin of a field of view are not
/// scanned, which keeps the slope arithmetic within `i64`.
const FAR: u32 = 1 << 24;

/// The slope `num / den` of a ray from the origin, `den > 0`: columns
/// across per row away.
#[derive(Clone, Copy)]
struct Slope {
    num: i64,
    den: i64,
}

impl Slope {
    /// The slope through the near edge of the cell at (`depth`, `col`).
    #[inline]
    const fn edge(depth: i64, col: i64) -> Self {
        Self {
            num: 2 * col - 1,
            den: 2 * depth,
        }
    }
}

/// One field-of-view computation: the grid, the origin and the callbacks.
struct View<'g, T, S: Storage<T>, O, V> {
    grid: &'g Grid<T, S>,
    x: i64,
    y: i64,
    // Which quarter is being scanned: 0 up, 1 right, 2 down, 3 left.
    quarter: u8,
    far: i64,
    radius_squared: i64,
    opaque: O,
    visible: V,
}

impl<T, S: Storage<T>, O, V> View<'_, T, S, O, V>
where
    O: FnMut(&T) -> bool,
    V: FnMut(u32, u32),
{
    /// The grid position of the cell `depth` rows out from the origin and
    /// `col` columns to the side, in the current quarter.
    #[inline]
    fn position(&self, depth: i64, col: i64) -> Option<(u32, u32)> {
        let (x, y) = match self.quarter {
            0 => (self.x + col, self.y - depth),
            1 => (self.x + depth, self.y + col),
            2 => (self.x + col, self.y + depth),
            _ => (self.x - depth, self.y + col),
        };
        let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
        self.grid.contains(x, y).then_some((x, y))
    }

    /// Whether the cell blocks sight; outside the grid everything does.
    #[inline]
    fn blocks(&mut self, cell: Option<(u32, u32)>) -> bool {
        match cell.and_then(|(x, y)| self.grid.get(x, y)) {
            Some(value) => (self.opaque)(value),
            None => true,
        }
    }

    #[inline]
    fn reveal(&mut self, cell: Option<(u32, u32)>, depth: i64, col: i64) {
        // The diagonals belong to both neighbouring quarters; only the up
        // and down quarters report them, so each cell is reported once.
        let diagonal = self.quarter % 2 == 1 && col.abs() == depth;
        if let Some((x, y)) = cell
            && !diagonal
            && col * col + depth * depth <= self.radius_squared
        {
            (self.visible)(x, y);
        }
    }

    /// Scans the rows from `depth` outwards between the rays `start` and
    /// `end` (symmetric shadowcasting, after Albert Ford). Recurses once
    /// per gap that opens behind a wall, so at most `far` calls deep.
    fn scan(&mut self, mut depth: i64, mut start: Slope, end: Slope) {
        while depth <= self.far {
            // The columns whose centres are within the rays, ties
            // rounded inwards.
            let first = (2 * depth * start.num + start.den).div_euclid(2 * start.den);
            let last = -(-(2 * depth * end.num - end.den)).div_euclid(2 * end.den);
            let mut before: Option<bool> = None;
            for col in first..=last {
                let cell = self.position(depth, col);
                let wall = self.blocks(cell);
                // A floor cell is seen only when its centre is within the
                // rays, which makes sight symmetric; a wall when any of it
                // is.
                let centred =
                    col * start.den >= depth * start.num && col * end.den <= depth * end.num;
                if wall || centred {
                    self.reveal(cell, depth, col);
                }
                match (before, wall) {
                    // A wall ends: the next row starts at this cell's edge.
                    (Some(true), false) => start = Slope::edge(depth, col),
                    // A wall starts: scan behind the gap before it.
                    (Some(false), true) => self.scan(depth + 1, start, Slope::edge(depth, col)),
                    _ => {}
                }
                before = Some(wall);
            }
            if before != Some(false) {
                return;
            }
            depth += 1;
        }
    }
}

impl<T, S: Storage<T>> Grid<T, S> {
    /// `true` when nothing blocks the [`line()`] from `from` to `to`: no cell
    /// strictly between them is `opaque`. The two end cells themselves may
    /// be (a wall can be seen). `false` when either end is outside the
    /// grid.
    ///
    /// Symmetric: `a` sees `b` exactly when `b` sees `a`. O(distance).
    pub fn line_of_sight(
        &self,
        from: (u32, u32),
        to: (u32, u32),
        mut opaque: impl FnMut(&T) -> bool,
    ) -> bool {
        if !self.contains(from.0, from.1) || !self.contains(to.0, to.1) {
            return false;
        }
        line(from, to)
            .skip(1)
            .take_while(|&cell| cell != to)
            .all(|(x, y)| !self.get(x, y).is_some_and(&mut opaque))
    }

    /// Calls `visible(x, y)` once for every cell seen from `(x, y)` within
    /// `radius` (a disc: `dx² + dy² <= radius²`), the origin included.
    /// Cells for which `opaque` is `true` block the view behind them, and
    /// are seen themselves. Nothing is reported when the origin is outside
    /// the grid.
    ///
    /// Symmetric shadowcasting: a floor cell `b` is seen from `a` exactly
    /// when `a` is seen from `b`, so a monster sees the player exactly when
    /// the player sees it. O(cells within the radius); recursion goes at
    /// most `radius` calls deep, and rows more than 2^24 away are not
    /// scanned.
    ///
    /// ```
    /// use vas::{Grid, Memory};
    ///
    /// let mut cells = Memory::<bool, 25>::new(); // true is a wall
    /// let Ok(mut map) = Grid::new(&mut cells, 5, 5, false) else {
    ///     return;
    /// };
    /// map.fill_rect(3, 0, 1, 5, true); // a wall down the middle
    ///
    /// let mut seen = 0;
    /// map.field_of_view(1, 2, 10, |wall| *wall, |x, _| {
    ///     assert!(x <= 3); // nothing behind the wall
    ///     seen += 1;
    /// });
    /// assert_eq!(seen, 20);
    /// ```
    pub fn field_of_view(
        &self,
        x: u32,
        y: u32,
        radius: u32,
        opaque: impl FnMut(&T) -> bool,
        mut visible: impl FnMut(u32, u32),
    ) {
        if !self.contains(x, y) {
            return;
        }
        visible(x, y);
        let radius = i64::from(radius.min(FAR));
        let mut view = View {
            grid: self,
            x: i64::from(x),
            y: i64::from(y),
            quarter: 0,
            far: radius.min(i64::from(self.width.max(self.height))),
            radius_squared: radius * radius,
            opaque,
            visible,
        };
        for quarter in 0..4 {
            view.quarter = quarter;
            view.scan(1, Slope { num: -1, den: 1 }, Slope { num: 1, den: 1 });
        }
    }
}
