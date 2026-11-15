# Spatial grid

`SpatialGrid<'a, T, const D: usize>` is a uniform grid over a region of
`D`-dimensional space that answers "which points are in this box / within
this distance", over user-provided memory. See [storage](storage.md),
[errors](errors.md) and [aabb](aabb.md).

Use it for broad-phase collision, neighbour lookups (flocking, AI senses),
and picking, when entries are spread fairly evenly.

## Layout

- `cells`: one `u32` per cell, the head of that cell's list of entries.
- `entries`: a [`Pool`](pool.md) of points; each stores its value, its
  position, its cell and its neighbours in the cell's list (12 bytes of
  links).

Insert, remove and move are O(1). A query walks only the cells its region
touches, then filters exactly by position.

Each entry is a **point**. For objects with a size, insert their centre and
widen the query by the largest object radius.

## Storage

```rust
use vas::spatial_grid::Slot;
use vas::{Aabb, Memory, SpatialGrid};

let world = Aabb::new([0.0, 0.0], [1000.0, 1000.0]);
let mut cells = Memory::<u32, { 32 * 32 }>::new();
let mut entries = Memory::<Slot<Entity, 2>, 4096>::new();
let mut grid = SpatialGrid::new(world, [32, 32], &mut cells, &mut entries)?;
```

Creation clears the cell memory, so it is O(cells). `NewError` is `Full`
(fewer `cells` than the product of the counts) or `Invalid` (non-finite or
empty bounds, or a zero count).

Choose the cell size close to the usual query size: much smaller and a query
walks many cells, much larger and it filters many entries.

## Outside the bounds

Positions outside the bounds are allowed. They are kept in the nearest edge
cell with their true position and are still found by any query that covers
them; the only cost is that edge cells get more crowded. A NaN position is
stored but never matches a query.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `SpatialGrid::new(bounds, counts, cells, entries)` | `Result<_, NewError>` |
| sizes | `len`, `capacity`, `is_empty`, `is_full`, `bounds` | O(1) |
| insert | `insert(position, T) -> Result<EntryId<T, D>, Full<T>>` | O(1) |
| remove | `remove(id) -> Option<T>` | O(1) |
| move | `set_position(id, position) -> bool` | O(1); `false` for a stale id |
| value | `get`, `get_mut`, `position` | `None` for a stale id |
| box query | `query(&Aabb<D>, \|id, value\| ..)` | exact; boundary included |
| radius query | `query_radius(center, radius, \|id, value\| ..)` | exact; boundary included |
| everything | `iter()` | `(id, position, value)` in no particular order |
| clear | `clear()` | O(cells) |

Queries report through a callback, so they need no memory for results.

## Guarantees

- No allocation, no panics.
- Query results are exact: every matching entry is visited once, and no
  other.
