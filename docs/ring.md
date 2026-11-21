# Ring

`vas::sync::Ring<T, S>` is a first-in, first-out ring buffer between
exactly two threads, one that pushes and one that pops: audio buffers, IO
completions, commands from the game thread to the render thread. With one
writer and one reader each side owns its index, so `push` and `pop` are a
plain write or read and one atomic store, with no compare-and-swap. Use
[`Queue`](queue.md) when more than one thread pushes or pops.

```rust
use vas::sync::Ring;

let mut commands: Ring<Command, Borrowed> = Ring::new(arena.alloc::<Command>(256)?);
let (producer, consumer) = commands.split();

std::thread::scope(|scope| {
    scope.spawn(move || {
        while let Some(command) = consumer.pop() { // render thread
            command.execute();
        }
    });
    for command in frame_commands {               // game thread
        if let Err(Full(command)) = producer.push(command) {
            // full: the render thread is behind
        }
    }
});
```

## Roles

`split(&mut self)` returns the two ends. Both borrow the ring, so there is
no reference counting and no allocation, and the ring cannot be split again
(or dropped) while they exist. Values left by earlier handles stay in the
ring for the next `split`.

| Handle | Operations | Thread rules |
|---|---|---|
| `Producer<'r, T>` | `push(T) -> Result<(), Full<T>>` | `Send`, not `Sync`, not `Clone` |
| `Consumer<'r, T>` | `pop() -> Option<T>` | `Send`, not `Sync`, not `Clone` |

Both also have `capacity()`, and `len()` / `is_empty()`, which are
**snapshots**: the other side may change them at once. The ring itself has
`capacity()`, `len()` and `is_empty()`, exact, as no handle exists while it
is borrowed.

The handles do not name the storage, so code taking a `Producer<'_, T>`
works with any ring of `T`.

For handles that are `'static` (threads that are not scoped), put the ring
in `'static` memory: `arena.alloc_value(Ring::new(slots))` on an arena over
a [`StaticBlock`](arena.md) gives a `&'static mut Ring`, and `split` then
gives `'static` handles.

## Storage

- `Inline<N>`: slots inside the ring; `Ring::inline()` is `const`.
- `Borrowed<'a>`: slots from the caller, `&mut [MaybeUninit<T>]` of plain
  `T` (no [`Slot`](queue.md) wrapper), for example
  `arena.alloc::<T>(1024)?`. `new` is O(1).
- No `Dynamic`: the capacity is fixed (the storage must be
  [`Fixed`](storage.md)).

The capacity is the **largest power of two** of the slots given (12 slots
give 8), all of them usable. Zero slots make a ring that is always full and
always empty.

## Guarantees

- Never allocates, never panics, never blocks.
- Values come out in the order they went in, each exactly once, or are
  dropped with the ring (`T: Send` for the handles to cross threads).
- Wait-free: `push` and `pop` finish in a bounded number of steps whatever
  the other thread does; a full or empty ring answers at once.

## How it works

Positions count up forever (with wrapping); position `p` lives in slot
`p % capacity`. Values occupy positions `head..tail`. Only the producer
moves `tail`, only the consumer moves `head`; each is on a cache line of
its own ([`CachePadded`](cache_padded.md)).

Each handle keeps its own index in a plain field, and the last value it
read of the other side's index. It reads the other index again only when
the ring looks full (producer) or empty (consumer), so in a steady stream
most operations touch no shared cache line but the slot itself.

### Memory orderings

| Access | Ordering | Why it is enough |
|---|---|---|
| `push`: write of the value | plain | the slot is free: the consumer has moved `head` past it |
| `push`: `tail` store | `Release` | the consumer that sees the new `tail` sees the value |
| `pop`: `tail` load | `Acquire` | pairs with that store: the values below `tail` are visible |
| `pop`: read of the value | plain | the slot is written and no one else reads it |
| `pop`: `head` store | `Release` | the producer that sees the new `head` writes the slot only after this read |
| `push`: `head` load | `Acquire` | pairs with that store |
| `len`, and the ring's own `len` | `Relaxed` | a snapshot (exact on the ring, which no handle borrows meanwhile) |

No ABA problem: the indices are compared only with each other, never
swapped, and `tail - head` never exceeds the capacity.

## Verification

- Single-threaded tests: order, full and empty, capacities 0, 1 and
  power-of-two rounding, wrap-around over thousands of laps checked against
  a model, splitting again.
- A stream of boxed values from one thread to another, 2 and 1024 slots:
  every value arrives once and in order.
- A ring in `'static` arena memory feeding a thread that is not scoped.
- Drop tests with a counting type, with values left across the end of the
  ring.
- A compile-time test that a handle cannot be shared between threads.
- Under Miri (`make miri`) and ThreadSanitizer (`make tsan`). Each of the
  four atomic orderings was weakened to `Relaxed` in turn: Miri reported a
  data race on each of three seeds every time.

## Speed

See [benchmarks](benchmarks.md): against `Queue` and `Mutex<VecDeque>`, and
against `rtrb`, `heapless::spsc` and `crossbeam`'s `ArrayQueue`, on one
thread and streaming between two.
