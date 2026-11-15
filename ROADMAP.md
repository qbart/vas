# Roadmap

## 0.0.0 — bootstrap

- [x] Empty `no_std` crate with lints, Makefile and docs skeleton
- [x] Reserve the `vas` crate name on crates.io

## 0.1.0 — conventions and vector

- [x] Storage convention spec (`docs/storage.md`): memory is provided by the library user
- [x] Error convention spec (`docs/errors.md`): no panics, `Result` / `Option` everywhere
- [x] Fixed-capacity vector over user-provided memory, exposing its contents as a contiguous slice (e.g. to hand to a renderer)
- [x] `Memory<T, N>` inline storage block
- [x] Vector `insert`, `remove`, `resize`, `move_into`

## 0.2.0 — pool

- [x] Generational pool (`Pool`, `Handle`) over user-provided memory

## 0.3.0 — map

- [x] Unordered map (`Map`) over user-provided memory
- [x] `FastHasher`, replaceable through `BuildHasher`

## 0.4.0 — deque

- [x] Double-ended queue / ring buffer (`Deque`) over user-provided memory

## Later

- [ ] Binary heap, bitset
- [ ] Allocator trait for containers that grow on their own
- [ ] Algorithms: A*, spatial grid, PRNG
