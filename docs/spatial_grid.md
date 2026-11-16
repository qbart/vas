# Spatial grid

`SpatialGrid<T, const D: usize, SC, SE = SC>` is a uniform grid over a region of
`D`-dimensional space that answers "which points are in this box / within
this distance", with [storage parameters](storage.md#storage-parameter). See [storage](storage.md),
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

## Storage

There are two blocks of memory, so there are two storage parameters: the
first for the cells (the product of `counts`), the second for the entries. The second defaults to
the first. See [storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `SpatialGrid<T, 2, Inline<1024>, Inline<256>>` | inside the container: no lifetime, a plain struct field. The two numbers are the capacities of the two blocks |
| `SpatialGrid<T, 2, Borrowed<'a>>` | both blocks provided by the caller: `&mut Memory<..>`, arena pieces or any `&mut [MaybeUninit<..>]` |

Pass `Memory::new()` for both blocks to `new`; too few cells is reported as `NewError::Full`. The two blocks may also use different kinds of storage. In
a turbofish write both: `::<.., _, _>`.

## Growing

With `Dynamic` storage, `new` takes the cell block (one `u32` per cell)
from the allocator, and `insert` takes more entry memory when it runs out.
Entry ids stay valid. `new` reports `NewError::Full` when the allocator
refuses the cell block.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
