# Ordered map

`OrderedMap<K, V, S>` is a fixed-capacity map that keeps its entries sorted
by key, with a [storage parameter](storage.md#storage-parameter). See [storage](storage.md) and
[errors](errors.md).

Use it when order matters: keyframes, timed events, sorted lookup tables,
"the nearest key at or below X", range queries.

## Layout

A sorted `Vector` of `(K, V)` pairs: one contiguous block, no links.

| Operation | Cost |
|---|---|
| lookup, nearest key, start of a range | O(log n), binary search |
| insert, remove | O(n): entries after the position are shifted |
| iterate | O(n), in key order, cache-friendly |

This beats a tree for small to medium maps and for maps that are read much
more than they change. For large maps with frequent inserts and removes use
[`TreeMap`](tree_map.md), a red-black tree with the same API.

## Storage

No slot type is needed; the block holds the pairs directly:

```rust
use vas::{Memory, OrderedMap};

let mut memory = Memory::<(u32, Keyframe), 64>::new();
let mut keyframes = OrderedMap::new(&mut memory);
```

## API

| Operation | Signature | Notes |
|---|---|---|
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1) |
| insert | `insert(K, V) -> Result<Option<V>, Full<(K, V)>>` | `Some(old)` when the key existed; returns both when full |
| lookup | `get(&Q)`, `get_mut(&Q)`, `contains_key(&Q)` | `K: Borrow<Q>`, so `&str` works for `String` keys |
| remove | `remove(&Q) -> Option<V>`, `remove_entry(&Q) -> Option<(K, V)>` | |
| ends | `first()`, `last()`, `pop_first()`, `pop_last()` | `pop_last` is O(1), `pop_first` O(n) |
| nearest | `at_or_before(&Q)`, `at_or_after(&Q)` | greatest key not above / smallest key not below |
| range | `range(a..b)` and the other range forms | entries with keys in the range; an inverted range is empty |
| iterate | `iter`, `iter_mut`, `&map`, `&mut map` | key order; double-ended |
| view | `as_slice() -> &[(K, V)]` | sorted, contiguous |
| clear | `clear()` | drops all entries |

## Guarantees

- No allocation, no panics, no `unsafe` of its own.
- A failed insert leaves the map unchanged and returns the key and value.
- Keys cannot be modified in place, so the order cannot be broken.

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `OrderedMap<K, V, Inline<64>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `OrderedMap<K, V, Borrowed<'a>>` | provided by the caller, sized in `(K, V)`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.

## Growing

With `OrderedMap<K, V, Dynamic<A>>`, inserting a new key takes more memory
from the allocator `A` when the map is full.

- `reserve(additional) -> Result<(), Full<()>>` makes room up front.
- `OrderedMap::<K, V, Dynamic<A>>::default()` when `A: Default`.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
