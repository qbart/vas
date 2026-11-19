# Sparse set

`SparseSet<T, SS, SD = SS, SV = SD>` maps `u32` ids to values and keeps
the values packed in one slice, with
[storage parameters](storage.md#storage-parameter). See
[storage](storage.md) and [errors](errors.md).

Use it as an ECS component store: ids are entities (or a
[`Pool`](pool.md) handle's `index()`), and systems walk `values()` as a
plain slice with no holes, however sparse the ids are. Lookups by id are
an array access, with no hashing.

## Layout

Three blocks:

| Block | Holds | Size |
|---|---|---|
| `sparse` | `u32`: for each id, its position in the dense blocks | the id range: ids must be below its length |
| `ids` | `u32`: the id of each value | one per value |
| `values` | `T`: the values, packed | one per value |

`ids[sparse[id]] == id` exactly when the set holds `id`, so stale entries in
`sparse` need no clearing: creating and clearing the set never touches
it. Removing a value moves the last one into its place (and fixes that
id's entry), so the values stay packed and their order changes.

The sparse block costs 4 bytes per possible id: 400 KB for ids below
100,000. Use [`Map`](map.md) when ids are spread over a large range and
few of them are used.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(sparse, ids, values)`, `SparseSet::<T, Inline<S>, Inline<N>>::inline()` | O(1), `const` |
| sizes | `len()`, `is_empty()`, `capacity()`, `id_capacity()` | `capacity` is the smaller of `ids` and `values` |
| insert | `insert(id, value) -> Result<Option<T>, Full<T>>` | returns the value replaced; `Full` hands the value back |
| remove | `remove(id) -> Option<T>` | the last value moves into its place |
| lookup | `contains(id)`, `get(id)`, `get_mut(id)`, `index_of(id)` | `index_of` is the position in the dense slices |
| dense slices | `values()`, `values_mut()`, `ids()` | same order |
| iterate | `iter()`, `iter_mut()`, `&set` | `(id, &value)` in dense order |
| bulk | `clear()`, `retain(\|id, &mut value\| keep)` | O(values) |

## Guarantees

- No allocation beyond the storage, no panics, no `unsafe`.
- Every operation but `clear` and `retain` is O(1); with dynamic storage
  inserting is O(1) amortised.
- Dropping the set drops each value once.

## Speed

10,000 values with ids below 100,000, `u64` values (see
[benchmarks](benchmarks.md)):

| ns per value | `SparseSet` | `Map` (50% full) | std `HashMap` + `FastHasher` |
|---|---:|---:|---:|
| insert | 1.8 | 2.9 | 2.7 |
| get | 0.6 | 2.4 | 1.4 |
| iterate values | 0.1 | 0.7 | 0.4 |

## Storage

| Type | Memory |
|---|---|
| `SparseSet<T, Inline<S>, Inline<N>>` | inside the set: ids below `S`, up to `N` values; can be cloned |
| `SparseSet<T, Borrowed<'a>>` | three blocks provided by the caller |
| `SparseSet<T, Dynamic<A>>` | the sparse block grows to the highest id, the dense blocks with the number of values |

## Not included

- Sorting the dense slices (EnTT-style group ordering): sort a copy of
  `ids()` and look values up, or keep a `Vector` of ids in the order
  needed.
- Several component types sharing one id block (groups). Use one
  `SparseSet` per component type and walk the smallest one, looking the
  others up by id.
