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
  takes O(n) for `unstable`, sorted input O(n) for `stable` (and reversed
  input too when no two elements are equal).
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

## Stable: quicksort through scratch, then merges

Input that is one run already (ascending, or strictly descending, which is
reversed) is left after one pass.

With scratch memory longer than 64 elements, the slice is cut into pieces
as long as the scratch, and each piece is sorted by a **stable quicksort**:

- a pivot is chosen as in the unstable sort;
- every element is copied once into the scratch, those less than the pivot
  from its start, the others from its end backwards, with no branch on the
  comparison; then all are copied back, the second group reversed, so both
  keep their order. Until the copy back the slice is only read, so a
  panicking comparison leaves every element in it;
- when a pivot equals the one of the step before (which is the smallest
  element of its part), all elements equal to it are taken out in one
  pass: k distinct keys cost O(n log k), not O(n log n). The earlier pivot
  is found by its position, tracked through the partition, never by a copy
  of it;
- slices of 32 small elements (16 larger ones) go to the small sort below;
  past 2 log₂ n steps deep it falls back to merging, so the sort stays
  O(n log n) whatever the input or the comparison.

The sorted pieces, and with less scratch runs of 32 small elements (16
larger ones) sorted as above, are then merged pairwise. A merge whose
shorter run fits in the scratch memory moves that run out and merges back
in one branch-free pass. Otherwise it is done in place: split the longer
run in the middle, binary search where that element belongs in the other,
rotate the two middle pieces past each other, and repeat on both sides.
Before each merge the parts already in place are trimmed off.

| | Comparisons | Moves | Memory |
|---|---|---|---|
| `stable*` | O(n log n) | O(n log² n) | O(log n) stack |
| `stable_in`, n / 2 scratch | O(n log n) | O(n log n) | the scratch |
| `stable_in`, n scratch | O(n log n), O(n log k) for k keys | the same | the scratch, O(log n) stack |

## Speed

ns per element, `(u32, u32)` sorted by the first field, 8 different inputs
in turn (see [benchmarks](benchmarks.md)):

| | 1,000, ~630 keys | 100,000, 1,000 keys | 100,000, all distinct |
|---|---:|---:|---:|
| `unstable_by_key` | 5.8 | 4.5 | 9.8 |
| `core` `sort_unstable_by_key` | 5.5 | 4.2 | 9.1 |
| `stable_in`, n scratch | 8.1 | 5.6 | 12.6 |
| `stable_in`, n / 2 scratch | 8.9 | 7.8 | 13.8 |
| `stable_in`, 256 scratch | 9.9 | 26.9 | 27.1 |
| `stable_by_key`, no memory | 48 | 95 | 117 |
| std `sort_by_key` (allocates n) | 7.4 | 4.8 | 11.1 |

The quicksort is as fast as `core`'s. With scratch for every element the
stable sort is 1.1x to 1.2x slower than std's driftsort (which allocates
that much); with half, 1.25x to 1.6x. Before 0.47.0 it only merged, and was
2x to 4.5x slower: a merge step cannot start before the comparison of the
step before, while a partition's comparisons with the pivot are
independent. With little scratch most merges are done in place and it
stays near 27 ns. Use `unstable` unless equal elements must keep their
order, and often a tie-breaker in the key (`(depth, entity_id)`) makes the
unstable result exactly the stable one.

## Selection

`select_nth_unstable(items, i)` (and `_by`, `_by_key`; the same methods on
`Vector`) puts the element a sort would put at `i` in place, no greater
element before it and no smaller one after, and returns the three parts.
It finds a median, the k nearest or the top k scores without sorting.

- `None`, with nothing changed, when `i` is out of range; `core`'s panics.
- Quickselect with the quicksort's pivot, partition and equal-pivot
  skipping. After 8 partitions that keep more than 7/8 of the slice, every
  pivot is a median of medians of five, so the worst case is O(n), also
  against an adversary that picks values as they are compared (a test
  runs McIlroy's): about 14 comparisons per element there, against about 3
  on random input.
- The same guarantees as the sorts: no panics on a bad comparison, every
  element kept if it panics, the same result on every target.

0.6 ns per element for the median of 100,000 `u32`, against 0.8 for
`core`'s.

## Not included

- Sorting a `Deque` in place without `make_contiguous`: a ring is not a
  slice.
- Partial sorting (the k smallest, in order): select the kth, then sort
  the part before it.
