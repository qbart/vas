//! Sparse set: values by `u32` id, kept in one dense slice, over borrowed,
//! inline or growing memory.

use core::fmt;
use core::iter::{FusedIterator, Zip};
use core::slice;

use crate::allocator::Allocator;
use crate::storage::{Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// Values looked up by a `u32` id (an entity, a handle's index), stored
/// packed in one slice: an ECS component store.
///
/// Insert, remove and lookup are O(1), and iterating walks a plain `&[T]`
/// with no holes. Removing moves the last value into the hole, so the
/// order of the values changes.
///
/// Three blocks of memory: `sparse` holds one `u32` per possible id (its
/// length bounds the ids), `ids` and `values` one entry per value stored.
///
/// ```
/// use vas::{Memory, SparseSet};
///
/// let mut sparse = Memory::<u32, 1000>::new(); // ids below 1000
/// let mut ids = Memory::<u32, 64>::new();
/// let mut values = Memory::<[f32; 2], 64>::new(); // up to 64 positions
/// let mut positions = SparseSet::new(&mut sparse, &mut ids, &mut values);
///
/// assert!(positions.insert(812, [1.0, 2.0]).is_ok());
/// assert!(positions.insert(5, [0.0, 0.0]).is_ok());
/// assert_eq!(positions.get(812), Some(&[1.0, 2.0]));
/// for position in positions.values_mut() {
///     position[0] += 1.0; // a tight loop over 2 values, not 1000
/// }
/// assert_eq!(positions.remove(5), Some([1.0, 0.0]));
/// ```
///
/// With [`Dynamic`] storage all three blocks grow.
pub struct SparseSet<T, SS: Storage<u32>, SD: Storage<u32> = SS, SV: Storage<T> = SD> {
    // `sparse[id]` is the position of `id` in `ids` and `values` when the
    // set holds `id`; any value otherwise. `id` is in the set exactly when
    // `ids[sparse[id]] == id`, so stale entries need no clearing. Its
    // length is one past the highest id ever inserted.
    sparse: Vector<u32, SS>,
    // `ids[i]` is the id of `values[i]`; no id appears twice.
    ids: Vector<u32, SD>,
    values: Vector<T, SV>,
}

impl<T, const S: usize, const D: usize, const V: usize>
    SparseSet<T, Inline<S>, Inline<D>, Inline<V>>
{
    /// Creates an empty set for ids below `S` and up to `min(D, V)`
    /// values, held inside itself. This is `const`, so it can initialise a
    /// `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            sparse: Vector::inline(),
            ids: Vector::inline(),
            values: Vector::inline(),
        }
    }
}

impl<T, const S: usize, const D: usize, const V: usize> Default
    for SparseSet<T, Inline<S>, Inline<D>, Inline<V>>
{
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Only sets with inline storage can be cloned.
impl<T: Clone, const S: usize, const D: usize, const V: usize> Clone
    for SparseSet<T, Inline<S>, Inline<D>, Inline<V>>
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            sparse: self.sparse.clone(),
            ids: self.ids.clone(),
            values: self.values.clone(),
        }
    }
}

impl<T, AS, AD, AV> Default for SparseSet<T, Dynamic<AS>, Dynamic<AD>, Dynamic<AV>>
where
    AS: Allocator + Default,
    AD: Allocator + Default,
    AV: Allocator + Default,
{
    /// Creates an empty set that grows through the default allocators.
    #[inline]
    fn default() -> Self {
        Self::new(
            Dynamic(AS::default()),
            Dynamic(AD::default()),
            Dynamic(AV::default()),
        )
    }
}

impl<T, SS: Storage<u32>, SD: Storage<u32>, SV: Storage<T>> SparseSet<T, SS, SD, SV> {
    /// Creates an empty set on three blocks: `sparse` (one `u32` per
    /// possible id), `ids` and `values` (one entry per value). References
    /// borrow the memory, `Memory` blocks passed by value are stored inside
    /// the set.
    ///
    /// O(1): no memory is touched until ids are inserted.
    pub fn new<BS, BD, BV>(sparse: BS, ids: BD, values: BV) -> Self
    where
        BS: IntoBuffer<u32, Storage = SS>,
        BD: IntoBuffer<u32, Storage = SD>,
        BV: IntoBuffer<T, Storage = SV>,
    {
        Self {
            sparse: Vector::new(sparse),
            ids: Vector::new(ids),
            values: Vector::new(values),
        }
    }

    /// Number of values.
    #[inline]
    pub const fn len(&self) -> usize {
        self.values.len()
    }

    /// `true` when the set holds no values.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Number of values the set has room for: the smaller of the `ids` and
    /// `values` blocks. With dynamic storage they grow.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.ids.capacity().min(self.values.capacity())
    }

    /// Ids below this fit the `sparse` block. With dynamic storage it
    /// grows.
    #[inline]
    pub fn id_capacity(&self) -> usize {
        self.sparse.capacity()
    }

    /// The position of `id` in [`ids`](Self::ids) and
    /// [`values`](Self::values), or `None` when the set does not hold it.
    #[inline]
    pub fn index_of(&self, id: u32) -> Option<usize> {
        let index = *self.sparse.get(id as usize)? as usize;
        (self.ids.get(index) == Some(&id)).then_some(index)
    }

    /// `true` when the set holds a value for `id`.
    #[inline]
    pub fn contains(&self, id: u32) -> bool {
        self.index_of(id).is_some()
    }

    /// The value for `id`.
    #[inline]
    pub fn get(&self, id: u32) -> Option<&T> {
        self.values.get(self.index_of(id)?)
    }

    /// The value for `id`, mutably.
    #[inline]
    pub fn get_mut(&mut self, id: u32) -> Option<&mut T> {
        let index = self.index_of(id)?;
        self.values.get_mut(index)
    }

    /// Sets the value for `id` and returns the one it replaces.
    ///
    /// Hands `value` back in [`Full`] when `id` does not fit the sparse
    /// block, or a new value does not fit the dense ones (with dynamic
    /// storage: the allocator refused). Replacing needs no room.
    pub fn insert(&mut self, id: u32, value: T) -> Result<Option<T>, Full<T>> {
        if let Some(slot) = self
            .index_of(id)
            .and_then(|index| self.values.get_mut(index))
        {
            return Ok(Some(core::mem::replace(slot, value)));
        }
        let index = self.values.len();
        let Ok(position) = u32::try_from(index) else {
            return Err(Full(value));
        };
        let at = id as usize;
        // The first two can fail; nothing is changed until both succeeded.
        let fits = (at < self.sparse.len()
            || at
                .checked_add(1)
                .is_some_and(|len| self.sparse.resize(len, 0).is_ok()))
            && self.ids.reserve(1).is_ok();
        if !fits {
            return Err(Full(value));
        }
        self.values.push(value)?;
        // Room was reserved above.
        let _ = self.ids.push(id);
        if let Some(slot) = self.sparse.get_mut(at) {
            *slot = position;
        }
        Ok(None)
    }

    /// Removes the value for `id` and returns it. The last value moves into
    /// its place.
    pub fn remove(&mut self, id: u32) -> Option<T> {
        let index = self.index_of(id)?;
        self.remove_at(index)
    }

    /// Removes the value at `index` of the dense slices, moving the last
    /// one into its place.
    fn remove_at(&mut self, index: usize) -> Option<T> {
        let value = self.values.swap_remove(index)?;
        self.ids.swap_remove(index);
        if let Some(&moved) = self.ids.get(index)
            && let Some(slot) = self.sparse.get_mut(moved as usize)
        {
            // `index` is below the old length, which fits a `u32`.
            *slot = index as u32;
        }
        Some(value)
    }

    /// Removes every value. O(values): the sparse block is left as it is.
    #[inline]
    pub fn clear(&mut self) {
        self.ids.clear();
        self.values.clear();
    }

    /// Keeps only the values for which `keep(id, &mut value)` is `true`.
    /// O(values); the order of the kept values changes.
    pub fn retain(&mut self, mut keep: impl FnMut(u32, &mut T) -> bool) {
        let mut index = 0;
        while let (Some(&id), Some(value)) = (self.ids.get(index), self.values.get_mut(index)) {
            if keep(id, value) {
                index += 1;
            } else {
                // The last value moves here and is looked at next.
                drop(self.remove_at(index));
            }
        }
    }

    /// The ids, in the order of [`values`](Self::values).
    #[inline]
    pub fn ids(&self) -> &[u32] {
        self.ids.as_slice()
    }

    /// The values, packed, in no particular order.
    #[inline]
    pub fn values(&self) -> &[T] {
        self.values.as_slice()
    }

    /// The values mutably, packed.
    #[inline]
    pub fn values_mut(&mut self) -> &mut [T] {
        self.values.as_mut_slice()
    }

    /// Every id with its value.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            inner: self.ids.iter().zip(self.values.iter()),
        }
    }

    /// Every id with its value, mutably.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            inner: self.ids.iter().zip(self.values.iter_mut()),
        }
    }
}

impl<T: fmt::Debug, SS: Storage<u32>, SD: Storage<u32>, SV: Storage<T>> fmt::Debug
    for SparseSet<T, SS, SD, SV>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'s, T, SS: Storage<u32>, SD: Storage<u32>, SV: Storage<T>> IntoIterator
    for &'s SparseSet<T, SS, SD, SV>
{
    type Item = (u32, &'s T);
    type IntoIter = Iter<'s, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'s, T, SS: Storage<u32>, SD: Storage<u32>, SV: Storage<T>> IntoIterator
    for &'s mut SparseSet<T, SS, SD, SV>
{
    type Item = (u32, &'s mut T);
    type IntoIter = IterMut<'s, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Iterator over the ids and values of a [`SparseSet`]. See
/// [`SparseSet::iter`].
#[derive(Clone)]
pub struct Iter<'s, T> {
    inner: Zip<slice::Iter<'s, u32>, slice::Iter<'s, T>>,
}

impl<'s, T> Iterator for Iter<'s, T> {
    type Item = (u32, &'s T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(id, value)| (*id, value))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> FusedIterator for Iter<'_, T> {}

/// Iterator over the ids and values of a [`SparseSet`], mutably. See
/// [`SparseSet::iter_mut`].
pub struct IterMut<'s, T> {
    inner: Zip<slice::Iter<'s, u32>, slice::IterMut<'s, T>>,
}

impl<'s, T> Iterator for IterMut<'s, T> {
    type Item = (u32, &'s mut T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(id, value)| (*id, value))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {}
impl<T> FusedIterator for IterMut<'_, T> {}
