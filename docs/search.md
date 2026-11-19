# Search

`vas::search` searches sorted slices with vas's own binary search, not
`core`'s. The same functions are methods on `Vector`, and they shadow the
slice methods of the same name. Any `&[T]` works.

| Function | Vector method | Returns |
|---|---|---|
| `binary_search(&items, &target)` | `binary_search(&target)` | `Ok(first equal index)` or `Err(insertion index)` |
| `binary_search_by(&items, \|x\| x.cmp(&target))` | `binary_search_by(cmp)` | as above; `cmp` orders an element against the target |
| `binary_search_by_key(&items, &key, \|x\| x.key)` | `binary_search_by_key(&key, f)` | as above, by a key |
| `partition_point(&items, \|x\| pred)` | `partition_point(pred)` | the first index where `pred` is `false` |

## Guarantees

- **The first match.** With several equal elements `core` may return any
  of them, and which one changed with Rust 1.52. These always return the
  lowest index, so the result is the same on every compiler and target,
  and `Ok(i)` / `Err(i)` together are the lower bound.
- **No panics.** On a slice that is not sorted, or a comparison that is not
  a total order, the result is unspecified but within `0..=len`.
- O(log n) comparisons, with no branch on their outcome: one `unsafe`
  unchecked read per step (the index is below the length by construction),
  because the bounds check alone kept the compiler from removing the
  branch and made the search 4x slower.

`OrderedMap` lookups and `KdTree` neighbour lists use these functions;
`Bvh` and `KdTree` builds use [`sort::select_nth_unstable`](sort.md#selection).

## Speed

100,000 lookups in 100,000 sorted `u32`: 11.8 ns each, against 15.6 ns for
`core`'s `binary_search`.
