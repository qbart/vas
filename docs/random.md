# Random

`Rng` is a small, fast, seedable pseudo-random number generator. It needs no
storage: its whole state is 32 bytes inside the value.

## Algorithm

xoshiro256++ (Blackman & Vigna), seeded by expanding a `u64` with SplitMix64.
Both are verified in the tests against their published reference output.

- **Deterministic:** the same seed gives the same sequence on every platform.
  All bounded results go through 64-bit arithmetic, so they do not depend on
  the width of `usize`.
- **Not cryptographically secure:** never use it for secrets or anything an
  adversary must not predict.

## API

| Operation | Signature | Notes |
|---|---|---|
| create | `Rng::new(seed: u64)` | any seed, including 0 |
| save / restore | `state() -> [u64; 4]`, `Rng::from_state([u64; 4])` | continues the sequence exactly; an all-zero state is replaced |
| split | `fork() -> Rng` | a new generator seeded from this one |
| raw | `u64()`, `u32()`, `bool()` | uniform |
| floats | `f32()`, `f64()` | in `0.0..1.0`, 1.0 excluded |
| probability | `chance(p: f32) -> bool` | 0.0 never, 1.0 always |
| bounded | `u32_below(n)`, `u64_below(n)`, `usize_below(n)` | in `0..n`, unbiased; 0 when `n` is 0 |
| ranges | `i32_range(min, max)`, `f32_range(min, max)` | `max` excluded; `min` when the range is empty |
| slices | `pick(&[T]) -> Option<&T>`, `shuffle(&mut [T])` | `None` for an empty slice; unbiased shuffle |

## Guarantees

- No allocation, no panics, no `unsafe`. Degenerate inputs (a bound of zero,
  an empty range or slice) return a defined value instead of failing.
