# Pool

`Pool<T, S>` is a fixed-capacity store of values addressed by generational
handles, with a [storage parameter](storage.md#storage-parameter). See [storage](storage.md) and
[errors](errors.md).

Use it for things that are created and destroyed at run time and referred to
from elsewhere: entities, resources, particles, UI nodes.

## Handles

`Handle<T>` is `Copy`, 8 bytes (slot index + generation, both `u32`). When a
value is removed its handle goes stale: every lookup returns `None`, even
after the slot has been reused for another value. Handles are typed by `T`
but are only meaningful for the pool that issued them.

`handle.index()` is the slot index: a dense number below the capacity, reused
after removal. Use it to keep extra per-value data in a parallel array.

## Storage

The pool needs one `pool::Slot<T>` per value (the value plus 8 bytes of
bookkeeping):

```rust
use vas::pool::Slot;
use vas::{Memory, Pool};

let mut memory = Memory::<Slot<u32>, 256>::new();
let mut pool = Pool::new(&mut memory);
```

Creating a pool is O(1); slots are set up lazily on first use. At most
`u32::MAX` slots are used.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `new(buffer)`, `Pool::<T, Inline<N>>::inline()` | O(1) |
| sizes | `len`, `capacity`, `is_empty`, `is_full` | O(1) |
| insert | `insert(T) -> Result<Handle<T>, Full<T>>` | O(1); returns the value when full |
| remove | `remove(Handle<T>) -> Option<T>` | O(1); `None` for a stale handle |
| lookup | `get`, `get_mut`, `contains` | O(1); `None` / `false` for a stale handle |
| clear | `clear()` | drops all values, all handles go stale |
| iterate | `iter`, `iter_mut`, `&pool`, `&mut pool` | yields `(Handle<T>, &T)` in slot order; cost is proportional to the slots ever used, not to `len` |

## Guarantees

- No allocation, no panics.
- Values never move while they are in the pool.
- Freed slots are reused before fresh ones.
- Live values are dropped when the pool is dropped.
- A slot is retired after 2^31 reuses rather than letting its generation wrap
  around, so a stale handle can never match a newer value. Each retirement
  reduces the usable capacity by one.

## Storage

The parameter `S` says where the memory lives; see
[storage](storage.md#storage-parameter).

| Type | Memory |
|---|---|
| `Pool<T, Inline<64>>` | inside the container: no lifetime, a plain struct field. Create with `inline()` or `default()`; `inline()` is `const` |
| `Pool<T, Borrowed<'a>>` | provided by the caller, sized in `Slot<T>`: pass `&mut Memory<..>`, an arena piece or any `&mut [MaybeUninit<..>]` to `new` |

Passing a `Memory` to `new` by value also gives inline storage. Functions
that should accept either kind are generic over `S`.

## Growing

With `Pool<T, Dynamic<A>>` an insert that finds no free slot takes more
memory from the allocator `A` instead of failing. Handles stay valid: a
slot's index never changes when the pool grows. Freed slots are reused
before the pool grows again.

- `reserve(additional) -> Result<(), Full<()>>` makes room up front.
- `insert` reports `Full` only when the allocator refuses.
- `Pool::<T, Dynamic<A>>::default()` when `A: Default` (e.g. `Global`).

See [storage](storage.md#dynamic-storage) and [allocator](allocator.md).
