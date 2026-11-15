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

/// An insert at a position was rejected. Carries the value back to the caller.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InsertError<T> {
    /// The container has no room left.
    Full(T),
    /// The position is past the end of the container.
    OutOfBounds(T),
}

impl<T> fmt::Debug for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "Full(..)",
            Self::OutOfBounds(_) => "OutOfBounds(..)",
        })
    }
}

impl<T> fmt::Display for InsertError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full(_) => "container is full",
            Self::OutOfBounds(_) => "position is out of bounds",
        })
    }
}

impl<T> core::error::Error for InsertError<T> {}

/// An index was past the end of the container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutOfBounds;

impl fmt::Display for OutOfBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("index is out of bounds")
    }
}

impl core::error::Error for OutOfBounds {}
