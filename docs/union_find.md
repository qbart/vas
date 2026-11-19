# Union-find

`UnionFind<S>` splits elements `0..len()` into disjoint sets, with a
[storage parameter](storage.md#storage-parameter). See [storage](storage.md)
and [errors](errors.md).

Use it for connectivity that only grows:

- maze and dungeon generation: knock down a wall only between rooms that
  are not connected yet;
- islands, regions, territory: which tiles belong together;
- Kruskal's minimum spanning tree: roads or corridors joining every room
  as cheaply as possible.

Sets are never split. To remove connections, rebuild (`reset` and union
again) or use a [`Graph`](graph.md) with [`Traversal`](traversal.md).

## Memory

One `union_find::Link` (8 bytes: parent and set size) per element:
`Memory::<Link, N>` holds `N` elements.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer)`, `UnionFind::<Inline<N>>::inline()` | empty, O(1); `inline` is `const` |
| elements | `reset(count)`, `add() -> Result<u32, Full<()>>`, `clear()` | each new element is a set of its own |
| sizes | `len()`, `is_empty()`, `capacity()`, `set_count()` | `set_count` is the number of distinct sets |
| join | `union(a, b) -> Result<bool, OutOfBounds>` | `true` when the sets were apart |
| ask | `same(a, b) -> bool`, `find(x) -> Option<u32>`, `set_size(x)` | `find` returns the set's root; shortens paths |
| ask, shared | `root(x) -> Option<u32>` | like `find` with `&self`, without shortening |

## How it works

Each element points at a parent; a root points at itself and stands for
its set. `union` puts the root of the smaller set under the root of the
larger (union by size), so no tree is deeper than log₂ n. `find` makes every
element on its way point at its grandparent (path halving), which flattens
the trees as they are used. Together they make `union`, `same` and `find`
nearly O(1) amortised (inverse Ackermann). Nothing recurses.

## Guarantees

- No allocation beyond the storage, no panics, no `unsafe`. Elements past
  the end are `OutOfBounds` for `union`, `None` for `find`, `false` for
  `same`.
- `reset` fails without changes when the elements do not fit.

## Storage

| Type | Memory |
|---|---|
| `UnionFind<Inline<N>>` | inside the structure: no lifetime; can be cloned |
| `UnionFind<Borrowed<'a>>` | provided by the caller |
| `UnionFind<Dynamic<A>>` | grows with `add` and `reset` |
