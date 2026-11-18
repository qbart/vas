# Sort

`vas::sort` sorts slices in place with vas's own code, not `core`'s. The
same sorts are methods on `Vector` (`sort_unstable*`, `sort_stable*`), and
they shadow the slice methods of the same name. A `Deque` is sorted through
`make_contiguous`. Any `&mut [T]` works: an array, a `Memory` block, an
arena piece.

| Function | Vector method | Order of equal elements | Memory |
|---|---|---|---|
| `unstable(&mut [T])`, `unstable_by(cmp)`, `unstable_by_key(key)` | `sort_unstable*` | may change | none (512 bytes of stack at most) |
| `stable(&mut [T])`, `stable_by(cmp)`, `stable_by_key(key)` | `sort_stable*` | kept | none |
| `stable_in(&mut [T], &mut [MaybeUninit<T>], cmp)` | `sort_stable_in` | kept | scratch from the caller, any size |

`T: Ord` for `unstable` and `stable`, `K: Ord` for the `_by_key` forms. The
key is computed on every comparison.

## Guarantees

These hold for every sort here, and are why vas has its own instead of
using `core`'s:

- **No panics.** A comparison that is not a total order (`partial_cmp` on
  floats meeting a NaN, a comparison whose answers change) leaves the
  elements in an unspecified order, but every element is still there
  exactly once and the call returns. `core`'s sorts may panic then.
- **The same result everywhere.** After an unstable sort, the order of
  equal elements depends only on the input and the vas version, not on the
  compiler, the target or the standard library (`core`'s changed with Rust
  1.81). Lockstep simulations and replays stay in sync. A test pins it;
  a vas release that changes it says so.
- **Panic safety.** If the comparison panics, every element is still in the
  slice exactly once and nothing is dropped twice.
- **O(n log n)** comparisons in the worst case. Sorted or reversed input
  takes O(n) for `unstable`, sorted input O(n) for `stable`.
- **No allocation.** Scratch memory comes only from the caller.

Zero-sized elements are left as they are.

## Unstable: quicksort

1. If the whole slice is already one ascending or strictly descending run,
   it is left or reversed: O(n).
2. Pivot: the median of three samples, or from 64 elements a median of
   medians of three spread over the slice.
3. Branch-free Lomuto partition: each element is compared once and moved
   twice, and the loop never branches on the comparison.
4. The larger part is sorted first, by recursion, while it is still in the
   cache; the smaller by the loop.
5. If the pivot is not above the pivot of the partition before, they are
   equal: the elements equal to it go first and are skipped. k distinct
   keys cost O(n log k).
6. After 2 log₂ n partitions a slice still left is heapsorted, so inputs
   built to defeat the pivot (median-of-three killers) stay O(n log n).
7. Short slices: up to 24 elements of up to 16 bytes are sorted without
   branching on the comparisons (below); larger elements, up to 20, by
   insertion.

### Short slices without branches

Comparisons in a short random slice are coin flips, and a mispredicted
branch costs about as much as sorting an element. So:

- runs of four are sorted into a 32-element stack buffer by a network of
  five comparisons that selects pointers instead of branching;
- runs are merged back and forth between the slice and the buffer;
- two runs of the same length merge from both ends at once, smallest to the
  front and largest to the back, in a loop of known length. If a bad
  comparison made the two ends take the same element, the end pointers do
  not meet; the merge is then redone as a plain copy of its input, which
  that pass did not write.

A pass that reads the buffer and writes the slice holds a guard that copies
the buffer back if the comparison panics.

## Stable: merge sort

Runs of 32 small elements (or 16 larger ones) are sorted as above, which is
stable, then merged pairwise. A merge whose shorter run fits in the scratch
memory moves that run out and merges back in one branch-free pass.
Otherwise it is done in place: split the longer run in the middle, binary
search where that element belongs in the other, rotate the two middle
pieces past each other, and repeat on both sides. Before the last merge the
parts already in place are trimmed off.

| | Comparisons | Moves | Memory |
|---|---|---|---|
| `stable*` | O(n log n) | O(n log² n) | O(log n) stack |
| `stable_in`, n / 2 scratch | O(n log n) | O(n log n) | the scratch |

## Speed

ns per element, `(u32, u32)` sorted by the first field, 8 different inputs
in turn (see [benchmarks](benchmarks.md)):

| | 1,000, ~630 keys | 100,000, 1,000 keys | 100,000, all distinct |
|---|---:|---:|---:|
| `unstable_by_key` | 5.8 | 4.5 | 9.6 |
| `core` `sort_unstable_by_key` | 6.0 | 4.8 | 9.7 |
| `stable_in`, n / 2 scratch | 10.6 | 22.1 | 22.1 |
| `stable_in`, 256 scratch | 10.9 | 25.8 | 26.1 |
| `stable_by_key`, no memory | 49 | 94 | 117 |
| std `sort_by_key` (allocates) | 7.1 | 4.8 | 11.0 |

The quicksort is as fast as `core`'s. The stable sort is 1.5x to 4.5x
slower than std's driftsort, which also detects existing runs and always
has a buffer. Use `unstable` unless equal elements must keep their order,
and often a tie-breaker in the key (`(depth, entity_id)`) makes the unstable
result exactly the stable one.

## Not included

- Sorting a `Deque` in place without `make_contiguous`: a ring is not a
  slice.
- `select_nth_unstable` and `binary_search`: still the slice's, through
  `Deref`.
