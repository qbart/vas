# Storage

vas never allocates on its own. Every container works on memory handed to
it by the library user, who decides where that memory lives (stack, static,
heap, arena, a mapped GPU buffer, ...), or on memory it takes from an
[allocator](allocator.md) the user picked.

## Convention

- A container borrows its storage for a lifetime `'a`
  (`&'a mut [MaybeUninit<T>]`), holds it inline, or takes it from an
  allocator; see "Storage parameter" below.
- The capacity of a container is the length of the storage slice. With
  borrowed and inline storage it never changes. Only dynamic storage grows.
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
| `Dynamic<A>` | taken from the allocator `A`, grows | `Dynamic(allocator)`, or `default()` when `A: Default` |

The kind follows from what is passed to `new`: a reference borrows, a value
is stored inline, `Dynamic(allocator)` grows. In type position write
`Vector<f32, Borrowed<'a>>`, `Vector<f32, Inline<1024>>` or
`Vector<f32, Dynamic<A>>`.

## Dynamic storage

`Dynamic<A>` is the storage that grows. The container starts with no memory
and asks the allocator `A` for more when it runs out; see
[allocator](allocator.md) for the trait and the ready-made allocators
(`Arena`, `Global`, `&A`).

- Inserting fails only when the allocator refuses. The error is the same
  `Full` as for fixed storage, and nothing panics.
- The capacity at least doubles on each growth (4 slots the first time), so
  a run of pushes copies each element a constant number of times on
  average. If the allocator cannot give the doubled size, exactly the
  needed size is tried before giving up.
- Growing moves the elements, so references into the container cannot be
  held across an insert (the borrow checker enforces this).
- The memory is given back when the container is dropped.
- With the `alloc` feature `A` defaults to `Global`: `Vector<f32, Dynamic>`.

**Which containers grow:** all that take a storage parameter. Each
container's page has a "Growing" section with the details.

| Container | How it grows |
|---|---|
| `Vector`, `Heap`, `OrderedMap` | the block is extended, in place when the allocator can |
| `Deque` | the block is extended, then the ring is put back in order |
| `Pool`, `Tree`, `BinaryTree`, `Graph` | the block is extended; handles and ids stay valid |
| `Map` | moves to two new blocks (entries and tags) when more than 3/4 full |
| `Bitset` | grows to hold the highest bit set |
| `StaticGraph` | `build` takes exactly what the graph needs |
| `SpatialGrid`, `Quadtree` / `Octree` | cells / nodes and entries grow as points are added |
| `PathFinder` | node memory grows to the highest node touched; the open list never fills |

`reserve` makes room up front on `Vector`, `Pool`, `Map`, `Deque`, `Heap`,
`OrderedMap` and `PathFinder`. On fixed storage it only checks.

A struct can own all of its containers without sizing any of them (here
with the `alloc` feature and the global allocator):

```rust
#[derive(Default)]
struct World {
    enemies: Pool<Enemy, Dynamic>,
    names: Map<u32, String, Dynamic>,
    events: Deque<Event, Dynamic>,
    roads: Graph<Town, f32, Dynamic>,
    finder: PathFinder<Dynamic>,
}
```

Fixed storage pays nothing for this: whether a buffer can grow is known at
compile time, and the benchmarks for borrowed storage are unchanged.

The pieces live in `vas::storage`:

- `Storage<T>`: implemented by `Borrowed`, `Inline` and `Dynamic`. Use it
  as a bound (`S: Storage<T>`) to accept a container over any storage.
- `Buffer<T>`: the slots themselves. An `unsafe` trait; containers rely on
  it returning the same slots every time, and on `grow(min)` keeping their
  contents. `replace(min, transfer)` switches to a new block and lets the
  container move its contents across, or cancel by returning `false`
  (used by `Map`, which switches two blocks together). Fixed buffers answer
  both with `false`; `GROWS` says at compile time whether a buffer can
  grow at all.
- `IntoBuffer<T>`: what `new` accepts.

Costs of the parameter:

- `new`, `capacity`, `is_full` and similar are not `const` (they go through
  the storage trait). `inline()`, `len` and `is_empty` are.
- Inline containers are copied whole when moved, used or not. Keep large
  ones in borrowed storage.
- Inline `Map` and `Bitset` initialise all their slots on creation.

`Borrowed<'a>` keeps the slice as a pointer and a length, so generic code
over `Vector<T, Borrowed<'a>>` needs no `T: 'a` bound.

## Cloning

A container with `Inline<N>` storage is `Clone` when its elements are (and,
for `Map` and `Heap`, the hasher or the order). The copy has the same
capacity and its own values.

```rust
let mut squad = Vector::<Unit, Inline<8>>::inline();
let snapshot = squad.clone();
```

- Containers that hand out handles or ids (`Pool`, `Tree`, `BinaryTree`,
  `Graph`, `SpatialGrid`, `Quadtree`/`Octree`) keep every value in its slot
  with its generation: a handle from the original refers to the same value
  in the copy, and a stale handle stays stale.
- `Map` keeps every entry in its slot, so cloning does not hash anything.
- `PathFinder` copies the result of its last search.
- Cloning costs one `clone` per element; no memory is requested.
- If an element's `clone` panics, the elements cloned so far are dropped.

Containers with `Borrowed` storage are not `Clone`: the copy would need
memory of its own, and only the user can provide it. Containers with
`Dynamic` storage are not `Clone` either: `clone` cannot report that the
allocator refused. To copy one, create a container on new memory and insert
the elements.

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
