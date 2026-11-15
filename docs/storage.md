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

## Example

```rust
use core::mem::MaybeUninit;
use vas::Vector;

let mut memory = [MaybeUninit::<u32>::uninit(); 4];
let mut v = Vector::new(&mut memory);
assert!(v.push(1).is_ok());
assert_eq!(v.as_slice(), &[1]);
```
