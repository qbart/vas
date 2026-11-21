# Map

`Map<K, V, S, H = FastState>` is an unordered map (hash table) with a
[storage parameter](storage.md#storage-parameter). See [storage](storage.md) and [errors](errors.md).

## Design

- Open addressing with linear probing: no per-entry allocation.
- Two blocks of memory of the same length. One holds the keys and values
  (`map::Slot<K, V>`, exactly the size of a key and a value). The other
  holds one byte per slot, the *tag*: 0 for an empty slot, otherwise the
  top bit plus 7 bits of the key's hash.
- A lookup reads 8 tags at once and compares a key only when its tag
  matches. The tags of 64 slots share one cache line, so a probe that
  walks past many slots touches little memory, and a missing key is
  usually rejected without looking at any key.
- Removal uses backward-shift deletion: there are no tombstones, so
  performance does not degrade after many inserts and removes. The
  entries that follow the removed one are hashed again to decide whether
  they move up.
- Iteration order is unspecified.

Earlier versions kept a 4-byte tag next to every key and value in one
block. That needed 12 bytes per `u32 -> u32` entry (now 9) and scanned slot
by slot, so it was slow above half full and on maps larger than the CPU
cache. See [benchmarks](benchmarks.md).

Robin Hood probing was tried and measured on the old layout: it made
lookups of missing keys in a nearly full map much faster, but inserts about
twice as slow, so it was not kept.

## Storage

The map needs one `map::Slot<K, V>` and one `u8` per entry:

```rust
use vas::map::Slot;
use vas::{Map, Memory};

let mut slots = Memory::<Slot<u32, f32>, 512>::new();
let mut tags = Memory::<u8, 512>::new();
let mut map = Map::new(&mut slots, &mut tags);
```

From an arena: `Map::new(arena.alloc(512)?, arena.alloc(512)?)`.

The capacity is the length of the shorter block, and at most `u32::MAX`.
Creating a map marks every slot empty, so it is O(capacity) (one byte per
slot).

The map can be filled to 100%. Finding keys that are in the map stays fast
up to about 90% full. Looking up keys that are **not** in the map slows
down past 3/4 full (see [benchmarks](benchmarks.md)); leave a quarter of
the slots free when that is common.

| Type | Memory |
|---|---|
| `Map<K, V, Inline<256>>` | both blocks inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `Map<K, V, Borrowed<'a>>` | provided by the caller: pass two `&mut Memory<..>`, arena pieces or any `&mut [MaybeUninit<..>]` to `new` |
| `Map<K, V, Dynamic<A>>` | taken from an allocator, grows; see below |

Passing two `Memory` blocks to `new` by value also gives inline storage.
Functions that should accept any kind are generic over
`S: Storage<Slot<K, V>> + Storage<u8>`.

The hasher parameter `H` comes after the storage:
`Map<K, V, Borrowed<'a>, MyHasher>`.

## Hashing

`std`'s default hasher is not available in `no_std`, so vas ships
`hash::FastHasher`: a small multiplicative hasher, deterministic and fast.
It is **not** resistant to deliberately crafted colliding keys; do not use it
for keys from untrusted input.

Any `core::hash::BuildHasher` can be used instead:

```rust
let mut map = Map::with_hasher(&mut slots, &mut tags, my_build_hasher);
```

The upper 32 bits of the hash choose the slot and 7 of the lower bits make
the tag, so a hasher should mix its input into both halves.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(slots, tags)`, `with_hasher(slots, tags, H)`, `inline()`, `inline_with_hasher(H)` | O(capacity) |
| sizes | `len`, `capacity`, `is_empty`, `is_full` | O(1) |
| insert | `insert(K, V) -> Result<Option<V>, Full<(K, V)>>` | `Some(old)` when the key existed; returns both when full |
| lookup | `get(&Q)`, `get_mut(&Q)`, `contains_key(&Q)` | `K: Borrow<Q>`, so `&str` works for `String` keys |
| remove | `remove(&Q) -> Option<V>`, `remove_entry(&Q) -> Option<(K, V)>` | hashes the entries right after the removed one |
| reserve | `reserve(additional) -> Result<(), Full<()>>` | grows dynamic storage; only checks fixed storage |
| clear | `clear()` | drops all entries |
| iterate | `iter`, `iter_mut`, `&map`, `&mut map` | `(&K, &V)` / `(&K, &mut V)`; cost is proportional to capacity |
| filter | `retain(FnMut(&K, &mut V) -> bool)` | O(capacity); one call per entry, in no particular order |

## Guarantees

- No allocation unless the storage is `Dynamic`, no panics.
- A failed insert leaves the map unchanged and returns the key and value.
- Entries still in the map are dropped when it is dropped.

## Growing

With `Map<K, V, Dynamic<A>>` the map moves to bigger storage, twice the
size, when it gets more than 7/8 full, as `std`'s `HashMap` does. Pass the
allocator twice, once per block:

```rust
let mut map = Map::new(Dynamic(&arena), Dynamic(&arena));
```

- Every key is hashed again when the map moves.
- `reserve(additional) -> Result<(), Full<()>>` makes room up front, at no
  more than 7/8 full.
- A growing map is between 7/16 and 7/8 full. Near 7/8 a lookup of a key
  that is **not** in the map costs up to 7x as much as in a half-full map
  (2 ns against up to 15 ns at 229,000 keys; `std` pays the same, 1.4x
  less). When misses are common, `reserve` twice the entries expected.
  Until 0.45.0 the map grew at 3/4 full: misses stayed near 2 ns, but
  filling took up to 2x as long and up to 2x the memory.
- `insert` reports `Full` only when the allocator refuses *and* every slot
  is taken: a map that cannot grow any more fills up completely, like a
  fixed one.
- `capacity()` is the number of slots, so it is larger than the number of
  entries that fit before the next growth.
- `Map::<K, V, Dynamic<A>, H>::default()` when `A: Default` and
  `H: Default`.
- Growing takes two new blocks and gives the old ones back. In an `Arena`
  the old blocks are left behind.
- About 8 ns per insert of a new `u32` key while growing from empty to
  2,000 keys and 16 ns to 50,000 (a growing `std::HashMap` with the same
  hasher: 10 ns and 8 ns). `reserve` first to get the fixed-storage speed
  of about 3 ns.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
