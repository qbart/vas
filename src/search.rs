//! Binary search on sorted slices: vas's own, not the one from `core`.
//!
//! - [`binary_search`], [`binary_search_by`], [`binary_search_by_key`]:
//!   `Ok(index)` of the **first** match, or `Err(index)` where the value
//!   would be inserted to keep the order.
//! - [`partition_point`]: the first index where a predicate stops holding.
//!
//! What they promise beyond `core`'s:
//!
//! - **The first match.** With equal elements, `core` may return any of
//!   them (and which one changed with Rust 1.52); these always return the
//!   lowest index, so the result is the same everywhere and doubles as a
//!   lower bound.
//! - **No panics.** On a slice that is not sorted, or a comparison that is
//!   not a total order, the result is unspecified but within
//!   `0..=items.len()`.
//!
//! Each search is O(log n) comparisons, with no branch on their outcome.
//! The same functions are methods on [`Vector`](crate::Vector).
//!
//! ```
//! use vas::search;
//!
//! let times: [f32; 4] = [0.5, 1.0, 1.0, 2.5];
//! assert_eq!(search::binary_search_by(&times, |t| t.total_cmp(&1.0)), Ok(1));
//! assert_eq!(search::partition_point(&times, |t| *t < 2.0), 3);
//! ```

#![allow(unsafe_code)]

use core::cmp::Ordering;

/// The first index whose element does not satisfy `before`, assuming the
/// elements that do all come first. Never branches on `before`.
#[inline]
fn first_not<T>(items: &[T], mut before: impl FnMut(&T) -> bool) -> usize {
    let mut size = items.len();
    if size == 0 {
        return 0;
    }
    let mut base = 0;
    while size > 1 {
        let half = size / 2;
        let mid = base + half;
        // SAFETY: `mid < base + size <= len`: `base + size` never exceeds
        // the length (it starts equal and `size` shrinks by what `base`
        // may gain), and `half < size`.
        let go = before(unsafe { items.get_unchecked(mid) });
        base = core::hint::select_unpredictable(go, mid, base);
        size -= half;
    }
    base + usize::from(items.get(base).is_some_and(before))
}

/// The first index where `predicate` is `false`, when the elements for
/// which it is `true` all come first. O(log n).
#[inline]
pub fn partition_point<T>(items: &[T], predicate: impl FnMut(&T) -> bool) -> usize {
    first_not(items, predicate)
}

/// Searches sorted `items` with `compare`, which orders an element against
/// the target: `Less` for an element before it. `Ok` with the index of the
/// first element that compares `Equal`, else `Err` with the index where
/// the target would go.
#[inline]
pub fn binary_search_by<T>(
    items: &[T],
    mut compare: impl FnMut(&T) -> Ordering,
) -> Result<usize, usize> {
    let index = first_not(items, |item| compare(item) == Ordering::Less);
    match items.get(index) {
        Some(item) if compare(item) == Ordering::Equal => Ok(index),
        _ => Err(index),
    }
}

/// Searches sorted `items` for `target`: `Ok` with the index of the first
/// equal element, else `Err` with the index where it would go.
#[inline]
pub fn binary_search<T: Ord>(items: &[T], target: &T) -> Result<usize, usize> {
    binary_search_by(items, |item| item.cmp(target))
}

/// Searches `items`, sorted by `key`, for the key `target`: `Ok` with the
/// index of the first element with that key, else `Err` with the index
/// where it would go.
#[inline]
pub fn binary_search_by_key<T, K: Ord>(
    items: &[T],
    target: &K,
    mut key: impl FnMut(&T) -> K,
) -> Result<usize, usize> {
    binary_search_by(items, |item| key(item).cmp(target))
}
