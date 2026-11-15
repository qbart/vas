# Aabb

`Aabb<const D: usize>` is an axis-aligned bounding box in `D` dimensions: a
rectangle for `D = 2`, a box for `D = 3`. It is the shared geometry type of
the spatial structures.

vas has no math library. Points are plain `[f32; D]` arrays, so the vectors
of any math library convert at no cost.

## Conventions

- `min` and `max` are public; `Aabb::new` sorts the two corners per axis.
- Edges are part of the box: a point on the boundary is inside, and boxes
  that only touch do intersect.
- A NaN coordinate is never inside a box.

## API

| Operation | Signature |
|---|---|
| create | `new(a, b)`, `point(p)`, `around(center, radius)` |
| measure | `center()`, `size()` |
| point tests | `contains_point(&p)`, `distance_squared(&p)` |
| box tests | `contains(&other)`, `intersects(&other)` |
| combine | `union(&other)`, `grown(&p)`, `expanded(margin)` |
