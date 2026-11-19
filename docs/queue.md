# Queue

`vas::sync::Queue<T, S>` is a first-in, first-out queue of fixed capacity
that any number of threads push to and pop from at the same time: a
multi-producer, multi-consumer (MPMC) queue. Use it to inject jobs from any
thread, deliver IO completions, and hand work to another thread.

```rust
use vas::Inline;
use vas::sync::{Queue, Slot};

static JOBS: Queue<Job, Inline<1024>> = Queue::inline();

// any thread
if let Err(Full(job)) = JOBS.push(job) {
    job.run(); // full: do it here instead
}
// any worker
while let Some(job) = JOBS.pop() {
    job.run();
}

// or in arena memory
let results: Queue<Result, Borrowed> = Queue::new(arena.alloc::<Slot<Result>>(256)?);
```

## Storage

- `Inline<N>`: the slots live in the queue. `Queue::inline()` is `const`,
  so the queue can be a `static` with no setup.
- `Borrowed<'a>`: slots from the caller, as `&mut [MaybeUninit<Slot<T>>]`,
  for example from an [arena](arena.md). `new` stamps them, O(capacity).
- No `Dynamic`: the capacity is fixed (the storage must be
  [`Fixed`](storage.md)), because growing would move values other threads
  are using.

The capacity is the **largest power of two** of the slots given (10 slots
give 8; `Inline<5>` gives 4), so that a position maps to its slot with a
mask. Give a power of two to use all of them. Zero slots make a queue that
is always full and always empty.

## API

All operations take `&self`.

| Operation | Signature | Cost |
|---|---|---|
| create | `Queue::inline()` (`const`), `Queue::new(buffer)`, `default()` | O(1) / O(capacity) |
| add at the back | `push(T) -> Result<(), Full<T>>` | O(1), lock-free |
| take from the front | `pop() -> Option<T>` | O(1), lock-free |
| size | `capacity()`; `len()`, `is_empty()` | O(1); a snapshot |

`Queue<T, S>` is `Sync` and `Send` when `T: Send`, so `&Queue` can be given
to any number of threads (scoped threads, a `static`, or `'static` arena
memory).

## Guarantees

- Never allocates, never panics, never blocks. A full queue hands the value
  back in `Full` at once; an empty one returns `None` at once.
- Every value pushed is popped exactly once, or dropped with the queue.
- Values from one thread come out in the order that thread pushed them; in
  fact the whole queue is FIFO in the order pushes claimed their position.
- Lock-free: a thread retries only when another thread's push or pop
  succeeded meanwhile, and waits a few bounded spins (at most 64) after a
  lost race so contending threads spread out.
- Values still in the queue are dropped when it is dropped.

Answers that can surprise, because nothing waits:

- `pop` returns `None` when the next value's push has claimed its slot but
  not finished writing it, even if later values are already written.
- `push` returns `Full` when the slot it needs is still being read by a
  `pop` of the previous lap.
- `len` and `is_empty` are snapshots; other threads may change them at once.

All of these are momentary: retrying later succeeds.

## How it works

Dmitry Vyukov's bounded queue. Each slot holds a value and an atomic
*stamp*; two counters, `head` (next pop) and `tail` (next push), each on its
own cache line ([`CachePadded`](cache_padded.md)), count positions forever
(with wrapping). Position `p` lives in slot `p % capacity`, and its stamp
says what state the slot is in for that position:

| Stamp | Meaning |
|---|---|
| `2p` | free for the push of position `p` |
| `2p + 1` | the value of position `p` is written |
| `2(p + capacity)` | taken; free for the push one lap later |

Doubling makes the states distinct even with a capacity of 1.

- **push** reads `tail = p` and the stamp of its slot. If it is `2p`, it
  claims the position with a compare-and-swap of `tail` from `p` to `p + 1`,
  writes the value, and stores the stamp `2p + 1`. A smaller stamp means the
  slot still belongs to the previous lap: full. A larger one means another
  push got there first: reload `tail`.
- **pop** does the same with `head` and the stamp `2p + 1`, reads the value,
  and stores `2(p + capacity)`.

A position is claimed by exactly one thread (the compare-and-swap succeeds
for one), so each value is written once and read once.

### Memory orderings

| Access | Ordering | Why it is enough |
|---|---|---|
| stamp store after writing a value | `Release` | publishes the value |
| stamp load in `pop` | `Acquire` | a pop that sees `2p + 1` sees the value written before it |
| stamp store after reading a value | `Release` | publishes that the read is over |
| stamp load in `push` | `Acquire` | a push that sees the slot free writes only after the previous value was read out |
| compare-and-swap of `head` / `tail` | `Relaxed` | it only decides which thread owns a position; all data passes through the stamps, and a read-modify-write on one counter always sees the latest value, so two threads cannot claim the same position |
| loads of `head` / `tail` | `Relaxed` | a stale value is caught by the stamp check (a larger stamp) and reloaded |
| `len` | `Relaxed` | a snapshot by definition |

No ABA problem on wrap-around: counters and stamps are `usize` and wrap
together (capacities are powers of two, so slot indices stay consistent).
For a stale counter to look current, a thread would have to stall while
other threads complete 2^63 operations on a 64-bit target. On 32-bit
targets that is 2^31 operations: a thread would have to be suspended,
between reading a stamp and its compare-and-swap, while about two billion
others complete.

## Verification

- Single-threaded tests: FIFO order, full and empty, a capacity of 1 and 0,
  power-of-two rounding, wrap-around over many laps.
- Stress tests: producers on several threads push known values while
  consumers on several others pop; every value arrives exactly once, and
  in order per producer, with 4 and 1024 slots.
- Drop tests with a counting type: no leak and no double drop, with values
  left in the queue across the end of the ring.
- The tests run under Miri (`make miri`, which checks for data races and
  emulates weak memory) and under ThreadSanitizer (`make tsan`). Weakening
  either `Release` store to `Relaxed` is reported as a data race by both.

## Speed

One thread pushing then popping: 7.8 ns for the pair, against 16.1 for a
`Mutex<VecDeque>` and 7.7 for `crossbeam`'s `ArrayQueue`. With producers
and consumers on separate threads, see [benchmarks](benchmarks.md).
