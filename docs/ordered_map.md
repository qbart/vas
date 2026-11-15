# Ordered map

`OrderedMap<'a, K, V>` is a fixed-capacity map that keeps its entries sorted
by key, over user-provided memory. See [storage](storage.md) and
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
more than they change. For large maps with frequent inserts and removes a
balanced tree is the better structure (planned: red-black tree).

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
