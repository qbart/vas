# Storage

vas never allocates. Every container works on memory handed to it by the
library user, who decides where that memory lives (stack, static, heap, arena,
a mapped GPU buffer, ...).

## Convention

- A container borrows its storage for a lifetime `'a`:
  `&'a mut [MaybeUninit<T>]`.
- The capacity of a container is the length of the storage slice. It never
  changes; containers do not grow.
- The container tracks which slots are initialised. Slots are initialised on
  insert and dropped on removal, `clear` or when the container is dropped.
- When the container is dropped the storage is returned to the user in an
  uninitialised state and can be reused.
- Contiguous containers expose their contents as `&[T]` / `&mut [T]` so the
  block can be passed on as-is (for example to a renderer).

## Unsafe

`MaybeUninit` storage avoids any initialisation cost and works for every `T`,
but reading from it requires `unsafe`. Rules:

- `unsafe_code` is denied crate-wide in `Cargo.toml`.
- A module that needs it opts in with `#![allow(unsafe_code)]` at its top.
- Every `unsafe` block carries a `// SAFETY:` comment stating the invariant
  it relies on.
- `make miri` runs the test suite under Miri.

## Memory

`Memory<T, N>` is a zero-cost wrapper around `[MaybeUninit<T>; N]` for inline
blocks (stack, `static`, struct field). `N` must be a compile-time constant:
without an allocator a block's size cannot be chosen at run time. For
run-time sizes, pass a `&mut [MaybeUninit<T>]` obtained from your own
allocator straight to the container.

## Sizes in bytes

Byte sizes are plain `usize`. The functions `vas::KB`, `vas::MB` and `vas::GB`
(powers of 1024) make them readable: `MB(20)`. They are `const`, so they work
in const generics too, e.g. `StaticBlock<{ MB(20) }>` or
`Memory::<u8, { KB(4) }>`. A size too large for a `usize` saturates to
`usize::MAX` instead of wrapping or panicking, so it is reported as `Full`. `MB` and `GB` do not exist on 16-bit targets.

## Arena

To pack many containers and values into one block, and to keep containers in
structs without a lifetime parameter, see [arena](arena.md). With the
optional `alloc` feature, `Arena::from_heap(bytes)` takes that block from the
heap in one call.

## Example

```rust
use vas::{Memory, Vector};

let mut memory = Memory::<f32, 1024>::new();
let mut v = Vector::new(&mut memory);
assert!(v.push(1.0).is_ok());
assert_eq!(v.as_slice(), &[1.0]);
```
