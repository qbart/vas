# JobCounter

`vas::sync::JobCounter` counts the unfinished jobs of a group, so that a
thread can wait for all of them: a frame's culling jobs before rendering,
the chunks of a parallel loop, the children of a job before its
continuation.

```rust
use vas::sync::JobCounter;

static CULLING: JobCounter = JobCounter::new();

CULLING.add(chunks.len())?;       // before handing the jobs out
for chunk in chunks {
    jobs.push(Job::cull(chunk))?; // each job calls `CULLING.done()` at its end
}
CULLING.wait(|| run_one_job(&jobs)); // help instead of idling
render(&visible);                    // every job's writes are visible here
```

## API

| Method | Does |
|---|---|
| `new()` | an empty counter, which is done; `const`, so it can be a `static` |
| `add(count) -> Result<(), Full<usize>>` | adds jobs; refuses (adding nothing) past `LIMIT` |
| `done() -> bool` | marks one job finished; `true` to the call that finishes the last one |
| `is_done() -> bool` | `true` when no job is unfinished |
| `wait(help)` | calls `help` until the counter is done |
| `pending() -> usize` | unfinished jobs; a snapshot |
| `LIMIT` | `isize::MAX`: the most jobs it holds |

`Default` gives `new()`; `Debug` shows `JobCounter { pending: 3 }`.

## Guarantees

- Never allocates, never panics, never blocks. `wait` loops on the
  caller's `help`, which decides how to wait: run another job (the usual
  choice in a job system), spin, or yield the thread. vas cannot put a
  thread to sleep without an operating system.
- Whatever a job wrote before its `done` is visible to any thread that then
  sees `is_done()` return `true` (or that `wait` returns to), and to the job
  whose `done` returned `true`. No further locking is needed to read the
  results.
- Exactly one `done` per round returns `true`: the place to start a job
  that depends on the whole group.
- Reusable: once done, `add` starts a new round.

## Mistakes

- **`done` too many times.** On a counter that is already done, `done`
  undoes itself and returns `false`; for that moment the counter reads as
  unfinished, and an `add` racing with it may be refused. An extra `done`
  while other jobs are still pending cannot be told apart from a real one:
  the group then looks done one job early. Any counter has that limit.
- **`add` after handing jobs out.** Add first: a job that finishes before
  its `add` may finish the group early.
- **Never calling `done`.** `wait` then never returns.

## How it works

One `AtomicUsize`, the number of unfinished jobs.

- `done` is a single `fetch_sub`. If the count was 0 (or above `LIMIT`,
  meaning another call too many is under way) it adds the 1 back. Real
  counts never pass `LIMIT`, so while any such undo is pending the count
  stays above `LIMIT`: it never reads 0 early, and `add` refuses rather than
  build on it.
- `add` is a compare-and-swap loop, so that the count never passes `LIMIT`
  even for a moment. It is called once per batch, so its cost does not
  matter.

### Memory orderings

| Access | Ordering | Why it is enough |
|---|---|---|
| `done`: `fetch_sub` | `AcqRel` | `Release` publishes the job's writes. Every change to the count is a read-modify-write, so all of them form one release sequence: an `Acquire` that sees any later value, 0 included, sees every job's writes. `Acquire` gives the same to the call that finishes the last job |
| `is_done` (and `wait`): load | `Acquire` | pairs with the `Release` of every `done` |
| `done`: the undo `fetch_add` | `Relaxed` | it restores a value; it publishes nothing |
| `add`: compare-and-swap | `Relaxed` | the jobs reach the threads that run them through something that synchronises (a queue, a spawn) after the add |
| `pending` | `Relaxed` | a snapshot by definition |

## Verification

- Single-threaded tests: counting down, reuse, the limit, `done` on a done
  counter, `wait`.
- A stress test where jobs write results **without atomics** and the main
  thread reads them once `wait` returns, while the workers are still
  running: only the counter's orderings make this sound, so a missing
  `Release` or `Acquire` is a data race. Weakening `done` to `Acquire`, or
  `is_done` to `Relaxed`, is reported as a data race by Miri on each of
  three seeds.
- A stress test where four threads call `done` once too often while adding
  and finishing jobs: the count must end at 0. (It caught a first version
  that undid only a count of 0, not a second call too many.)
- A `static` counter shared with plain threads.
- Under Miri (`make miri`) and ThreadSanitizer (`make tsan`).

## Speed

See [benchmarks](benchmarks.md): threads doing nothing but finishing jobs on
one counter, against a bare `fetch_sub` and a `Mutex<usize>`. `done` costs
the same as the bare `fetch_sub`. Under that much contention, padding the
counter with [`CachePadded`](cache_padded.md) keeps other data off its
cache line.
