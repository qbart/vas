# Map

`Map<'a, K, V, S = FastState>` is a fixed-capacity unordered map (hash table)
over user-provided memory. See [storage](storage.md) and [errors](errors.md).

## Design

- Open addressing with linear probing in one flat block: no per-entry
  allocation, cache-friendly probes.
- Each slot stores 31 bits of the key's hash, so most mismatches are rejected
  without comparing keys and nothing is rehashed on removal.
- Removal uses backward-shift deletion: there are no tombstones, so
  performance does not degrade after many inserts and removes.
- Iteration order is unspecified.

Robin Hood probing was tried and measured: it made lookups of missing keys
in a nearly full map much faster, but inserts about twice as slow and hits
slightly slower at the recommended fill, so plain linear probing was kept.

## Storage

The map needs one `map::Slot<K, V>` per entry (key, value and 4 bytes of
bookkeeping, plus padding):

```rust
use vas::map::Slot;
use vas::{Map, Memory};

let mut memory = Memory::<Slot<u32, f32>, 512>::new();
let mut map = Map::new(&mut memory);
```

Creating a map marks every slot empty, so it is O(capacity). At most
`u32::MAX` slots are used.

The map can be filled to 100%, but probes get longer as it fills, and a
lookup for a key that is **not** in the map suffers most: at 90% full it is
many times slower than at 50% (see [benchmarks](benchmarks.md)). Give the map
about twice the number of entries you expect.

## Hashing

`std`'s default hasher is not available in `no_std`, so vas ships
`hash::FastHasher`: a small multiplicative hasher, deterministic and fast.
It is **not** resistant to deliberately crafted colliding keys; do not use it
for keys from untrusted input.

Any `core::hash::BuildHasher` can be used instead:

```rust
let mut map = Map::with_hasher(&mut memory, my_build_hasher);
```

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(storage)`, `with_hasher(storage, S)` | O(capacity) |
| sizes | `len`, `capacity`, `is_empty`, `is_full` | O(1) |
| insert | `insert(K, V) -> Result<Option<V>, Full<(K, V)>>` | `Some(old)` when the key existed; returns both when full |
| lookup | `get(&Q)`, `get_mut(&Q)`, `contains_key(&Q)` | `K: Borrow<Q>`, so `&str` works for `String` keys |
| remove | `remove(&Q) -> Option<V>`, `remove_entry(&Q) -> Option<(K, V)>` | |
| clear | `clear()` | drops all entries |
| iterate | `iter`, `iter_mut`, `&map`, `&mut map` | `(&K, &V)` / `(&K, &mut V)`; cost is proportional to capacity |

## Guarantees

- No allocation, no panics.
- A failed insert leaves the map unchanged and returns the key and value.
- Entries still in the map are dropped when it is dropped.
