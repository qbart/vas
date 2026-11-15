# Vector

`Vector<'a, T>` is a fixed-capacity, contiguous, growable-up-to-capacity array
over user-provided memory. See [storage](storage.md) and [errors](errors.md).

Its contents are always one contiguous block, available as `&[T]`, which is
what makes it suitable for handing data to a renderer.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(&'a mut [MaybeUninit<T>]) -> Vector<'a, T>` | empty; capacity = slice length |
| sizes | `len`, `capacity`, `remaining`, `is_empty`, `is_full` | O(1) |
| push | `push(T) -> Result<(), Full<T>>` | returns the value when full |
| pop | `pop() -> Option<T>` | `None` when empty |
| bulk push | `extend_from_slice(&[T]) -> Result<(), Full<()>>` | `T: Copy`; all or nothing, one `memcpy` |
| resize | `resize(usize, T) -> Result<(), Full<()>>`, `resize_with(usize, FnMut() -> T)` | `resize` needs `T: Clone`; fails unchanged when above capacity |
| remove | `swap_remove(usize) -> Option<T>` | O(1), does not preserve order |
| shrink | `truncate(usize)`, `clear()` | drops the removed elements |
| view | `as_slice`, `as_mut_slice`, `Deref<Target = [T]>` | all slice methods available |

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
