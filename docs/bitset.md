# Bitset

`Bitset<'a>` is a fixed-capacity set of bits over user-provided memory: one
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
| create | `new(&'a mut [MaybeUninit<u64>]) -> Bitset<'a>` | all bits clear |
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
