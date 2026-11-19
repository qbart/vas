# StealDeque

`vas::sync::StealDeque<T, S>` is a work-stealing deque of fixed capacity, for
job systems: each worker thread owns one, pushes the jobs it creates and
pops them back last in, first out, while idle threads steal from the other
end, first in, first out. It is Chase and Lev's deque, adapted so that no
value is ever read while it might be overwritten (see below).

```rust
use vas::sync::{Slot, Steal, StealDeque};

let mut deque: StealDeque<Job, Borrowed> = StealDeque::new(arena.alloc::<Slot<Job>>(256)?);
let (worker, stealer) = deque.split();

std::thread::scope(|scope| {
    for _ in 0..3 {
        scope.spawn(move || loop {
            match stealer.steal() {
                Steal::Success(job) => job.run(),
                Steal::Retry => continue,
                Steal::Empty => break,
            }
        });
    }
    for job in jobs {
        if let Err(Full(job)) = worker.push(job) {
            job.run(); // full: run it here
        }
    }
    while let Some(job) = worker.pop() {
        job.run();
    }
});
```

## Roles

`split(&mut self)` returns the two handles. Both borrow the deque, so there
is no reference counting and no allocation, and the deque cannot be split
again (or dropped) while they exist.

| Handle | Operations | Thread rules |
|---|---|---|
| `Worker<'d, T, S>` | `push(T) -> Result<(), Full<T>>`, `pop() -> Option<T>` (back, LIFO), `stealer()` | `Send`, not `Sync`, not `Clone`: one owner at a time, which may move to another thread |
| `Stealer<'d, T, S>` | `steal() -> Steal<T>` (front, FIFO) | `Copy`, `Send` and `Sync`: any number of thieves |

Both also have `capacity()`, and `len()` / `is_empty()`, which are
**snapshots**: other threads may change them at once.

`Steal<T>` is `Empty`, `Success(T)` or `Retry` (another thread took that
value first; the deque may still hold others). It has `success() ->
Option<T>`, `is_empty()`, `is_success()` and `is_retry()`. `steal` never
waits or retries on its own; the caller decides.

For handles that are `'static` (threads that are not scoped), put the
deque in `'static` memory: `arena.alloc_value(StealDeque::new(slots))` on
an arena over a [`StaticBlock`](arena.md) gives a `&'static mut
StealDeque`, and `split` then gives `'static` handles.

`Worker` cannot be shared between threads:

```rust
let (worker, _) = deque.split();
std::thread::scope(|scope| {
    scope.spawn(|| worker.push(1)); // error: `&Worker` is not `Send`
});
```

## Storage

- `Inline<N>`: slots inside the deque; `StealDeque::inline()` is `const`.
- `Borrowed<'a>`: slots from the caller, `&mut [MaybeUninit<Slot<T>>]`,
  for example from an [arena](arena.md). `new` stamps them, O(capacity).
- No `Dynamic`: the capacity is fixed (the storage must be
  [`Fixed`](storage.md)).

The capacity is the **largest power of two** of the slots given (12 slots
give 8). Give a power of two to use all of them. Zero slots make a deque
that is always full and always empty.

## Guarantees

- Never allocates, never panics, never blocks.
- Every value pushed is taken exactly once, by the owner or by one thief,
  or dropped with the deque (`T: Send`; values left in the deque are
  dropped with it).
- Lock-free: a thief whose compare-and-swap fails gets `Retry` because
  another thread took a value.
- `push` returns `Full` when the deque is full, and also, momentarily, when
  the slot it needs is still being read by a thief that took the value one
  lap earlier (only possible while the deque is nearly full).

## How it works

Positions count up forever (with wrapping); position `p` lives in slot
`p % capacity`. Values occupy positions `top..bottom`. The owner pushes and
pops at `bottom`; thieves take at `top`. `top` and `bottom` are each on a
cache line of their own ([`CachePadded`](cache_padded.md)).

- **push** (owner): checks the slot's stamp, writes the value at `bottom`,
  then raises `bottom`.
- **pop** (owner): lowers `bottom` first, to reserve the last value, then
  reads `top`. If more than one value is left, the one at `bottom` is the
  owner's. If exactly one is left, the owner and the thieves race for it
  with a compare-and-swap on `top`. If none, `bottom` goes back up.
- **steal** (thief): reads `top`, then `bottom`; if there is a value, claims
  it by moving `top` up one with a compare-and-swap, and only **then** reads
  it.

### Why thieves claim before reading

In the textbook algorithm a thief reads the value first and then tries to
claim it, throwing the copy away if it loses. In Rust that is undefined
behaviour: while the losing thief is reading, the owner can wrap around
the ring and write a new value into the same slot, and a read racing with
a write is a data race even if its result is discarded (Miri reports it).
Claiming first means a thief only ever reads a value it owns.

The owner still must not write a slot while a thief that claimed the
previous lap's value is reading it. Each slot therefore has a **stamp**: the
position it is free for. A thief that has read position `p` sets the stamp
to `p + capacity`; the owner pops of the last value do the same. A push to
position `b` goes ahead only when the stamp is exactly `b`, which also
covers the full check: while the value one lap back is still in the deque
the stamp is lower. Owner pops of other values leave the stamp alone, as
the next push reuses that same position.

### Memory orderings

| Access | Ordering | Why it is enough |
|---|---|---|
| `push`: stamp load | `Acquire` | the owner writes the slot only after the thief's read of the previous value |
| `push`: `bottom` store | `Release` | a thief that sees the new `bottom` sees the value |
| `pop`: `bottom` stores | `Release` | a thief may read any `bottom` value; each must publish all values pushed before it |
| `pop` and `steal`: fence between the `bottom` and `top` accesses | `SeqCst` | the two fences are totally ordered: either the thief sees the lowered `bottom` (and finds the deque empty), or the owner sees the thief's raised `top` (and races for the last value). Without them both could take the last value |
| `pop`: `top` load | `Relaxed` | ordered by the fence |
| `steal`: `top` load, `bottom` load | `Acquire` | the `bottom` load makes the values below it visible to the thief |
| compare-and-swap of `top` (thief and owner) | `SeqCst` | part of the fence argument; also gives the winner exclusive ownership of the position |
| `steal`: stamp store after reading | `Release` | publishes that the read is over |
| `pop`: stamp store after taking the last value | `Relaxed` | only the owner reads stamps, and it wrote this one itself |
| `len` | `Relaxed` | a snapshot by definition |

No ABA problem on wrap-around: `top` only moves up, by compare-and-swap, so
a thief's stale `top` matches again only after 2^64 steals on a 64-bit
target. On 32-bit targets that is 2^32: a thief would have to be suspended
between reading `top` and its compare-and-swap while four billion values
pass through the deque.

## Verification

- Single-threaded tests: LIFO for the owner, FIFO for thieves, the race for
  the last value, full and empty, capacities 0 and 1, power-of-two
  rounding, wrap-around over thousands of laps checked against a model.
- Stress tests: the owner pushes and pops while 1, 3 or 4 thieves steal,
  with 4 and 1024 slots; every value is taken exactly once.
- A deque in `'static` arena memory feeding a thread that is not scoped.
- Drop tests with a counting type: no leak and no double drop, with values
  left across the end of the ring.
- Under Miri (`make miri`, data races and weak memory) and ThreadSanitizer
  (`make tsan`). Each of six orderings was weakened in turn as a check of the
  tests: the stamp load in `push`, the `bottom` store in `push`, both
  `SeqCst` fences, the `bottom` load in `steal`, and the stamp store in
  `steal`. Every one was caught over three Miri seeds: a data race on every
  seed for five of them; for the fence in `pop`, a data race on one seed
  and a stress test that never finished on the other two.

## Speed

See [benchmarks](benchmarks.md): the owner's push and pop against
`Mutex<VecDeque>` and `crossbeam_deque`, alone and with thieves stealing.
