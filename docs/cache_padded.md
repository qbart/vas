# CachePadded

`vas::sync::CachePadded<T>` keeps a value on a cache line of its own. Use it
for values written by different threads that would otherwise share a line:
each write by one thread then evicts the line from the other's cache ("false
sharing"), which can make two independent counters as slow as one shared
one.

```rust
use core::sync::atomic::AtomicUsize;
use vas::sync::CachePadded;

struct Worker {
    pushed: CachePadded<AtomicUsize>, // written by the owner
    stolen: CachePadded<AtomicUsize>, // written by thieves
}
```

- Aligned (and so sized) to 128 bytes on x86_64, aarch64 and powerpc64,
  whose CPUs fetch cache lines in pairs, and 64 bytes elsewhere.
- Derefs to `T`; `new` is `const`, `into_inner` unwraps it. `Clone`,
  `Copy`, `Default`, `PartialEq`, `Eq`, `Hash` and `Debug` when `T` has
  them, and `From<T>`.
- Needs no atomics, so it is available on every target. vas's own
  [`Queue`](queue.md) keeps its two indices in it.
