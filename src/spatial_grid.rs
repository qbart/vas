//! Uniform grid for "what is near here" queries, over borrowed,
//! inline or growing memory.

use core::fmt;

use crate::aabb::{Aabb, distance_squared};
use crate::pool::{self, Handle, Pool};
use crate::storage::{IntoBuffer, Storage};
use crate::{Full, Vector};

/// "No entry" in a link.
const NONE: u32 = u32::MAX;

/// A value in a [`SpatialGrid`] together with its position and links.
/// Opaque; it appears in the types [`Slot`] and [`EntryId`].
pub struct Entry<T, const D: usize> {
    value: T,
    position: [f32; D],
    // The cell this entry is listed in, and its neighbours in that list.
    cell: u32,
    next: u32,
    prev: u32,
}

/// One slot of entry storage: `Memory::<Slot<T, D>, COUNT>`.
pub type Slot<T, const D: usize> = pool::Slot<Entry<T, D>>;

/// Identifies an entry of a [`SpatialGrid`]. `Copy`, 8 bytes, stale once the
/// entry is removed.
pub type EntryId<T, const D: usize> = Handle<Entry<T, D>>;

/// Why a [`SpatialGrid`] could not be created.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NewError {
    /// The cell memory has fewer entries than the grid has cells.
    Full,
    /// The bounds are not finite, have no extent along some axis, or the
    /// cell count is zero along some axis (or `D` is zero).
    Invalid,
}

impl fmt::Display for NewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "cell memory is too small for the grid",
            Self::Invalid => "grid bounds or cell counts are invalid",
        })
    }
}

impl core::error::Error for NewError {}

/// A uniform grid over a region of `D`-dimensional space that answers "which
/// points are in this box / within this distance", in memory provided by
/// the caller.
///
/// Each entry is a point with a value. Inserting, removing and moving an
/// entry are O(1); a query only looks at the cells the queried region
/// touches. Works best when entries are spread fairly evenly and queries are
/// about the size of a cell.
///
/// Positions outside the bounds are allowed: they are kept in the nearest
/// edge cell and still found by queries that cover them.
///
/// ```
/// use vas::spatial_grid::Slot;
/// use vas::{Aabb, Memory, SpatialGrid};
///
/// let world = Aabb::new([0.0, 0.0], [100.0, 100.0]);
/// let mut cells = Memory::<u32, 100>::new(); // 10 x 10 cells
/// let mut entries = Memory::<Slot<&str, 2>, 64>::new();
///
/// if let Ok(mut grid) = SpatialGrid::new(world, [10, 10], &mut cells, &mut entries) {
///     assert!(grid.insert([12.0, 14.0], "goblin").is_ok());
///     assert!(grid.insert([80.0, 75.0], "dragon").is_ok());
///
///     let mut near = 0;
///     grid.query_radius([10.0, 10.0], 8.0, |_, _| near += 1);
///     assert_eq!(near, 1);
/// }
/// ```
///
/// With [`Dynamic`](crate::Dynamic) storage the capacity is not fixed: the
/// memory comes from an allocator and the container grows when it runs out.
pub struct SpatialGrid<T, const D: usize, SC: Storage<u32>, SE: Storage<Slot<T, D>> = SC> {
    // Invariant: `cells[i]` is the head of a doubly linked list of exactly
    // the live entries whose `cell` is `i`.
    cells: Vector<u32, SC>,
    entries: Pool<Entry<T, D>, SE>,
    bounds: Aabb<D>,
    // Number of cells along each axis.
    counts: [u32; D],
    // Cells per world unit along each axis.
    scale: [f32; D],
}

impl<T, const D: usize, SC: Storage<u32>, SE: Storage<Slot<T, D>>> SpatialGrid<T, D, SC, SE> {
    /// Creates an empty grid covering `bounds` with `counts` cells along
    /// each axis.
    ///
    /// `cells` needs one `u32` per cell (the product of `counts`); `entries`
    /// sets how many points the grid can hold. Initialises the cell memory,
    /// so creation is O(cells).
    ///
    /// References to memory borrow it; `Memory` blocks passed by value are
    /// stored inside the grid.
    pub fn new<BC, BE>(
        bounds: Aabb<D>,
        counts: [u32; D],
        cells: BC,
        entries: BE,
    ) -> Result<Self, NewError>
    where
        BC: IntoBuffer<u32, Storage = SC>,
        BE: IntoBuffer<Slot<T, D>, Storage = SE>,
    {
        if D == 0 {
            return Err(NewError::Invalid);
        }
        let mut total: usize = 1;
        let mut scale = [0.0f32; D];
        let axes = scale
            .iter_mut()
            .zip(counts.iter())
            .zip(bounds.min.iter().zip(bounds.max.iter()));
        for ((scale, count), (low, high)) in axes {
            let extent = high - low;
            if *count == 0
                || !low.is_finite()
                || !high.is_finite()
                || extent.is_nan()
                || extent <= 0.0
            {
                return Err(NewError::Invalid);
            }
            *scale = *count as f32 / extent;
            total = total
                .checked_mul(*count as usize)
                .ok_or(NewError::Invalid)?;
        }
        if u32::try_from(total).is_err() {
            return Err(NewError::Invalid);
        }
        let mut cells = Vector::new(cells);
        cells.resize(total, NONE).map_err(|_| NewError::Full)?;
        Ok(Self {
            cells,
            entries: Pool::new(entries),
            bounds,
            counts,
            scale,
        })
    }

    /// Number of entries in the grid.
    #[inline]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Maximum number of entries the grid can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// `true` when the grid holds no entries.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` when no more entries can be inserted.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    /// The region the grid's cells cover.
    #[inline]
    pub const fn bounds(&self) -> Aabb<D> {
        self.bounds
    }

    /// Adds `value` at `position`, or hands the value back in [`Full`] when
    /// there is no room.
    pub fn insert(&mut self, position: [f32; D], value: T) -> Result<EntryId<T, D>, Full<T>> {
        let cell = self.cell_of(&position);
        let entry = Entry {
            value,
            position,
            cell,
            next: NONE,
            prev: NONE,
        };
        let id = self
            .entries
            .insert(entry)
            .map_err(|Full(entry)| Full(entry.value))?;
        self.link(id.index(), cell);
        Ok(id)
    }

    /// Removes an entry and returns its value.
    pub fn remove(&mut self, id: EntryId<T, D>) -> Option<T> {
        if !self.entries.contains(id) {
            return None;
        }
        self.unlink(id.index());
        self.entries.remove(id).map(|entry| entry.value)
    }

    /// Moves an entry to `position`. Returns `false` when the id is stale.
    pub fn set_position(&mut self, id: EntryId<T, D>, position: [f32; D]) -> bool {
        let cell = self.cell_of(&position);
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        entry.position = position;
        if entry.cell != cell {
            self.unlink(id.index());
            self.link(id.index(), cell);
        }
        true
    }

    /// The value of entry `id`.
    #[inline]
    pub fn get(&self, id: EntryId<T, D>) -> Option<&T> {
        self.entries.get(id).map(|entry| &entry.value)
    }

    /// The value of entry `id`, mutably.
    #[inline]
    pub fn get_mut(&mut self, id: EntryId<T, D>) -> Option<&mut T> {
        self.entries.get_mut(id).map(|entry| &mut entry.value)
    }

    /// The position of entry `id`.
    #[inline]
    pub fn position(&self, id: EntryId<T, D>) -> Option<[f32; D]> {
        self.entries.get(id).map(|entry| entry.position)
    }

    /// Removes and drops all entries. Every existing id goes stale.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cells.fill(NONE);
    }

    /// Calls `visit(id, value)` for every entry whose position is inside
    /// `area` (boundary included). The order is unspecified.
    pub fn query(&self, area: &Aabb<D>, mut visit: impl FnMut(EntryId<T, D>, &T)) {
        self.for_each_in_cells(area, |id, entry| {
            if area.contains_point(&entry.position) {
                visit(id, &entry.value);
            }
        });
    }

    /// Calls `visit(id, value)` for every entry within `radius` of `center`
    /// (boundary included). The order is unspecified.
    pub fn query_radius(
        &self,
        center: [f32; D],
        radius: f32,
        mut visit: impl FnMut(EntryId<T, D>, &T),
    ) {
        if radius.is_nan() || radius < 0.0 {
            return;
        }
        let limit = radius * radius;
        self.for_each_in_cells(&Aabb::around(center, radius), |id, entry| {
            if distance_squared(&entry.position, &center) <= limit {
                visit(id, &entry.value);
            }
        });
    }

    /// Iterates over all entries as `(id, position, value)`, in no
    /// particular order.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (EntryId<T, D>, [f32; D], &T)> {
        self.entries
            .iter()
            .map(|(id, entry)| (id, entry.position, &entry.value))
    }

    /// Visits every entry listed in a cell that `area` touches.
    fn for_each_in_cells(
        &self,
        area: &Aabb<D>,
        mut visit: impl FnMut(EntryId<T, D>, &Entry<T, D>),
    ) {
        let low = self.coordinates(&area.min);
        let high = self.coordinates(&area.max);
        let mut current = low;
        loop {
            let mut next = self
                .cells
                .get(self.flatten(&current))
                .copied()
                .unwrap_or(NONE);
            while let Some(entry) = self.entries.get_at(next) {
                if let Some(id) = self.entries.handle_at(next) {
                    visit(id, entry);
                }
                next = entry.next;
            }
            // Step to the next cell of the box, like an odometer.
            let mut wrapped = true;
            for ((value, low), high) in current.iter_mut().zip(low.iter()).zip(high.iter()) {
                if *value < *high {
                    *value += 1;
                    wrapped = false;
                    break;
                }
                *value = *low;
            }
            if wrapped {
                return;
            }
        }
    }

    /// Cell coordinates of `position`, clamped into the grid.
    fn coordinates(&self, position: &[f32; D]) -> [u32; D] {
        let mut coordinates = [0u32; D];
        let axes = coordinates
            .iter_mut()
            .zip(position.iter())
            .zip(self.bounds.min.iter().zip(self.scale.iter()))
            .zip(self.counts.iter());
        for (((coordinate, value), (low, scale)), count) in axes {
            // The cast saturates: below the grid and NaN become 0, far
            // above becomes the last cell.
            *coordinate = (((value - low) * scale) as u32).min(count - 1);
        }
        coordinates
    }

    /// Index into `cells` for cell coordinates.
    fn flatten(&self, coordinates: &[u32; D]) -> usize {
        let mut index = 0usize;
        let mut stride = 1usize;
        for (coordinate, count) in coordinates.iter().zip(self.counts.iter()) {
            index += *coordinate as usize * stride;
            stride *= *count as usize;
        }
        index
    }

    fn cell_of(&self, position: &[f32; D]) -> u32 {
        // The total number of cells fits in `u32` (checked in `new`).
        self.flatten(&self.coordinates(position)) as u32
    }

    /// Puts the detached entry `index` at the front of the list of `cell`.
    fn link(&mut self, index: u32, cell: u32) {
        let Some(head) = self.cells.get_mut(cell as usize) else {
            return;
        };
        let first = *head;
        *head = index;
        if let Some(entry) = self.entries.get_at_mut(index) {
            entry.cell = cell;
            entry.prev = NONE;
            entry.next = first;
        }
        if let Some(after) = self.entries.get_at_mut(first) {
            after.prev = index;
        }
    }

    /// Takes entry `index` out of its cell's list.
    fn unlink(&mut self, index: u32) {
        let Some(entry) = self.entries.get_at(index) else {
            return;
        };
        let (cell, next, prev) = (entry.cell, entry.next, entry.prev);
        if let Some(before) = self.entries.get_at_mut(prev) {
            before.next = next;
        } else if let Some(head) = self.cells.get_mut(cell as usize) {
            *head = next;
        }
        if let Some(after) = self.entries.get_at_mut(next) {
            after.prev = prev;
        }
    }
}
