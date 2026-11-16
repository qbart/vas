# Vector

`Vector<T, S>` is a fixed-capacity, contiguous, growable-up-to-capacity array.
The storage parameter `S` says where its memory lives. See
[storage](storage.md) and [errors](errors.md).

| Type | Memory | Lifetime |
|---|---|---|
| `Vector<T, Borrowed<'a>>` | provided by the caller: a `Memory` block, an arena piece, any `&mut [MaybeUninit<T>]` | `'a` (`'static` for a static or heap arena) |
| `Vector<T, Inline<N>>` | room for `N` elements inside the vector | none |

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
| create | `new(buffer)` | a reference borrows (`&mut Memory`, `&mut [MaybeUninit<T>]`, arena piece); a value is stored inline (`Memory::new()`) |
| create inline | `Vector::<T, Inline<N>>::inline()`, `default()` | `inline` is `const`, so it can initialise a `static` |
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1) |
| push | `push(T) -> Result<(), Full<T>>` | returns the value when full |
| pop | `pop() -> Option<T>` | `None` when empty |
| bulk push | `extend_from_slice(&[T]) -> Result<(), Full<()>>` | `T: Copy`; all or nothing, one `memcpy` |
| resize | `resize(usize, T) -> Result<(), Full<()>>`, `resize_with(usize, FnMut() -> T)` | `resize` needs `T: Clone`; fails unchanged when above capacity |
| insert | `insert(usize, T) -> Result<(), InsertError<T>>` | O(n), keeps order; returns the value when full or out of bounds |
| remove, ordered | `remove(usize) -> Option<T>` | O(n), keeps order |
| remove, fast | `swap_remove(usize) -> Option<T>` | O(1), does not preserve order |
| shrink | `truncate(usize)`, `clear()` | drops the removed elements |
| view | `as_slice`, `as_mut_slice`, `Deref<Target = [T]>` | all slice methods available |

## Growing

The vector never grows on its own. To grow, obtain a bigger block and call
`move_into(buffer) -> Result<Vector, Full<Vector>>`. It accepts the same
things as `new`, so it can also change the kind of storage (borrowed to
inline or back). It moves every
element with one `memcpy` (any `T`), returns a vector over the new block and
leaves the old block free. When the new block is too small the original
vector is handed back unchanged.

## Choosing a storage

- **Inline** for small and medium vectors that belong to one struct. The
  struct needs no lifetime and no setup. Moving the struct copies all `N`
  slots, used or not, and a large one on the stack can overflow it.
- **Borrowed** for large blocks, for sizes chosen at run time, and for
  packing many containers into one budget with an [arena](arena.md).

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

## No `reserve`

The capacity is fixed by the storage the user passed in, so there is nothing
to reserve. Use `remaining()` to check for room up front.

## Guarantees

- No allocation, no panics.
- Elements still in the vector are dropped when it is dropped.
- Zero-sized types and zero-capacity storage are supported.
