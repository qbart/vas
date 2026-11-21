//! Least-recently-used cache over borrowed, inline or growing memory.

use core::borrow::Borrow;
use core::fmt;
use core::hash::{BuildHasher, Hash};

use crate::hash::FastState;
use crate::storage::{Inline, IntoBuffer, Storage};
use crate::{Full, Vector};

/// "No entry": an empty table slot, or the end of the recency list.
const NONE: u32 = u32::MAX;

/// A cached key and value with its place in the recency list. Opaque; it
/// only exists so the user can size the memory:
/// `Memory::<Entry<K, V>, N>`.
pub struct Entry<K, V> {
    key: K,
    value: V,
    // Neighbours in the recency list: `prev` was used more recently.
    prev: u32,
    next: u32,
    // The low bits of the key's hash, for probing without hashing again.
    hash: u32,
}

/// A cache of up to `capacity` entries that, when full, makes room by
/// evicting the entry used least recently.
///
/// Lookups by key are O(1) on average (a hash table of indices, linear
/// probing), and so is keeping the recency order (a linked list through
/// the entries). [`get`](Self::get) counts as a use;
/// [`peek`](Self::peek) does not.
///
/// Use it to keep the most useful of many things that are expensive to
/// make: decoded textures and sounds, generated chunks, computed paths.
///
/// Two blocks: one [`Entry`] per cached entry, and a table of `u32` at
/// least a third larger than the capacity (twice the capacity is faster).
///
/// ```
/// use vas::lru::Entry;
/// use vas::{LruCache, Memory};
///
/// let mut entries = Memory::<Entry<u32, &str>, 2>::new();
/// let mut table = Memory::<u32, 4>::new();
/// let Ok(mut chunks) = LruCache::new(&mut entries, &mut table, 2) else {
///     return;
/// };
/// chunks.insert(1, "forest");
/// chunks.insert(2, "lake");
/// assert_eq!(chunks.get(&1), Some(&"forest")); // 1 is now the newest
/// assert_eq!(chunks.insert(3, "hills"), Some((2, "lake"))); // 2 evicted
/// ```
pub struct LruCache<K, V, SE: Storage<Entry<K, V>>, ST: Storage<u32> = SE, H = FastState> {
    // Packed: removing an entry moves the last one into its place.
    entries: Vector<Entry<K, V>, SE>,
    // Power-of-two length, every slot initialised: `NONE`, or an index
    // into `entries` in the low bits (as many as the table size needs),
    // with the entry's hash shifted above it (see `packed`). Indices stay
    // below 3/4 of the table size, so a packed slot is never `NONE`.
    table: Vector<u32, ST>,
    // Most and least recently used, or `NONE` when empty.
    head: u32,
    tail: u32,
    capacity: usize,
    hasher: H,
}

/// The largest number of entries a table of `slots` slots serves: 3/4 of
/// its power-of-two size.
#[inline]
const fn served(slots: usize) -> usize {
    if slots == 0 {
        return 0;
    }
    let size = 1 << slots.ilog2();
    size / 4 * 3 + size % 4 * 3 / 4
}

impl<K: Hash + Eq, V, const E: usize, const T: usize> LruCache<K, V, Inline<E>, Inline<T>> {
    /// Creates an empty cache held inside itself, with the largest
    /// capacity its memory serves: `E`, or 3/4 of `T` rounded down to a
    /// power of two, whichever is smaller.
    pub fn inline() -> Self {
        let capacity = E.min(served(T));
        // Cannot fail: the capacity fits both blocks.
        let mut cache = Self::empty(
            Vector::inline(),
            Vector::inline(),
            capacity,
            FastState::default(),
        );
        cache.fill_table(T.checked_ilog2().map_or(0, |bits| 1 << bits));
        cache
    }
}

impl<K: Hash + Eq, V, const E: usize, const T: usize> Default
    for LruCache<K, V, Inline<E>, Inline<T>>
{
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

impl<K: Hash + Eq, V, SE: Storage<Entry<K, V>>, ST: Storage<u32>> LruCache<K, V, SE, ST> {
    /// Creates an empty cache of up to `capacity` entries on two blocks:
    /// `entries` and `table` (references borrow, `Memory` blocks passed
    /// by value are stored inside, `Dynamic` takes the memory from the
    /// allocator now).
    ///
    /// Fails with [`Full`] when `entries` cannot hold `capacity` entries or
    /// `table`, rounded down to a power of two, has fewer than
    /// `capacity * 4 / 3` slots.
    pub fn new<BE, BT>(entries: BE, table: BT, capacity: usize) -> Result<Self, Full<()>>
    where
        BE: IntoBuffer<Entry<K, V>, Storage = SE>,
        BT: IntoBuffer<u32, Storage = ST>,
    {
        Self::with_hasher(entries, table, capacity, FastState::default())
    }
}

impl<K: Hash + Eq, V, SE: Storage<Entry<K, V>>, ST: Storage<u32>, H: BuildHasher>
    LruCache<K, V, SE, ST, H>
{
    /// Like [`new`](LruCache::new), hashing keys with `hasher`.
    pub fn with_hasher<BE, BT>(
        entries: BE,
        table: BT,
        capacity: usize,
        hasher: H,
    ) -> Result<Self, Full<()>>
    where
        BE: IntoBuffer<Entry<K, V>, Storage = SE>,
        BT: IntoBuffer<u32, Storage = ST>,
    {
        // Indices and `NONE` must fit a `u32`.
        if capacity >= NONE as usize {
            return Err(Full(()));
        }
        let mut entries = Vector::new(entries);
        let mut table = Vector::new(table);
        entries.reserve(capacity)?;
        // At least capacity * 4 / 3 slots, as a power of two; none for a
        // cache that holds nothing.
        let wanted = if capacity == 0 {
            0
        } else {
            capacity
                .checked_add(capacity.div_ceil(3))
                .and_then(usize::checked_next_power_of_two)
                .ok_or(Full(()))?
        };
        // Fixed memory may be larger than needed: use all of it.
        let available = table.capacity().max(wanted);
        let size = available.checked_ilog2().map_or(0, |bits| 1usize << bits);
        if served(size) < capacity {
            return Err(Full(()));
        }
        table.reserve(size)?;
        let mut cache = Self::empty(entries, table, capacity, hasher);
        cache.fill_table(size);
        Ok(cache)
    }

    fn empty(
        entries: Vector<Entry<K, V>, SE>,
        table: Vector<u32, ST>,
        capacity: usize,
        hasher: H,
    ) -> Self {
        Self {
            entries,
            table,
            head: NONE,
            tail: NONE,
            capacity,
            hasher,
        }
    }

    /// Sets the table to `size` empty slots; there is room for them.
    fn fill_table(&mut self, size: usize) {
        self.table.clear();
        let _ = self.table.resize(size, NONE);
    }

    /// Number of cached entries.
    #[inline]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when nothing is cached.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The most entries the cache holds before it evicts.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    fn hash<Q: Hash + ?Sized>(&self, key: &Q) -> u32 {
        // The low bits pick the slot; the table has at most 2^32 slots.
        self.hasher.hash_one(key) as u32
    }

    #[inline]
    fn mask(&self) -> usize {
        self.table.len().wrapping_sub(1)
    }

    /// How many low bits of a table slot hold the entry index: log2 of the
    /// table size.
    #[inline]
    fn shift(&self) -> u32 {
        self.table.len().trailing_zeros()
    }

    /// The bits of a table slot that hold the entry index. (All of them for
    /// a table of 2^32 slots or more, which then keeps no hash bits.)
    #[inline]
    fn index_bits(&self) -> u32 {
        self.mask() as u32
    }

    /// The hash as kept in a table slot: its low bits, shifted above the
    /// index.
    #[inline]
    fn stored(&self, hash: u32) -> u32 {
        hash.checked_shl(self.shift()).unwrap_or(0)
    }

    /// A table slot for entry `index` with hash `hash`. A probe compares the
    /// hash bits with the key's and reads the entry only when they match.
    /// For tables of up to 2^16 slots the hash bits include the entry's
    /// home slot, so moving entries needs no entry read either.
    #[inline]
    fn packed(&self, index: u32, hash: u32) -> u32 {
        self.stored(hash) | index
    }

    /// The table slot and entry index of `key`.
    #[inline]
    fn find<Q>(&self, key: &Q) -> Option<(usize, u32)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if self.entries.is_empty() {
            return None;
        }
        self.find_hashed(key, self.hash(key))
    }

    /// [`find`](Self::find) for a key whose hash is known.
    #[inline]
    fn find_hashed<Q>(&self, key: &Q, hash: u32) -> Option<(usize, u32)>
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        let mask = self.mask();
        let index_bits = self.index_bits();
        let stored = self.stored(hash);
        let mut slot = hash as usize & mask;
        // The table is never full, so an empty slot ends the probe; the
        // bound only guards against a broken invariant.
        for _ in 0..self.table.len() {
            let packed = *self.table.get(slot)?;
            if packed == NONE {
                return None;
            }
            // The upper bits are hash bits: only a match reads the entry.
            if (packed ^ stored) & !index_bits == 0 {
                let index = packed & index_bits;
                if let Some(entry) = self.entries.get(index as usize)
                    && entry.hash == hash
                    && entry.key.borrow() == key
                {
                    return Some((slot, index));
                }
            }
            slot = (slot + 1) & mask;
        }
        None
    }

    /// The table slot holding entry `index`, whose hash is `hash`.
    fn slot_of(&self, index: u32, hash: u32) -> Option<usize> {
        let mask = self.mask();
        let wanted = self.packed(index, hash);
        let mut slot = hash as usize & mask;
        for _ in 0..self.table.len() {
            match *self.table.get(slot)? {
                NONE => return None,
                found if found == wanted => return Some(slot),
                _ => slot = (slot + 1) & mask,
            }
        }
        None
    }

    /// Empties table slot `hole`, shifting later entries of its probe run
    /// back so that every entry stays reachable from its home slot.
    fn clear_slot(&mut self, mut hole: usize) {
        let mask = self.mask();
        let mut slot = (hole + 1) & mask;
        let index_bits = self.index_bits();
        let shift = self.shift();
        // The slot keeps the home's bits when the index leaves room for them.
        let homes_in_table = shift <= 16;
        loop {
            let Some(&packed) = self.table.get(slot) else {
                return;
            };
            if packed == NONE {
                break;
            }
            let home = if homes_in_table {
                (packed >> shift) as usize & mask
            } else {
                let Some(entry) = self.entries.get((packed & index_bits) as usize) else {
                    return;
                };
                entry.hash as usize & mask
            };
            // Move it into the hole when the hole lies between its home and
            // where it is now. Without a branch, which would be mispredicted
            // half the time: an entry that stays is written over itself.
            let moves = (slot.wrapping_sub(home) & mask) >= (slot.wrapping_sub(hole) & mask);
            let target = if moves { hole } else { slot };
            if let Some(target) = self.table.get_mut(target) {
                *target = packed;
            }
            hole = if moves { slot } else { hole };
            slot = (slot + 1) & mask;
        }
        if let Some(target) = self.table.get_mut(hole) {
            *target = NONE;
        }
    }

    /// Takes entry `index` out of the recency list.
    fn unlink(&mut self, index: u32) {
        let Some(entry) = self.entries.get(index as usize) else {
            return;
        };
        let (prev, next) = (entry.prev, entry.next);
        match self.entries.get_mut(prev as usize) {
            Some(before) => before.next = next,
            None => self.head = next,
        }
        match self.entries.get_mut(next as usize) {
            Some(after) => after.prev = prev,
            None => self.tail = prev,
        }
    }

    /// Puts entry `index`, not in the list, at the front (most recent).
    fn link_front(&mut self, index: u32) {
        let head = self.head;
        if let Some(entry) = self.entries.get_mut(index as usize) {
            entry.prev = NONE;
            entry.next = head;
        }
        match self.entries.get_mut(head as usize) {
            Some(first) => first.prev = index,
            None => self.tail = index,
        }
        self.head = index;
    }

    /// Makes entry `index` the most recently used.
    fn touch(&mut self, index: u32) {
        if self.head != index {
            self.unlink(index);
            self.link_front(index);
        }
    }

    /// Removes entry `index`, which is in table slot `slot`, and returns
    /// its key and value.
    fn take(&mut self, slot: usize, index: u32) -> Option<(K, V)> {
        self.unlink(index);
        self.clear_slot(slot);
        let last = u32::try_from(self.entries.len().checked_sub(1)?).ok()?;
        if index != last {
            // The last entry moves into `index`: repoint its table slot and
            // its neighbours.
            let moved = self.entries.get(last as usize)?;
            let (hash, prev, next) = (moved.hash, moved.prev, moved.next);
            let repointed = self.packed(index, hash);
            if let Some(slot) = self.slot_of(last, hash)
                && let Some(target) = self.table.get_mut(slot)
            {
                *target = repointed;
            }
            match self.entries.get_mut(prev as usize) {
                Some(before) => before.next = index,
                None => self.head = index,
            }
            match self.entries.get_mut(next as usize) {
                Some(after) => after.prev = index,
                None => self.tail = index,
            }
        }
        let entry = self.entries.swap_remove(index as usize)?;
        Some((entry.key, entry.value))
    }

    /// `true` when `key` is cached. Not a use.
    #[inline]
    pub fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.find(key).is_some()
    }

    /// The value for `key`, which becomes the most recently used.
    #[inline]
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (_, index) = self.find(key)?;
        self.touch(index);
        self.entries.get(index as usize).map(|entry| &entry.value)
    }

    /// The value for `key`, mutably; it becomes the most recently used.
    #[inline]
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (_, index) = self.find(key)?;
        self.touch(index);
        self.entries
            .get_mut(index as usize)
            .map(|entry| &mut entry.value)
    }

    /// The value for `key`, without counting as a use.
    #[inline]
    pub fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (_, index) = self.find(key)?;
        self.entries.get(index as usize).map(|entry| &entry.value)
    }

    /// Caches `value` for `key` as the most recently used entry.
    ///
    /// Returns what had to go: the old value with its key when `key` was
    /// cached, else the least recently used entry when the cache was full,
    /// else `None`. A cache of capacity 0 returns the new entry itself.
    pub fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        let hash = self.hash(&key);
        if let Some((_, index)) = self.find_hashed(&key, hash) {
            self.touch(index);
            let entry = self.entries.get_mut(index as usize)?;
            let old_key = core::mem::replace(&mut entry.key, key);
            let old_value = core::mem::replace(&mut entry.value, value);
            return Some((old_key, old_value));
        }
        if self.capacity == 0 {
            return Some((key, value));
        }
        if self.entries.len() >= self.capacity {
            return self.replace_lru(key, value, hash);
        }
        // Below the capacity, which is below `NONE`.
        let index = self.entries.len() as u32;
        let entry = Entry {
            key,
            value,
            prev: NONE,
            next: NONE,
            hash,
        };
        if let Err(Full(entry)) = self.entries.push(entry) {
            // Cannot happen: room for `capacity` entries was reserved.
            return Some((entry.key, entry.value));
        }
        self.place(index, hash);
        self.link_front(index);
        None
    }

    /// Puts entry `index`, whose hash is `hash`, into the first free slot of
    /// its probe run.
    fn place(&mut self, index: u32, hash: u32) {
        let mask = self.mask();
        let packed = self.packed(index, hash);
        let mut slot = hash as usize & mask;
        while let Some(target) = self.table.get_mut(slot) {
            if *target == NONE {
                *target = packed;
                return;
            }
            slot = (slot + 1) & mask;
        }
    }

    /// Evicts the least recently used entry by writing the new one over it
    /// in place, and returns the old key and value. Moves no other entry.
    fn replace_lru(&mut self, key: K, value: V, hash: u32) -> Option<(K, V)> {
        let index = self.tail;
        let old_hash = self.entries.get(index as usize)?.hash;
        let Some(slot) = self.slot_of(index, old_hash) else {
            return Some((key, value));
        };
        self.clear_slot(slot);
        let Some(entry) = self.entries.get_mut(index as usize) else {
            return Some((key, value));
        };
        let old_key = core::mem::replace(&mut entry.key, key);
        let old_value = core::mem::replace(&mut entry.value, value);
        entry.hash = hash;
        self.place(index, hash);
        self.touch(index);
        Some((old_key, old_value))
    }

    /// Removes `key` and returns its value.
    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let (slot, index) = self.find(key)?;
        self.take(slot, index).map(|(_, value)| value)
    }

    /// Removes the least recently used entry and returns it.
    pub fn pop_lru(&mut self) -> Option<(K, V)> {
        let index = self.tail;
        let hash = self.entries.get(index as usize)?.hash;
        let slot = self.slot_of(index, hash)?;
        self.take(slot, index)
    }

    /// The least recently used entry: the next to be evicted.
    #[inline]
    pub fn peek_lru(&self) -> Option<(&K, &V)> {
        self.entries
            .get(self.tail as usize)
            .map(|entry| (&entry.key, &entry.value))
    }

    /// Removes every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
        let size = self.table.len();
        self.fill_table(size);
        self.head = NONE;
        self.tail = NONE;
    }

    /// The entries, most recently used first. Not a use.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            entries: self.entries.as_slice(),
            next: self.head,
            left: self.entries.len(),
        }
    }
}

impl<K, V, SE, ST, H> fmt::Debug for LruCache<K, V, SE, ST, H>
where
    K: Hash + Eq + fmt::Debug,
    V: fmt::Debug,
    SE: Storage<Entry<K, V>>,
    ST: Storage<u32>,
    H: BuildHasher,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

/// Iterator over the entries of an [`LruCache`], most recently used first.
/// See [`LruCache::iter`].
pub struct Iter<'c, K, V> {
    entries: &'c [Entry<K, V>],
    next: u32,
    // Bounds the walk, so a broken list can never loop.
    left: usize,
}

impl<'c, K, V> Iterator for Iter<'c, K, V> {
    type Item = (&'c K, &'c V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.left = self.left.checked_sub(1)?;
        let entry = self.entries.get(self.next as usize)?;
        self.next = entry.next;
        Some((&entry.key, &entry.value))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}
