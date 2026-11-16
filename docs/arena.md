# Arena

`Arena<'a>` hands out pieces of one block of memory, so that many containers
and values can be packed into it. See [storage](storage.md) and
[errors](errors.md).

Use it to give the whole game, a level, or one system a single fixed budget,
and to build structs of arrays and structs of structs without declaring a
separate block for every field.

## How it works

The arena keeps a position in the block. Each `alloc::<T>(count)` moves it
forward to the next address aligned for `T`, hands out the bytes for `count`
values as `&mut [MaybeUninit<T>]`, and moves past them. That slice is what
every vas container takes as its storage, so no container had to change.

- Pieces never overlap.
- `alloc` is a few arithmetic operations; nothing is searched.
- When a request does not fit it returns `Err(Full(()))` and the arena is
  unchanged.

```rust
use vas::{Arena, Memory, Pool, Vector};

let mut block = Memory::<u8, 4096>::new();
let arena = Arena::new(&mut block);

let mut positions = Vector::new(arena.alloc::<[f32; 2]>(100)?);
let mut enemies = Pool::new(arena.alloc::<vas::pool::Slot<Enemy>>(16)?);
```

## Sizes

Sizes are byte counts (`usize`). Write them with `vas::KB`, `vas::MB` and
`vas::GB` (powers of 1024): `MB(20)`, `KB(64)`. They are `const` functions,
so they also work in `StaticBlock<{ MB(20) }>`. A plain number of bytes is
still accepted everywhere.

## Lifetimes

Pieces live as long as the **block**, not as long as the arena. The arena is
only needed during setup and can be dropped afterwards.

**A block for the whole program.** `arena::StaticBlock<N>` is a block of `N`
bytes to declare as a `static` and take exactly once. Its pieces are
`'static`, so containers built on them need no lifetime parameter and can be
stored in any struct:

```rust
use vas::arena::StaticBlock;
use vas::{Arena, MB, Vector};

static GAME_MEMORY: StaticBlock<{ MB(20) }> = StaticBlock::new();

struct Particles {
    x: Vector<'static, f32>,
    y: Vector<'static, f32>,
}

let arena = Arena::new(GAME_MEMORY.take().ok_or("already taken")?);
let particles = Particles {
    x: Vector::new(arena.alloc(100_000)?),
    y: Vector::new(arena.alloc(100_000)?),
};
```

`StaticBlock` needs no allocator and no `unsafe` in your code. It is not
initialised, so it takes no space in the executable file. `take` returns
`None` on every call after the first, from any thread. It is available on
targets with atomic operations.

**A shorter-lived block** (a level, a frame). Structs holding the containers
then carry the block's lifetime: `Particles<'level>`.

A block can also come from your own allocator or the operating system: any
`&mut [MaybeUninit<u8>]` works.

## Blocks from the heap (`alloc` feature)

Off by default. Enable it with `vas = { version = "...", features = ["alloc"] }`.
The crate stays `no_std` and dependency free; the feature only uses the
global allocator of your program, and only when you call one of the two
functions below. Nothing else in vas allocates.

**For the whole program.** `Arena::from_heap(bytes)` takes the block once and
never returns it. Pieces are `'static`, exactly as with `StaticBlock`, and
the size can be chosen at run time:

```rust
let arena = Arena::from_heap(MB(20))?;
let particles = Particles {
    x: Vector::new(arena.alloc(100_000)?),
    y: Vector::new(arena.alloc(100_000)?),
};
```

**For a level or a loading screen.** `arena::HeapBlock::new(bytes)` owns the
block and returns it to the system when dropped. Structs built on it carry
its lifetime. Reuse it by building a new arena on it:

```rust
let mut block = HeapBlock::new(MB(1))?;
for level in levels {
    let arena = Arena::new(&mut block);
    let state = Level::new(&arena)?; // Level<'_>
}
```

Both return `Err(Full(()))` when the system cannot provide the block; they
never panic or abort. The block is aligned to 16 bytes.

## Freeing and starting over

- Individual pieces are never freed.
- Values stored with `alloc_value` are **never dropped**: the arena does not
  run destructors. Containers built on arena memory still drop their own
  contents when they are dropped.
- To start over, create a new `Arena` on the same block. Rust only allows
  that once everything that used the old pieces is gone, so stale pieces
  cannot be used by mistake. A `'static` block therefore can never be
  started over; use it for memory that lives as long as the game.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `Arena::new(&'a mut [MaybeUninit<u8>])` | O(1) |
| sizes | `capacity()`, `used()`, `remaining()` | bytes; `used` includes alignment padding |
| room for values | `alloc::<T>(count) -> Result<&'a mut [MaybeUninit<T>], Full<()>>` | uninitialised, aligned for `T` |
| one value | `alloc_value(T) -> Result<&'a mut T, Full<T>>` | returns the value when it does not fit; never dropped |
| copy of a slice | `alloc_copy(&[T]) -> Result<&'a mut [T], Full<()>>` | `T: Copy` |
| budget | `sub(bytes) -> Result<Arena<'a>, Full<()>>` | splits off part as its own arena |
| heap, forever | `Arena::from_heap(bytes) -> Result<Arena<'static>, Full<()>>` | `alloc` feature; never freed |
| heap, owned | `HeapBlock::new(bytes) -> Result<HeapBlock, Full<()>>`, `capacity()` | `alloc` feature; freed on drop; derefs to `[MaybeUninit<u8>]` |
| static block | `StaticBlock::<N>::new()`, `take() -> Option<&'static mut [MaybeUninit<u8>]>` | declare as a `static`; taken once |

## Guarantees

- No allocation unless `from_heap` or `HeapBlock::new` is called; no panics.
- Pieces are disjoint and correctly aligned.
- A failed request changes nothing.
