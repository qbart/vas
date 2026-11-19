# Grid

`Grid<T, S>` is a dense `width` × `height` grid of cells stored row after
row in one block, with a [storage parameter](storage.md#storage-parameter).
See [storage](storage.md) and [errors](errors.md).

Use it for anything with one value per tile: tile maps, walls and floors,
height fields, fog of war, influence maps. For many moving objects looked
up by area, use [`SpatialGrid`](spatial_grid.md) instead: it lists points
per cell, while `Grid` holds one value in each cell.

## Positions and indices

A position is `(x, y)`: `x` the column, `y` the row, both `u32` from 0.
Every cell also has an index, `y * width + x`, which is its node id for
[`PathFinder`](path.md) and [`Traversal`](traversal.md).

- `index(x, y)` and `position(index)` convert, `None` outside the grid.
- `offset(x, y, dx, dy)` moves by a signed step and returns `None` when
  that leaves the grid, so `x - 1` at the left edge never underflows.
- `neighbors(x, y, Moves::Four | Moves::Eight)` lists the positions next
  to a cell that are inside the grid: the sides (left, right, up, down),
  then the diagonals.

`width * height` must fit a `u32`.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer, width, height, fill)` (`T: Clone`), `from_fn(buffer, width, height, \|x, y\| cell)` | `Full` when the memory holds fewer than `width * height` cells |
| sizes | `width()`, `height()`, `len()`, `is_empty()` | fixed after creation |
| cell | `get(x, y)`, `get_mut(x, y)` | `None` outside |
| rows | `row(y)`, `row_mut(y)`, `rows()`, `rows_mut()` | slices, left to right; top row first |
| all cells | `as_slice()`, `as_mut_slice()` | row after row: hand it to a renderer or upload it as a texture |
| iterate | `iter()`, `iter_mut()`, `&grid` | `(x, y, &cell)`, row after row |
| fill | `fill(value)`, `fill_rect(x, y, width, height, value)` | the rectangle is clipped to the grid |
| positions | `contains`, `index`, `position`, `offset`, `neighbors` | |
| search | `space(moves, cost)` | a [`Space`](path.md#the-space) over the grid; see below |

## Path search and traversal

`grid.space(moves, cost)` turns the grid into a `Space`, so a grid needs no
hand-written `Space` impl:

```rust
use vas::grid::Moves;

let walk = map.space(Moves::Eight, |_from, to| match to {
    Tile::Floor => Some(1.0),
    Tile::Mud => Some(3.0),
    Tile::Wall => None, // cannot step here
});
finder.find(&walk, map.index(0, 0)?, map.index(9, 9)?)?;
let path = finder.path_rev().filter_map(|node| map.position(node));
```

- `cost(from, to)` is the cost of a step to the side between two cells, or
  `None` when the step cannot be taken.
- A diagonal step costs √2 times `cost(from, to)`. It is taken only when
  both side steps around its corner can be taken, so a unit never squeezes
  between two walls touching at a corner. `.cutting_corners()` allows it.
- The A* heuristic is the Manhattan distance (four moves) or the octile
  distance (eight moves), counted at 1 per side step. When a step can cost
  less than 1, pass the lowest cost to `.min_cost(c)`, or the path found
  may not be the cheapest; `.min_cost(0.0)` gives Dijkstra's algorithm.
- `Traversal` ignores the costs; any step with a cost counts as one.

The node memory of a `PathFinder` or `Traversal` needs one entry per cell.

## Guarantees

- No allocation beyond the storage, no panics, no `unsafe`. Reads outside
  the grid return `None`, writes outside are left out.
- Every cell is initialised: creation fills them all, O(width × height).

## Storage

| Type | Memory |
|---|---|
| `Grid<T, Inline<N>>` | inside the grid: no lifetime, a plain struct field; can be cloned |
| `Grid<T, Borrowed<'a>>` | provided by the caller: pass `&mut Memory<T, N>`, an arena piece or any `&mut [MaybeUninit<T>]` to `new` |
| `Grid<T, Dynamic<A>>` | taken from the allocator `A` when the grid is created, exactly `width * height` cells |

## Not included

- Resizing a grid. Create a new one and copy the overlapping rows
  (`row` / `row_mut`).
- Three or more dimensions. Index a 3D voxel grid by hand in a `Vector`,
  or use one `Grid` per layer.
- Hexagonal grids: write a `Space` for the layout the game uses.
