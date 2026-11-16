# Bitset

`Bitset<S>` is a fixed-capacity set of bits with a [storage parameter](storage.md#storage-parameter): one
bit per possible member. See [storage](storage.md) and [errors](errors.md).

Use it as a set of small integers: visited cells in pathfinding, entity or
component masks, free-slot tracking.

## Storage

The memory is a block of `u64` words, 64 bits each. `bitset::words(bits)`
gives the number of words needed:

```rust
use vas::bitset::words;
use vas::{Bitset, Memory};

let mut memory = Memory::<u64, { words(1000) }>::new();
let mut visited = Bitset::new(&mut memory);
```

The capacity is always a multiple of 64 (1024 bits above). Creating a bitset
clears every word, so it is O(words).

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer)`, `Bitset::<Inline<N>>::inline()` (`N` in 64-bit words) | all bits clear |
| sizes | `capacity()`, `count()`, `is_empty()`, `is_full()` | `count` is a popcount over all words |
| test | `contains(i) -> bool` | `false` when out of range |
| set | `insert(i) -> Result<bool, OutOfBounds>` | `true` when the bit was clear before |
| clear | `remove(i) -> bool` | `true` when the bit was set before; `false` when out of range |
| assign | `set(i, bool) -> Result<(), OutOfBounds>` | |
| flip | `toggle(i) -> Result<bool, OutOfBounds>` | returns the new value |
| all bits | `clear()`, `fill()` | |
| search | `first_set()`, `first_clear() -> Option<usize>` | lowest index |
| combine | `union_with`, `intersect_with`, `difference_with` | in place; sets may differ in size |
| iterate | `iter()`, `&bitset` | indices of set bits, lowest first |
| raw | `as_words() -> &[u64]` | |

## Guarantees

- No allocation, no panics. Writing to an out-of-range index is an
  `OutOfBounds` error; reading one reports "not set".

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `Bitset<Inline<{ words(1000) }>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `Bitset<Borrowed<'a>>` | provided by the caller, sized in `u64` words: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.

For inline storage `N` counts 64-bit words, not bits.

## Growing

With `Bitset<Dynamic<A>>` the set starts with no bits and grows to hold the
highest bit that is set.

- `insert`, `set(index, true)` and `toggle` grow the set to reach `index`.
  They report `OutOfBounds` only when the allocator refuses.
- `set(index, false)`, `remove` and `contains` past the end need no memory:
  such bits are clear.
- `union_with` grows the set to hold the other set's highest bit.
- The capacity at least doubles on each growth; new bits are clear.
- `Bitset::<Dynamic<A>>::default()` when `A: Default` (e.g. `Global`).

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
