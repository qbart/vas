# Tree map

`TreeMap<K, V, S>` is a map that keeps its entries sorted by key, as a
red-black tree, with a [storage parameter](storage.md#storage-parameter).
See [storage](storage.md) and [errors](errors.md).

It has the API of [`OrderedMap`](ordered_map.md). Use it when the map is
large and changes often: an event queue with cancellation, a sorted index
over thousands of entities, an interval or score table that is updated
every frame.

## TreeMap or OrderedMap

| | `OrderedMap` | `TreeMap` |
|---|---|---|
| insert, remove | O(n): entries are shifted | O(log n) |
| lookup, nearest key | O(log n), binary search | O(log n), tree descent |
| iterate | very fast: one sorted block | about 3 to 4 ns per entry: follows links |
| memory per entry | key + value | key + value + 12 bytes |
| sorted slice view | `as_slice()` | no |

Rule of thumb: `OrderedMap` up to about a thousand entries, or when the map
is built once and then read; `TreeMap` when inserts and removes on a large
map are frequent. At 16,384 entries a remove-then-insert costs about 420 ns
on `OrderedMap` and 66 ns on `TreeMap`; see [benchmarks](benchmarks.md).

## Layout

One block of nodes. A node is the key, the value and three `u32` links
(parent, left, right); the colour is the top bit of the parent link, so a
map holds at most 2^31 - 1 entries.

The nodes are packed at the front of the block in no particular order.
Removing an entry moves the last node into the freed slot and redirects the
three links that pointed at it, so there is no free list and no hole.

The tree is kept balanced by the red-black rules: the root is black, a red
node has no red child, and every path from the root down crosses the same
number of black nodes. The height is at most 2 log2(n + 1).

Lookups pick the child to follow without a branch: which way a search goes
is close to random, and a mispredicted branch per level would more than
double the cost on large maps.

## Storage

The block is sized in `Node<K, V>`:

```rust
use vas::tree_map::Node;
use vas::{Memory, TreeMap};

let mut memory = Memory::<Node<u32, Event>, 4096>::new();
let mut events = TreeMap::new(&mut memory);
```

| Type | Memory |
|---|---|
| `TreeMap<K, V, Inline<64>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `TreeMap<K, V, Borrowed<'a>>` | provided by the caller, sized in `Node<K, V>`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |
| `TreeMap<K, V, Dynamic<A>>` | taken from the allocator `A` as the map fills up |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept any kind are generic over `S`.

## API

| Operation | Signature | Notes |
|---|---|---|
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1) |
| insert | `insert(K, V) -> Result<Option<V>, Full<(K, V)>>` | `Some(old)` when the key existed; returns both when full |
| lookup | `get(&Q)`, `get_mut(&Q)`, `contains_key(&Q)` | `K: Borrow<Q>`, so `&str` works for `String` keys |
| remove | `remove(&Q) -> Option<V>`, `remove_entry(&Q) -> Option<(K, V)>` | |
| ends | `first()`, `last()`, `pop_first()`, `pop_last()` | all O(log n) |
| nearest | `at_or_before(&Q)`, `at_or_after(&Q)` | greatest key not above / smallest key not below |
| range | `range(a..b)` and the other range forms | entries with keys in the range; an inverted range is empty |
| iterate | `iter`, `iter_mut`, `&map`, `&mut map` | key order; double-ended |
| clear | `clear()` | drops all entries |
| filter | `retain(FnMut(&K, &mut V) -> bool)` | O(n log n), key order |
| reserve | `reserve(additional) -> Result<(), Full<()>>` | grows dynamic storage; only checks on fixed storage |
| clone | `clone()` | inline storage only; see [storage](storage.md#cloning) |

Differences from `OrderedMap`: there is no `as_slice`, and the iterators do
not know their exact length (use `len()`).

## Growing

With `TreeMap<K, V, Dynamic<A>>`, inserting a new key takes more memory
from the allocator `A` when the map is full. Links are indices, so growing
moves the block without touching the tree.

- `reserve(additional)` makes room up front.
- `TreeMap::<K, V, Dynamic<A>>::default()` when `A: Default`.

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).

## Guarantees

- No allocation of its own, no panics.
- A failed insert leaves the map unchanged and returns the key and value.
- Replacing the value of an existing key needs no room and keeps the key.
- Keys cannot be modified in place, so the order cannot be broken.
- No recursion: every operation uses constant stack space.

## Unsafe

The tree itself (insert, remove, rebalancing, lookups, `iter`) is safe
code. `iter_mut` is the one place with `unsafe`: it hands out `&mut V` to
one entry after another while it keeps reading the links of the same block,
which the borrow checker cannot express. It reads only link fields, through
raw pointers, and yields every entry at most once.

## Tests

- `tests/tree_map.rs`: the API against `std::collections::BTreeMap` over
  30,000 random operations, ranges, both iteration directions, drops,
  growing, cloning.
- Unit tests in `src/tree_map.rs` check the red-black rules, the parent
  links and the key order of the whole tree after every one of 20,000
  random operations, and that sorted input stays shallow.
