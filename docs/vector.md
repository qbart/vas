# Vector

`Vector<T, S>` is a contiguous array. The storage parameter `S` says where
its memory lives, and whether the capacity is fixed or grows. See
[storage](storage.md) and [errors](errors.md).

| Type | Memory | Lifetime |
|---|---|---|
| `Vector<T, Borrowed<'a>>` | provided by the caller: a `Memory` block, an arena piece, any `&mut [MaybeUninit<T>]` | `'a` (`'static` for a static or heap arena) |
| `Vector<T, Inline<N>>` | room for `N` elements inside the vector | none |
| `Vector<T, Dynamic<A>>` | taken from the allocator `A`; grows | that of `A` (none for `Global`) |

```rust
use vas::{Borrowed, Inline, Memory, Vector};

// Borrowed: pass a reference.
let mut memory = Memory::<f32, 1024>::new();
let mut v = Vector::new(&mut memory);

// Inline: the vector owns its memory, so it is a plain struct field.
struct Particles {
    x: Vector<f32, Inline<1024>>,
    y: Vector<f32, Inline<1024>>,
}
let particles = Particles { x: Vector::inline(), y: Vector::inline() };

// Any storage: be generic over `S`.
fn total<S: vas::storage::Storage<f32>>(values: &Vector<f32, S>) -> f32 {
    values.iter().sum()
}
```

Its contents are always one contiguous block, available as `&[T]`, which is
what makes it suitable for handing data to a renderer.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer)` | a reference borrows (`&mut Memory`, `&mut [MaybeUninit<T>]`, arena piece); a value is stored inline (`Memory::new()`); `Dynamic(allocator)` grows |
| create inline | `Vector::<T, Inline<N>>::inline()`, `default()` | `inline` is `const`, so it can initialise a `static` |
| create dynamic | `Vector::<T, Dynamic<A>>::default()` | when `A: Default`, e.g. `Global` |
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1); `capacity` is what fits without growing |
| reserve | `reserve(additional) -> Result<(), Full<()>>` | makes room for `additional` more elements; on fixed storage only checks |
| push | `push(T) -> Result<(), Full<T>>` | returns the value when there is no room (fixed and full, or the allocator refused) |
| pop | `pop() -> Option<T>` | `None` when empty |
| bulk push | `extend_from_slice(&[T]) -> Result<(), Full<()>>` | `T: Copy`; all or nothing, one `memcpy` |
| resize | `resize(usize, T) -> Result<(), Full<()>>`, `resize_with(usize, FnMut() -> T)` | `resize` needs `T: Clone`; fails unchanged when there is no room |
| insert | `insert(usize, T) -> Result<(), InsertError<T>>` | O(n), keeps order; returns the value when full or out of bounds |
| remove, ordered | `remove(usize) -> Option<T>` | O(n), keeps order |
| remove, fast | `swap_remove(usize) -> Option<T>` | O(1), does not preserve order |
| shrink | `truncate(usize)`, `clear()` | drops the removed elements |
| view | `as_slice`, `as_mut_slice`, `Deref<Target = [T]>` | all slice methods available |
| filter | `retain(FnMut(&T) -> bool)`, `retain_mut(FnMut(&mut T) -> bool)` | O(n), keeps order, one call per element front to back |
| remove repeats | `dedup()`, `dedup_by_key(FnMut(&mut T) -> K)`, `dedup_by(FnMut(&mut T, &mut T) -> bool)` | O(n); consecutive repeats only, so sort first to remove all duplicates |
| stable sort | `sort_stable()`, `sort_stable_by(cmp)`, `sort_stable_by_key(key)` | equal elements keep their order; no memory needed; O(n log n) comparisons, O(n log² n) moves |
| stable sort, faster | `sort_stable_in(&mut [MaybeUninit<T>], cmp)` | uses the scratch memory you pass, any size; with room for n / 2 elements O(n log n) moves; 3x to 4x faster than without |
| slice algorithms | `sort_unstable*`, `binary_search*`, `select_nth_unstable`, `reverse`, `rotate_*`, `contains`, `iter().position` | from the slice, through `Deref`

## Growing

With `Dynamic<A>` storage `push`, `insert`, `extend_from_slice`, `resize`
and `reserve` grow the vector through the allocator; see
[storage](storage.md) and [allocator](allocator.md). About 1.0 ns per push
of a `u64` against 0.8 with fixed storage and 0.9 for a growing `std::Vec`.

```rust
let mut block = Memory::<u8, 4096>::new();
let arena = Arena::new(&mut block);
let mut v = Vector::new(Dynamic(&arena)); // grows inside the arena
assert!(v.push(1u32).is_ok());

let mut heap = Vector::<u32, Dynamic>::default(); // `alloc` feature
```

With borrowed and inline storage the vector never grows on its own. To
grow, obtain a bigger block and call
`move_into(buffer) -> Result<Vector, Full<Vector>>`. It accepts the same
things as `new`, so it can also change the kind of storage (borrowed to
inline or back). It moves every
element with one `memcpy` (any `T`), returns a vector over the new block and
leaves the old block free. When the new block is too small the original
vector is handed back unchanged. `move_into(Dynamic(allocator))` turns a
fixed vector into a growing one.

## Choosing a storage

- **Inline** for small and medium vectors that belong to one struct. The
  struct needs no lifetime and no setup. Moving the struct copies all `N`
  slots, used or not, and a large one on the stack can overflow it.
- **Borrowed** for large blocks, for sizes chosen at run time, and for
  packing many containers into one budget with an [arena](arena.md).
- **Dynamic** when no sensible upper bound is known. Inserts can move all
  elements and take memory at a moment you do not choose; `reserve` up
  front to avoid that in hot code.

## Limits of this version

- `new`, `capacity`, `remaining`, `is_full` and `as_slice` are no longer
  `const` (they go through the storage trait). `inline()`, `len` and
  `is_empty` are.
- A turbofish needs both parameters: `Vector::<i16, _>::new(memory)`.
- `new` takes the exact types listed in [storage](storage.md). Something
  that only derefs to a slice, such as `&mut Vec<MaybeUninit<T>>`, must be
  passed as `&mut vec[..]`.

## Paths

The type lives in the public module `vas::vector` and is re-exported as
`vas::Vector`.

## Guarantees

- No panics. Memory is taken only through the allocator of `Dynamic<A>`;
  with borrowed and inline storage nothing is ever allocated.
- Elements still in the vector are dropped when it is dropped.
- Zero-sized types and zero-capacity storage are supported.
