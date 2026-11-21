# LRU cache

`LruCache<K, V, SE, ST = SE, H = FastState>` keeps up to `capacity`
entries and, when full, evicts the one used least recently. It has
[storage parameters](storage.md#storage-parameter); see
[storage](storage.md) and [errors](errors.md).

Use it to keep the most useful of many things that are expensive to make
or load: decoded textures and sounds, generated terrain chunks, computed
paths, glyph rasters, query results.

## Memory

| Block | Holds | Size |
|---|---|---|
| `entries` | `lru::Entry<K, V>`: key, value, two links, a hash | `capacity` entries |
| `table` | `u32` indices into `entries` | rounded down to a power of two, at least `capacity * 4 / 3`; twice the capacity is faster |

Keys are stored once, so `K` needs only `Hash + Eq`, not `Clone`. The
capacity is fixed at creation; `Dynamic` storage takes both blocks from
the allocator then, and never grows them.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(entries, table, capacity) -> Result<Self, Full<()>>`, `with_hasher`, `LruCache::<K, V, Inline<E>, Inline<T>>::inline()` | `inline` takes the largest capacity its memory serves |
| sizes | `len()`, `is_empty()`, `capacity()` | |
| use | `get(&key)`, `get_mut(&key)` | the entry becomes the most recently used |
| look | `peek(&key)`, `contains(&key)`, `peek_lru()` | not a use |
| insert | `insert(key, value) -> Option<(K, V)>` | returns the replaced entry (same key), or the evicted one, or `None` |
| remove | `remove(&key) -> Option<V>`, `pop_lru() -> Option<(K, V)>` | |
| all | `iter()` (most recent first), `clear()` | |

Keys are looked up through `Borrow`, so a cache keyed by
[`String`](string.md) takes a `&str`.

## How it works

- The entries are packed in one block, with a doubly linked recency list
  through them (`u32` links, most recent at the head).
- The table maps keys to entries by open addressing with linear probing,
  kept at most 3/4 full. Removing from it shifts the rest of the probe run
  back (no tombstones), so lookups never slow down with use.
- Each table slot is a `u32`: the entry index in its low bits (as many as
  the table size needs; indices stay below 3/4 of it, so a slot is never
  the empty marker), and the low bits of the key's hash above it. A probe
  reads an entry only when those hash bits match. For tables of up to
  65,536 slots the hash bits include the entry's home slot, so removing
  shifts the probe run without reading any entry either.
- A full cache evicts by writing the new entry over the least recently
  used one, in place. `remove` moves the last entry into the hole.

`get`, `insert`, `remove` and `pop_lru` are O(1) on average.

## Speed

A cache of 1,024 entries in front of 2,048 keys, a `get` per key and an
`insert` on each miss (half of them): 19.8 ns per key, against 15.5 ns for
the `lru` crate (same hasher), which allocates a node per entry and uses
hashbrown's SIMD table.

Split up, a hit costs 6 ns against 3, and an eviction 25 ns against 13.
Until 0.46.0 each probe read the entry to compare hashes; keeping hash
bits in the table instead gained 4% here (20.6 → 19.8 ns), where every
entry fits in the L1 cache, and 15% with 128-byte values and 16,384
entries (31.5 → 26.9 ns, `lru` 19.7). What is left is probing: an
eviction walks four probe runs of unpredictable length (the new key, the
old key, the shift, the free slot), one slot per step, where hashbrown
checks 16 slots per SIMD step and almost always stops at the first. Its
speed also depends on where its nodes landed on the heap: after heavy
churn the same `lru` case measured 29 ns.

## Guarantees

- No allocation beyond the storage, no panics. `unsafe` is not used.
- A cache of capacity 0 keeps nothing: `insert` hands the entry back.
- Dropping the cache drops each key and value once.
