# Allocator

Containers with `Dynamic<A>` storage (see [storage](storage.md)) grow by
asking an allocator `A` for memory. The allocator is chosen and, if you
want, written by the library user, so allocation stays under your control:
a container never takes memory from anywhere else.

## The trait

`vas::allocator::Allocator` (re-exported as `vas::Allocator`) is an `unsafe`
trait with three methods, all taking `&self`:

| Method | Signature | Notes |
|---|---|---|
| allocate | `allocate(&self, Layout) -> Result<NonNull<u8>, Full<()>>` | a block of `layout.size()` bytes aligned to `layout.align()`, or `Full` when there is no memory |
| free | `unsafe free(&self, NonNull<u8>, Layout)` | takes back a block with the layout it was handed out with |
| grow | `unsafe grow(&self, NonNull<u8>, old: Layout, new: Layout) -> Result<NonNull<u8>, Full<()>>` | bigger block with the same first `old.size()` bytes; optional, the default allocates, copies and frees |

`Layout` is `core::alloc::Layout`. What an implementation must guarantee:

- A block is valid for its size, aligned, and overlaps no other live block.
- It stays at its address with its contents until freed or grown. Moving the
  allocator value does not invalidate it.
- A failed `grow` leaves the old block as it was.

Methods take `&self` so one allocator can serve many containers; state that
changes goes in a `Cell` or an atomic.

## Ready-made allocators

| Allocator | Memory | Notes |
|---|---|---|
| `Arena` | pieces of the arena's block | the last piece grows in place and is reclaimed when freed; other pieces are left behind until the arena is thrown away |
| `Global` | the program's global allocator | needs the `alloc` feature; the default `A` of `Dynamic` |
| `&A` | whatever `A` uses | how several containers share one allocator |

```rust
use vas::{Arena, Dynamic, Memory, Vector};

// Growing inside an arena (no `alloc` feature needed).
let mut block = Memory::<u8, 4096>::new();
let arena = Arena::new(&mut block);
let mut visible = Vector::new(Dynamic(&arena));
let mut hits = Vector::new(Dynamic(&arena));
assert!(visible.push(1u32).is_ok());
assert!(hits.push([0.0f32; 3]).is_ok());
```

```rust
// On the heap (`alloc` feature).
use vas::{Dynamic, Vector};

#[derive(Default)]
struct Particles {
    x: Vector<f32, Dynamic>,
    y: Vector<f32, Dynamic>,
}
let mut particles = Particles::default();
assert!(particles.x.push(1.0).is_ok());
```

## Failure

Nothing panics or aborts when memory runs out. The operation that needed to
grow reports `Full`, exactly as it does for a full fixed-capacity container,
and the container is unchanged.

## Guarantees

- Requests of zero bytes are never made by containers; an allocator may
  answer them with any aligned non-null address.
- A container frees its block when dropped, after dropping its elements.
- An empty container holds no memory.
