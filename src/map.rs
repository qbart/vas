//! Fixed-capacity unordered map over user-provided memory.

#![allow(unsafe_code)]

use core::borrow::Borrow;
use core::hash::{BuildHasher, Hash};
use core::mem::{self, MaybeUninit};
use core::{fmt, slice};

use crate::Full;
use crate::hash::FastState;

/// `meta` of a slot that holds no entry.
const EMPTY: u32 = 0;
/// Set in `meta` of every slot that holds an entry.
const OCCUPIED: u32 = 1 << 31;

/// One slot of map storage. Opaque; it only exists so the user can size the
/// memory for a [`Map`]: `Memory::<Slot<K, V>, N>`.
pub struct Slot<K, V> {
    // `EMPTY`, or `OCCUPIED` plus 31 bits of the key's hash.
    meta: u32,
    // Both initialised exactly while `meta != EMPTY`.
    key: MaybeUninit<K>,
    value: MaybeUninit<V>,
}

/// The slot an entry with this `meta` would ideally live in.
#[inline]
const fn home(meta: u32, capacity: usize) -> usize {
    // Maps the 31 hash bits onto `0..capacity` without a division.
    // `capacity <= u32::MAX`, so the product fits in a `u64` and the result
    // is always below `capacity`.
    (((meta & !OCCUPIED) as u64 * capacity as u64) >> 31) as usize
}

/// How many slots past its home the entry with this `meta` sits when it is
/// stored at `index`.
#[inline]
const fn probe_distance(meta: u32, index: usize, capacity: usize) -> usize {
    let home = home(meta, capacity);
    if index >= home {
        index - home
    } else {
        index + capacity - home
    }
}

#[inline]
const fn next(index: usize, capacity: usize) -> usize {
    if index + 1 < capacity { index + 1 } else { 0 }
}

/// A fixed-capacity unordered map (hash table) that lives in memory provided
/// by the caller.
///
/// Open addressing with Robin Hood linear probing in one flat block. Lookups get slower
/// as the map fills up, so give it more room than the number of entries you
/// expect (twice as much is a good default).
///
/// ```
/// use vas::map::Slot;
/// use vas::{Map, Memory};
///
/// let mut memory = Memory::<Slot<u32, &str>, 64>::new();
/// let mut map = Map::new(&mut memory);
///
/// assert_eq!(map.insert(7, "sword"), Ok(None));
/// assert_eq!(map.get(&7), Some(&"sword"));
/// assert_eq!(map.remove(&7), Some("sword"));
/// ```
pub struct Map<'a, K, V, S = FastState> {
    // Invariants:
    // - a slot's key and value are initialised exactly while its `meta` is
    //   not `EMPTY`.
    // - every entry is reachable by probing forward from its home slot
    //   without crossing an empty slot.
    // - Robin Hood order: along such a run, no entry is closer to its home
    //   than the entry before it is to its own, minus one. Lookups rely on
    //   this to stop early.
    slots: &'a mut [Slot<K, V>],
    len: usize,
    hasher: S,
}

impl<'a, K, V> Map<'a, K, V> {
    /// Creates an empty map using `storage` as its memory, with the default
    /// [`FastHasher`](crate::hash::FastHasher).
    ///
    /// Marks every slot as empty, so creation is O(capacity).
    #[inline]
    pub fn new(storage: &'a mut [MaybeUninit<Slot<K, V>>]) -> Self {
        Self::with_hasher(storage, FastState::default())
    }
}

impl<'a, K, V, S> Map<'a, K, V, S> {
    /// Creates an empty map using `storage` as its memory and `hasher` to
    /// hash keys.
    ///
    /// Marks every slot as empty, so creation is O(capacity). At most
    /// `u32::MAX` slots are used.
    pub fn with_hasher(storage: &'a mut [MaybeUninit<Slot<K, V>>], hasher: S) -> Self {
        let used = storage.len().min(u32::MAX as usize);
        let storage = match storage.get_mut(..used) {
            Some(storage) => storage,
            None => &mut [],
        };
        for slot in storage.iter_mut() {
            slot.write(Slot {
                meta: EMPTY,
                key: MaybeUninit::uninit(),
                value: MaybeUninit::uninit(),
            });
        }
        // SAFETY: every element was just initialised as a `Slot` (whose key
        // and value are themselves allowed to be uninitialised), and
        // `MaybeUninit<Slot>` has the layout of `Slot`.
        let slots = unsafe {
            slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<Slot<K, V>>(), storage.len())
        };
        Self {
            slots,
            len: 0,
            hasher,
        }
    }

    /// Number of entries in the map.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Maximum number of entries the map can hold.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// `true` when the map holds no entries.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` when no new key can be inserted.
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.len == self.slots.len()
    }

    /// Removes and drops all entries.
    pub fn clear(&mut self) {
        self.len = 0;
        for slot in self.slots.iter_mut() {
            if slot.meta != EMPTY {
                // Mark the slot empty before dropping, so a panicking `Drop`
                // cannot lead to a double drop.
                slot.meta = EMPTY;
                // SAFETY: the slot was occupied, so key and value are
                // initialised, and it is now marked empty, so they are
                // dropped exactly once.
                unsafe {
                    slot.key.assume_init_drop();
                    slot.value.assume_init_drop();
                }
            }
        }
    }

    /// Iterates over all entries in an unspecified order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            slots: self.slots.iter(),
        }
    }

    /// Iterates over all entries in an unspecified order, with mutable
    /// access to the values.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            slots: self.slots.iter_mut(),
        }
    }
}

impl<K: Hash + Eq, V, S: BuildHasher> Map<'_, K, V, S> {
    /// Inserts `value` under `key`.
    ///
    /// Returns `Ok(Some(old))` when the key was already present (the value
    /// is replaced, the existing key is kept), `Ok(None)` when it is new, and
    /// hands both back in [`Full`] when the key is new and there is no room.
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, Full<(K, V)>> {
        let meta = self.meta_for(&key);
        let capacity = self.slots.len();
        let mut index = home(meta, capacity);
        let mut distance = 0;

        // Look for the key, or for the place where it belongs: the first
        // slot that is empty or holds an entry closer to its own home.
        let mut has_place = false;
        for _ in 0..capacity {
            let Some(slot) = self.slots.get_mut(index) else {
                break;
            };
            if slot.meta == EMPTY {
                has_place = true;
                break;
            }
            // SAFETY: the slot is occupied, so its key is initialised.
            if slot.meta == meta && *unsafe { slot.key.assume_init_ref() } == key {
                // SAFETY: the slot is occupied, so its value is initialised.
                let old = mem::replace(unsafe { slot.value.assume_init_mut() }, value);
                return Ok(Some(old));
            }
            if probe_distance(slot.meta, index, capacity) < distance {
                has_place = true;
                break;
            }
            index = next(index, capacity);
            distance += 1;
        }
        if !has_place || self.len == capacity {
            return Err(Full((key, value)));
        }

        // Robin Hood insertion: the entry being carried takes the slot of
        // any entry that is closer to its home, and that one is carried on.
        // No user code runs here, so the table cannot be left half updated.
        let mut carried = Slot {
            meta,
            key: MaybeUninit::new(key),
            value: MaybeUninit::new(value),
        };
        for _ in 0..capacity {
            let Some(slot) = self.slots.get_mut(index) else {
                break;
            };
            if slot.meta == EMPTY {
                *slot = carried;
                self.len += 1;
                return Ok(None);
            }
            let resident = probe_distance(slot.meta, index, capacity);
            if resident < distance {
                mem::swap(slot, &mut carried);
                distance = resident;
            }
            index = next(index, capacity);
            distance += 1;
        }
        // Not reachable: the map is not full, so the loop above finds an
        // empty slot within `capacity` steps.
        Ok(None)
    }

    /// The value stored under `key`.
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.slots.get(self.find(key)?)?;
        // SAFETY: `find` only returns occupied slots.
        Some(unsafe { slot.value.assume_init_ref() })
    }

    /// The value stored under `key`, mutably.
    #[inline]
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let index = self.find(key)?;
        let slot = self.slots.get_mut(index)?;
        // SAFETY: `find` only returns occupied slots.
        Some(unsafe { slot.value.assume_init_mut() })
    }

    /// `true` when the map has an entry for `key`.
    #[inline]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.find(key).is_some()
    }

    /// Removes the entry for `key` and returns its value.
    #[inline]
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.remove_entry(key).map(|(_, value)| value)
    }

    /// Removes the entry for `key` and returns its key and value.
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let capacity = self.slots.len();
        let mut hole = self.find(key)?;
        let slot = self.slots.get_mut(hole)?;
        slot.meta = EMPTY;
        // SAFETY: `find` only returns occupied slots, so key and value are
        // initialised. The slot was marked empty first, so they are moved
        // out exactly once.
        let entry = unsafe { (slot.key.assume_init_read(), slot.value.assume_init_read()) };
        self.len -= 1;

        // Backward-shift deletion: pull the following entries back by one
        // until an empty slot or an entry that is already at its home, so
        // no tombstones are needed.
        for _ in 1..capacity {
            let following = next(hole, capacity);
            let Some(slot) = self.slots.get(following) else {
                break;
            };
            if slot.meta == EMPTY || probe_distance(slot.meta, following, capacity) == 0 {
                break;
            }
            if let Ok([empty, moved]) = self.slots.get_disjoint_mut([hole, following]) {
                mem::swap(empty, moved);
            }
            hole = following;
        }
        Some(entry)
    }

    /// `meta` for an entry with this key: the occupied bit plus the top 31
    /// bits of the hash.
    #[inline]
    fn meta_for<Q: Hash + ?Sized>(&self, key: &Q) -> u32 {
        ((self.hasher.hash_one(key) >> 33) as u32) | OCCUPIED
    }

    /// Index of the occupied slot holding `key`.
    #[inline]
    fn find<Q>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let meta = self.meta_for(key);
        let capacity = self.slots.len();
        let mut index = home(meta, capacity);
        for distance in 0..capacity {
            let slot = self.slots.get(index)?;
            if slot.meta == EMPTY {
                return None;
            }
            // SAFETY: the slot is occupied, so its key is initialised.
            if slot.meta == meta && unsafe { slot.key.assume_init_ref() }.borrow() == key {
                return Some(index);
            }
            // An entry closer to its home than we are to ours would have
            // been displaced by the key, so the key is not in the map.
            if probe_distance(slot.meta, index, capacity) < distance {
                return None;
            }
            index = next(index, capacity);
        }
        None
    }
}

impl<K, V, S> Drop for Map<'_, K, V, S> {
    #[inline]
    fn drop(&mut self) {
        self.clear();
    }
}

impl<K: fmt::Debug, V: fmt::Debug, S> fmt::Debug for Map<'_, K, V, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'m, K, V, S> IntoIterator for &'m Map<'_, K, V, S> {
    type Item = (&'m K, &'m V);
    type IntoIter = Iter<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'m, K, V, S> IntoIterator for &'m mut Map<'_, K, V, S> {
    type Item = (&'m K, &'m mut V);
    type IntoIter = IterMut<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Iterator over the entries of a [`Map`]. See [`Map::iter`].
pub struct Iter<'m, K, V> {
    slots: slice::Iter<'m, Slot<K, V>>,
}

impl<'m, K, V> Iterator for Iter<'m, K, V> {
    type Item = (&'m K, &'m V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for slot in self.slots.by_ref() {
            if slot.meta != EMPTY {
                // SAFETY: an occupied slot holds an initialised key and
                // value.
                return Some(unsafe { (slot.key.assume_init_ref(), slot.value.assume_init_ref()) });
            }
        }
        None
    }
}

/// Iterator over the entries of a [`Map`] with mutable values. See
/// [`Map::iter_mut`].
pub struct IterMut<'m, K, V> {
    slots: slice::IterMut<'m, Slot<K, V>>,
}

impl<'m, K, V> Iterator for IterMut<'m, K, V> {
    type Item = (&'m K, &'m mut V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for slot in self.slots.by_ref() {
            if slot.meta != EMPTY {
                // SAFETY: an occupied slot holds an initialised key and
                // value.
                return Some(unsafe { (slot.key.assume_init_ref(), slot.value.assume_init_mut()) });
            }
        }
        None
    }
}
