# Errors

vas never panics. There is no `unwrap`, `expect`, `panic!` or unchecked
indexing in the library; clippy lints in `Cargo.toml` enforce this.

## Convention

- An operation that can run out of capacity returns `Result<(), Full<T>>`.
  `Full` hands the rejected value back to the caller, so nothing is lost or
  dropped silently. Bulk operations that reject the whole input return
  `Full<()>` and leave the container unchanged.
- An operation that can miss (empty container, index out of range) returns
  `Option`.
- Failed operations leave the container unchanged.
- Error types implement `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq` when the
  payload does, and `core::fmt::Display` + `core::error::Error`.
