# Map

`Map<K, V, S, H = FastState>` is a fixed-capacity unordered map (hash table)
with a [storage parameter](storage.md#storage-parameter). See [storage](storage.md) and [errors](errors.md).

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
| create | `new(buffer)`, `with_hasher(buffer, H)`, `inline()`, `inline_with_hasher(H)` | O(capacity) |
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

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `Map<K, V, Inline<256>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `Map<K, V, Borrowed<'a>>` | provided by the caller, sized in `Slot<K, V>`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.

The hasher parameter is now called `H` and comes after the storage:
`Map<K, V, Borrowed<'a>, MyHasher>`.

## Growing

With `Map<K, V, Dynamic<A>>` the map moves to bigger storage when it gets
more than 3/4 full, so lookups stay fast without sizing it by hand. The
stored hash bits place the entries in the new storage; no key is hashed
again.

- `reserve(additional) -> Result<(), Full<()>>` makes room up front, at no
  more than 3/4 full.
- `insert` reports `Full` only when the allocator refuses *and* every slot
  is taken: a map that cannot grow any more fills up completely, like a
  fixed one.
- `capacity()` is the number of slots, so it is larger than the number of
  entries that fit before the next growth.
- `Map::<K, V, Dynamic<A>, H>::default()` when `A: Default` and
  `H: Default`.
- Growing takes a new block and gives the old one back. In an `Arena` the
  old block is left behind.
- About 6 to 9 ns per insert of a new `u32` key while growing from empty
  (a growing `std::HashMap` with the same hasher: about 10 ns at 2,000
  keys and 8 ns at 50,000, where vas takes 18 to 26 ns). `reserve` first
  to get the fixed-storage speed.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
