//! Error types shared by all containers.

use core::fmt;

/// The container has no room left.
///
/// Carries the rejected value back to the caller. Bulk operations that reject
/// their whole input use `Full<()>`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Full<T>(pub T);

impl<T> fmt::Debug for Full<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Full(..)")
    }
}

impl<T> fmt::Display for Full<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("container is full")
    }
}

impl<T> core::error::Error for Full<T> {}
