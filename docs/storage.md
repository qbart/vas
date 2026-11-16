# Storage

vas never allocates. Every container works on memory handed to it by the
library user, who decides where that memory lives (stack, static, heap, arena,
a mapped GPU buffer, ...).

## Convention

- A container either borrows its storage for a lifetime `'a`
  (`&'a mut [MaybeUninit<T>]`) or holds it inline; see "Storage parameter"
  below.
- The capacity of a container is the length of the storage slice. It never
  changes; containers do not grow.
- The container tracks which slots are initialised. Slots are initialised on
  insert and dropped on removal, `clear` or when the container is dropped.
- When the container is dropped the storage is returned to the user in an
  uninitialised state and can be reused.
- Contiguous containers expose their contents as `&[T]` / `&mut [T]` so the
  block can be passed on as-is (for example to a renderer).

## Storage parameter

Containers take a parameter `S` that says where their memory lives:
`Vector<T, S>`, `Pool<T, S>`, `Map<K, V, S>`, `Deque<T, S>`, `Heap<T, S>`,
`OrderedMap<K, V, S>`, `Bitset<S>`, `Tree<T, S>`, `BinaryTree<T, S>`.

Containers with two blocks of memory take two parameters, the second
defaulting to the first: `Graph<N, E, SN, SE>`, `StaticGraph<E, SO, SE>`,
`SpatialGrid<T, D, SC, SE>`, `Quadtree<T, SN, SE>` / `Octree<T, SN, SE>`,
`PathFinder<SN, SO>`. So `Graph<N, E, Borrowed<'a>>` borrows both blocks and
`Graph<N, E, Inline<64>, Inline<256>>` holds 64 nodes and 256 edges itself.

`Bvh` and `KdTree` arrange a slice of items that you own, so they stay
borrowed views with a lifetime.

With inline storage a struct can own all of its containers, with no
lifetime and no `Slot` types to name:

```rust
#[derive(Default)]
struct World {
    enemies: Pool<Enemy, Inline<64>>,
    names: Map<u32, &'static str, Inline<256>>,
    events: Deque<Event, Inline<32>>,
}
let world = World::default();
```

| `S` | Memory | Built from |
|---|---|---|
| `Borrowed<'a>` | provided by the caller, borrowed for `'a` | `&mut [MaybeUninit<T>]`, `&mut [MaybeUninit<T>; N]`, `&mut Memory<T, N>`; with the `alloc` feature also `&mut Vec<MaybeUninit<T>>` and `&mut Box<[MaybeUninit<T>]>` |
| `Inline<N>` | `N` slots inside the container | `inline()`, `default()`, or `Memory<T, N>` / `[MaybeUninit<T>; N]` passed by value |

The kind follows from what is passed to `new`: a reference borrows, a value
is stored inline. In type position write `Vector<f32, Borrowed<'a>>` or
`Vector<f32, Inline<1024>>`.

The pieces live in `vas::storage`:

- `Storage<T>`: implemented by `Borrowed` and `Inline`. Use it as a bound
  (`S: Storage<T>`) to accept a container over any storage.
- `Buffer<T>`: the slots themselves. An `unsafe` trait; containers rely on
  it returning the same slots every time.
- `IntoBuffer<T>`: what `new` accepts.

Costs of the parameter:

- `new`, `capacity`, `is_full` and similar are not `const` (they go through
  the storage trait). `inline()`, `len` and `is_empty` are.
- Inline containers are copied whole when moved, used or not. Keep large
  ones in borrowed storage.
- Inline `Map` and `Bitset` initialise all their slots on creation.

`Borrowed<'a>` keeps the slice as a pointer and a length, so generic code
over `Vector<T, Borrowed<'a>>` needs no `T: 'a` bound.

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
