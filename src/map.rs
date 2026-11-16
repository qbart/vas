//! Fixed-capacity unordered map over user-provided memory.

#![allow(unsafe_code)]

use core::borrow::Borrow;
use core::hash::{BuildHasher, BuildHasherDefault, Hash};
use core::mem::{self, MaybeUninit};
use core::{fmt, slice};

use crate::Full;
use crate::hash::FastState;
use crate::storage::{Buffer, Inline, IntoBuffer, Storage};

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

#[inline]
const fn next(index: usize, capacity: usize) -> usize {
    if index + 1 < capacity { index + 1 } else { 0 }
}

/// A fixed-capacity unordered map (hash table) that lives in memory provided
/// by the caller.
///
/// Open addressing with linear probing in one flat block. Lookups get slower
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
pub struct Map<K, V, S: Storage<Slot<K, V>>, H = FastState> {
    // Invariant: a slot's key and value are initialised exactly while its
    // `meta` is not `EMPTY`, and every entry is reachable by probing forward
    // from its home slot without crossing an empty slot.
    //
    // The first `used` slots of the buffer are initialised `Slot`s (whose
    // key and value may themselves be uninitialised); only those are used.
    buffer: S::Buffer,
    used: usize,
    len: usize,
    hasher: H,
}

impl<K, V, const N: usize> Map<K, V, Inline<N>> {
    /// Creates an empty map with `N` slots inside itself, with the default
    /// [`FastHasher`](crate::hash::FastHasher).
    ///
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self::inline_with_hasher(BuildHasherDefault::new())
    }
}

impl<K, V, H, const N: usize> Map<K, V, Inline<N>, H> {
    /// Creates an empty map with `N` slots inside itself and `hasher` to
    /// hash keys.
    #[inline]
    pub const fn inline_with_hasher(hasher: H) -> Self {
        Self {
            buffer: [const {
                MaybeUninit::new(Slot {
                    meta: EMPTY,
                    key: MaybeUninit::uninit(),
                    value: MaybeUninit::uninit(),
                })
            }; N],
            used: if N < u32::MAX as usize {
                N
            } else {
                u32::MAX as usize
            },
            len: 0,
            hasher,
        }
    }
}

impl<K, V, H: Default, const N: usize> Default for Map<K, V, Inline<N>, H> {
    #[inline]
    fn default() -> Self {
        Self::inline_with_hasher(H::default())
    }
}

impl<K, V, S: Storage<Slot<K, V>>> Map<K, V, S> {
    /// Creates an empty map on `buffer`, with the default
    /// [`FastHasher`](crate::hash::FastHasher). A reference to memory
    /// borrows it; a `Memory` passed by value is stored inside the map.
    ///
    /// Marks every slot as empty, so creation is O(capacity).
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Slot<K, V>, Storage = S>,
    {
        Self::with_hasher(buffer, FastState::default())
    }
}

impl<K, V, S: Storage<Slot<K, V>>, H> Map<K, V, S, H> {
    /// Creates an empty map on `buffer` with `hasher` to hash keys.
    ///
    /// Marks every slot as empty, so creation is O(capacity). At most
    /// `u32::MAX` slots are used.
    pub fn with_hasher<B>(buffer: B, hasher: H) -> Self
    where
        B: IntoBuffer<Slot<K, V>, Storage = S>,
    {
        let mut buffer = buffer.into_buffer();
        let slots = buffer.slots_mut();
        let used = slots.len().min(u32::MAX as usize);
        for slot in slots.iter_mut().take(used) {
            slot.write(Slot {
                meta: EMPTY,
                key: MaybeUninit::uninit(),
                value: MaybeUninit::uninit(),
            });
        }
        Self {
            buffer,
            used,
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
        self.used
    }

    /// `true` when the map holds no entries.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` when no new key can be inserted.
    #[inline]
    pub const fn is_full(&self) -> bool {
        self.len == self.used
    }

    /// Removes and drops all entries.
    pub fn clear(&mut self) {
        self.len = 0;
        for slot in live_mut(&mut self.buffer, self.used).iter_mut() {
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
            slots: live(&self.buffer, self.used).iter(),
        }
    }

    /// Iterates over all entries in an unspecified order, with mutable
    /// access to the values.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            slots: live_mut(&mut self.buffer, self.used).iter_mut(),
        }
    }
}

impl<K: Hash + Eq, V, S: Storage<Slot<K, V>>, H: BuildHasher> Map<K, V, S, H> {
    /// Inserts `value` under `key`.
    ///
    /// Returns `Ok(Some(old))` when the key was already present (the value
    /// is replaced, the existing key is kept), `Ok(None)` when it is new, and
    /// hands both back in [`Full`] when the key is new and there is no room.
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, Full<(K, V)>> {
        let meta = self.meta_for(&key);
        let capacity = self.used;
        let mut index = home(meta, capacity);
        for _ in 0..capacity {
            let Some(slot) = live_mut(&mut self.buffer, self.used).get_mut(index) else {
                break;
            };
            if slot.meta == EMPTY {
                slot.key.write(key);
                slot.value.write(value);
                slot.meta = meta;
                self.len += 1;
                return Ok(None);
            }
            // SAFETY: the slot is occupied, so its key is initialised.
            if slot.meta == meta && *unsafe { slot.key.assume_init_ref() } == key {
                // SAFETY: the slot is occupied, so its value is initialised.
                let old = mem::replace(unsafe { slot.value.assume_init_mut() }, value);
                return Ok(Some(old));
            }
            index = next(index, capacity);
        }
        Err(Full((key, value)))
    }

    /// The value stored under `key`.
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = live(&self.buffer, self.used).get(self.find(key)?)?;
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
        let slot = live_mut(&mut self.buffer, self.used).get_mut(index)?;
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
        let capacity = self.used;
        let mut hole = self.find(key)?;
        let slot = live_mut(&mut self.buffer, self.used).get_mut(hole)?;
        slot.meta = EMPTY;
        // SAFETY: `find` only returns occupied slots, so key and value are
        // initialised. The slot was marked empty first, so they are moved
        // out exactly once.
        let entry = unsafe { (slot.key.assume_init_read(), slot.value.assume_init_read()) };
        self.len -= 1;

        // Backward-shift deletion: close the hole by pulling later entries of
        // the same probe run back, so no tombstones are needed and every
        // entry stays reachable from its home slot.
        let mut index = hole;
        for _ in 1..capacity {
            index = next(index, capacity);
            let Some(slot) = live(&self.buffer, self.used).get(index) else {
                break;
            };
            if slot.meta == EMPTY {
                break;
            }
            let home = home(slot.meta, capacity);
            // An entry whose home lies cyclically in `(hole, index]` is still
            // reachable where it is; any other entry must move into the hole.
            let stays = if hole <= index {
                hole < home && home <= index
            } else {
                hole < home || home <= index
            };
            if !stays {
                if let Ok([empty, moved]) =
                    live_mut(&mut self.buffer, self.used).get_disjoint_mut([hole, index])
                {
                    mem::swap(empty, moved);
                }
                hole = index;
            }
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
        let capacity = self.used;
        let mut index = home(meta, capacity);
        for _ in 0..capacity {
            let slot = live(&self.buffer, self.used).get(index)?;
            if slot.meta == EMPTY {
                return None;
            }
            // SAFETY: the slot is occupied, so its key is initialised.
            if slot.meta == meta && unsafe { slot.key.assume_init_ref() }.borrow() == key {
                return Some(index);
            }
            index = next(index, capacity);
        }
        None
    }
}

impl<K, V, S: Storage<Slot<K, V>>, H> Drop for Map<K, V, S, H> {
    #[inline]
    fn drop(&mut self) {
        self.clear();
    }
}

impl<K: fmt::Debug, V: fmt::Debug, S: Storage<Slot<K, V>>, H> fmt::Debug for Map<K, V, S, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'m, K, V, S: Storage<Slot<K, V>>, H> IntoIterator for &'m Map<K, V, S, H> {
    type Item = (&'m K, &'m V);
    type IntoIter = Iter<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'m, K, V, S: Storage<Slot<K, V>>, H> IntoIterator for &'m mut Map<K, V, S, H> {
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

/// The initialised slots of a map's buffer.
#[inline]
fn live<K, V, B: Buffer<Slot<K, V>>>(buffer: &B, used: usize) -> &[Slot<K, V>] {
    // SAFETY: every constructor initialises the first `used` slots of the
    // buffer as `Slot`s and they stay initialised; `MaybeUninit<Slot>` has
    // the layout of `Slot`.
    unsafe { slice::from_raw_parts(buffer.slots().as_ptr().cast::<Slot<K, V>>(), used) }
}

/// The initialised slots of a map's buffer, mutably. Takes the buffer
/// rather than the map so the map's other fields stay usable.
#[inline]
fn live_mut<K, V, B: Buffer<Slot<K, V>>>(buffer: &mut B, used: usize) -> &mut [Slot<K, V>] {
    // SAFETY: as in `live`, and the buffer is borrowed exclusively.
    unsafe { slice::from_raw_parts_mut(buffer.slots_mut().as_mut_ptr().cast::<Slot<K, V>>(), used) }
}
