//! Fixed-capacity ordered map over user-provided memory.

use core::borrow::Borrow;
use core::mem;
use core::ops::{Bound, RangeBounds};
use core::{fmt, slice};

use crate::storage::{Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// A fixed-capacity map that keeps its entries sorted by key, in memory
/// provided by the caller.
///
/// The entries are one contiguous sorted block. Lookups are a binary search,
/// O(log n); inserting or removing shifts the entries after the position,
/// O(n). That makes it a good fit for maps that are read far more often than
/// they change, and for small to medium sizes.
///
/// ```
/// use vas::{Memory, OrderedMap};
///
/// let mut memory = Memory::<(u32, &str), 16>::new();
/// let mut keyframes = OrderedMap::new(&mut memory);
///
/// assert_eq!(keyframes.insert(20, "jump"), Ok(None));
/// assert_eq!(keyframes.insert(0, "idle"), Ok(None));
/// assert_eq!(keyframes.insert(10, "step"), Ok(None));
///
/// // Sorted by key, whatever the insertion order.
/// assert_eq!(keyframes.first(), Some((&0, &"idle")));
/// // The keyframe in effect at time 15.
/// assert_eq!(keyframes.at_or_before(&15), Some((&10, &"step")));
/// ```
pub struct OrderedMap<K, V, S: Storage<(K, V)>> {
    // Invariant: sorted by key, strictly increasing (no duplicate keys).
    entries: Vector<(K, V), S>,
}

impl<K, V, const N: usize> OrderedMap<K, V, Inline<N>> {
    /// Creates an empty map with room for `N` entries inside itself.
    ///
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            entries: Vector::inline(),
        }
    }
}

impl<K, V, const N: usize> Default for OrderedMap<K, V, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<K, V, S: Storage<(K, V)>> OrderedMap<K, V, S> {
    /// Creates an empty map on `buffer`: a reference to memory borrows it,
    /// a `Memory` passed by value is stored inside the map.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<(K, V), Storage = S>,
    {
        Self {
            entries: Vector::new(buffer),
        }
    }

    /// Number of entries in the map.
    #[inline]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Maximum number of entries the map can hold.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// Number of entries that can still be inserted.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.entries.remaining()
    }

    /// `true` when the map holds no entries.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `true` when no new key can be inserted.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    /// The entry with the smallest key.
    #[inline]
    pub fn first(&self) -> Option<(&K, &V)> {
        self.entries.first().map(|(key, value)| (key, value))
    }

    /// The entry with the greatest key.
    #[inline]
    pub fn last(&self) -> Option<(&K, &V)> {
        self.entries.last().map(|(key, value)| (key, value))
    }

    /// Removes and returns the entry with the smallest key. O(n).
    #[inline]
    pub fn pop_first(&mut self) -> Option<(K, V)> {
        self.entries.remove(0)
    }

    /// Removes and returns the entry with the greatest key. O(1).
    #[inline]
    pub fn pop_last(&mut self) -> Option<(K, V)> {
        self.entries.pop()
    }

    /// Removes and drops all entries.
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// All entries as one contiguous slice, sorted by key.
    #[inline]
    pub fn as_slice(&self) -> &[(K, V)] {
        self.entries.as_slice()
    }

    /// Iterates over all entries in key order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            entries: self.entries.iter(),
        }
    }

    /// Iterates over all entries in key order, with mutable access to the
    /// values.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            entries: self.entries.iter_mut(),
        }
    }
}

impl<K: Ord, V, S: Storage<(K, V)>> OrderedMap<K, V, S> {
    /// Inserts `value` under `key`.
    ///
    /// Returns `Ok(Some(old))` when the key was already present (the value
    /// is replaced, the existing key is kept), `Ok(None)` when it is new, and
    /// hands both back in [`Full`] when the key is new and there is no room.
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, Full<(K, V)>> {
        match self.search(&key) {
            Ok(index) => match self.entries.get_mut(index) {
                Some((_, slot)) => Ok(Some(mem::replace(slot, value))),
                None => Err(Full((key, value))),
            },
            Err(index) => match self.entries.insert(index, (key, value)) {
                Ok(()) => Ok(None),
                Err(crate::InsertError::Full(entry) | crate::InsertError::OutOfBounds(entry)) => {
                    Err(Full(entry))
                }
            },
        }
    }

    /// The value stored under `key`.
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let index = self.search(key).ok()?;
        self.entries.get(index).map(|(_, value)| value)
    }

    /// The value stored under `key`, mutably.
    #[inline]
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let index = self.search(key).ok()?;
        self.entries.get_mut(index).map(|(_, value)| value)
    }

    /// `true` when the map has an entry for `key`.
    #[inline]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.search(key).is_ok()
    }

    /// Removes the entry for `key` and returns its value. O(n).
    #[inline]
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.remove_entry(key).map(|(_, value)| value)
    }

    /// Removes the entry for `key` and returns its key and value. O(n).
    #[inline]
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let index = self.search(key).ok()?;
        self.entries.remove(index)
    }

    /// The entry with the greatest key that is not above `key`.
    ///
    /// For keyframes or timed events: the one in effect at `key`.
    #[inline]
    pub fn at_or_before<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let after = self.entries.partition_point(|(k, _)| k.borrow() <= key);
        let (key, value) = self.entries.get(after.checked_sub(1)?)?;
        Some((key, value))
    }

    /// The entry with the smallest key that is not below `key`.
    #[inline]
    pub fn at_or_after<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        let index = self.entries.partition_point(|(k, _)| k.borrow() < key);
        let (key, value) = self.entries.get(index)?;
        Some((key, value))
    }

    /// Iterates, in key order, over the entries whose keys fall in `range`
    /// (`a..b`, `a..=b`, `..b`, `a..`, `..`). An inverted range is empty.
    pub fn range<Q, R>(&self, range: R) -> Iter<'_, K, V>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
        R: RangeBounds<Q>,
    {
        let start = match range.start_bound() {
            Bound::Included(key) => self.entries.partition_point(|(k, _)| k.borrow() < key),
            Bound::Excluded(key) => self.entries.partition_point(|(k, _)| k.borrow() <= key),
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(key) => self.entries.partition_point(|(k, _)| k.borrow() <= key),
            Bound::Excluded(key) => self.entries.partition_point(|(k, _)| k.borrow() < key),
            Bound::Unbounded => self.entries.len(),
        };
        Iter {
            entries: self.entries.get(start..end).unwrap_or_default().iter(),
        }
    }

    /// Binary search: `Ok(index)` of the entry for `key`, or `Err(index)`
    /// where it would be inserted.
    #[inline]
    fn search<Q>(&self, key: &Q) -> Result<usize, usize>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.entries.binary_search_by(|(k, _)| k.borrow().cmp(key))
    }
}

impl<K: fmt::Debug, V: fmt::Debug, S: Storage<(K, V)>> fmt::Debug for OrderedMap<K, V, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'m, K, V, S: Storage<(K, V)>> IntoIterator for &'m OrderedMap<K, V, S> {
    type Item = (&'m K, &'m V);
    type IntoIter = Iter<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'m, K, V, S: Storage<(K, V)>> IntoIterator for &'m mut OrderedMap<K, V, S> {
    type Item = (&'m K, &'m mut V);
    type IntoIter = IterMut<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Iterator over entries of an [`OrderedMap`] in key order. See
/// [`OrderedMap::iter`] and [`OrderedMap::range`].
pub struct Iter<'m, K, V> {
    entries: slice::Iter<'m, (K, V)>,
}

impl<'m, K, V> Iterator for Iter<'m, K, V> {
    type Item = (&'m K, &'m V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next().map(|(key, value)| (key, value))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.entries.size_hint()
    }
}

impl<K, V> DoubleEndedIterator for Iter<'_, K, V> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.entries.next_back().map(|(key, value)| (key, value))
    }
}

impl<K, V> ExactSizeIterator for Iter<'_, K, V> {}

/// Iterator over entries of an [`OrderedMap`] in key order, with mutable
/// values. See [`OrderedMap::iter_mut`].
pub struct IterMut<'m, K, V> {
    entries: slice::IterMut<'m, (K, V)>,
}

impl<'m, K, V> Iterator for IterMut<'m, K, V> {
    type Item = (&'m K, &'m mut V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next().map(|(key, value)| (&*key, value))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.entries.size_hint()
    }
}

impl<K, V> DoubleEndedIterator for IterMut<'_, K, V> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.entries.next_back().map(|(key, value)| (&*key, value))
    }
}

impl<K, V> ExactSizeIterator for IterMut<'_, K, V> {}
