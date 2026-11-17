//! Unordered map over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::borrow::Borrow;
use core::hash::{BuildHasher, BuildHasherDefault, Hash};
use core::iter::Zip;
use core::mem::{self, MaybeUninit};
use core::{fmt, ptr, slice};

use crate::Full;
use crate::allocator::Allocator;
use crate::hash::FastState;
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};

/// Tag of a slot that holds no entry. Tags of entries have the top bit set.
const EMPTY: u8 = 0;
/// The lowest bit of every byte of a group.
const LOW: u64 = 0x0101_0101_0101_0101;
/// The top bit of every byte of a group.
const HIGH: u64 = 0x8080_8080_8080_8080;
/// Number of tags looked at in one step.
const GROUP: usize = 8;

/// One slot of map storage. Opaque; it only exists so the user can size the
/// memory for a [`Map`]: `Memory::<Slot<K, V>, N>`.
pub struct Slot<K, V> {
    key: K,
    value: V,
}

/// Number of entries up to which a growable map of `capacity` slots is not
/// yet crowded: 3/4 of the slots.
#[inline]
const fn roomy(capacity: usize) -> usize {
    capacity - capacity / 4
}

/// The slot an entry with this hash would ideally live in.
#[inline]
const fn home(hash: u64, capacity: usize) -> usize {
    // Maps the upper 32 hash bits onto `0..capacity` without a division.
    // `capacity <= u32::MAX`, so the product fits in a `u64` and the result
    // is always below `capacity`.
    (((hash >> 32) * capacity as u64) >> 32) as usize
}

/// The tag of an entry with this hash: the top bit plus 7 hash bits that
/// `home` uses least.
#[inline]
const fn tag(hash: u64) -> u8 {
    (hash >> 25) as u8 | 0x80
}

#[inline]
const fn next(index: usize, capacity: usize) -> usize {
    if index + 1 < capacity { index + 1 } else { 0 }
}

/// `index` brought back into `0..capacity`; `capacity` is not zero.
#[inline]
const fn wrap(index: usize, capacity: usize) -> usize {
    if index < capacity {
        index
    } else if index - capacity < capacity {
        index - capacity
    } else {
        index % capacity
    }
}

/// The `GROUP` tags starting at `position`, wrapping around the end, with
/// the first tag in the lowest byte. `position` is below `tags.len()`.
#[inline]
fn group(tags: &[u8], position: usize) -> u64 {
    if position + GROUP <= tags.len() {
        // SAFETY: the `GROUP` bytes starting at `position` are inside
        // `tags`, as just checked; the read needs no alignment.
        u64::from_le(unsafe { tags.as_ptr().add(position).cast::<u64>().read_unaligned() })
    } else {
        group_at_end(tags, position)
    }
}

/// [`group`] for a position less than `GROUP` tags before the end.
#[inline(never)]
fn group_at_end(tags: &[u8], position: usize) -> u64 {
    if let (Some(first), Some(last)) = (tags.first_chunk::<GROUP>(), tags.last_chunk::<GROUP>()) {
        // The tags up to the end are the upper bytes of the last group; the
        // rest comes from the start.
        let before_end = tags.len().saturating_sub(position).clamp(1, GROUP - 1);
        let tail = u64::from_le_bytes(*last) >> (8 * (GROUP - before_end));
        let head = u64::from_le_bytes(*first) << (8 * before_end);
        return tail | head;
    }
    // Fewer tags than a group: some appear more than once, which only
    // makes the caller look at a slot twice.
    let mut bytes = [EMPTY; GROUP];
    let mut index = position;
    for byte in &mut bytes {
        *byte = tags.get(index).copied().unwrap_or(EMPTY);
        index = next(index, tags.len());
    }
    u64::from_le_bytes(bytes)
}

/// The top bit of every byte of `group` that is `EMPTY`.
#[inline]
const fn empties(group: u64) -> u64 {
    !group & HIGH
}

/// The top bit of every byte of `group` that equals `tag`. A byte above a
/// match may be marked too when it differs from `tag` in its lowest bit
/// only; callers compare keys anyway. An `EMPTY` byte is never marked:
/// it differs from `tag` in its top bit.
#[inline]
const fn matches(group: u64, tag: u8) -> u64 {
    let diff = group ^ (LOW * tag as u64);
    diff.wrapping_sub(LOW) & !diff & HIGH
}

/// Offset in the group of the lowest marked byte.
#[inline]
const fn first(marks: u64) -> usize {
    (marks.trailing_zeros() / 8) as usize
}

/// The first empty slot at or after `home`, wrapping around.
#[inline]
fn first_empty(tags: &[u8], home: usize) -> Option<usize> {
    let capacity = tags.len();
    let mut position = home;
    let mut left = capacity;
    while left > 0 {
        let empty = empties(group(tags, position));
        if empty != 0 {
            return Some(wrap(position + first(empty), capacity));
        }
        left = left.saturating_sub(GROUP);
        position = wrap(position + GROUP, capacity);
    }
    None
}

/// An unordered map (hash table) over memory provided by the caller.
///
/// Open addressing with linear probing. The map uses two blocks of memory
/// of the same length: one [`Slot`] per entry for keys and values, and one
/// byte per entry for a tag (a few bits of the key's hash). A lookup scans
/// the tags eight at a time and only looks at a key when its tag matches,
/// so finding a key stays fast when the map is nearly full. Looking up
/// keys that are **not** in the map slows down past 3/4 full; leave a
/// quarter of the slots free when that is common.
///
/// ```
/// use vas::map::Slot;
/// use vas::{Map, Memory};
///
/// let mut slots = Memory::<Slot<u32, &str>, 64>::new();
/// let mut tags = Memory::<u8, 64>::new();
/// let mut map = Map::new(&mut slots, &mut tags);
///
/// assert_eq!(map.insert(7, "sword"), Ok(None));
/// assert_eq!(map.get(&7), Some(&"sword"));
/// assert_eq!(map.remove(&7), Some("sword"));
/// ```
///
/// With [`Inline`] storage both blocks are inside the map, and with
/// [`Dynamic`] storage they come from an allocator and the map grows when
/// it runs out of room.
pub struct Map<K, V, S: Storage<Slot<K, V>> + Storage<u8>, H = FastState> {
    // Invariant: both buffers have at least `used` slots. The first `used`
    // tags are initialised. Slot `i < used` holds an initialised key and
    // value exactly while tag `i` is not `EMPTY`, and that tag is then
    // `tag(hash of the key)`. Every entry is reachable by walking forward
    // from its home slot without crossing an empty slot.
    slots: <S as Storage<Slot<K, V>>>::Buffer,
    tags: <S as Storage<u8>>::Buffer,
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
            slots: [const { MaybeUninit::uninit() }; N],
            tags: [MaybeUninit::new(EMPTY); N],
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

/// Clones the entries and the hasher into a map of the same capacity. Only
/// maps with inline storage can be cloned.
impl<K: Clone, V: Clone, H: Clone, const N: usize> Clone for Map<K, V, Inline<N>, H> {
    fn clone(&self) -> Self {
        let mut copy = Self::inline_with_hasher(self.hasher.clone());
        // Same capacity and same hasher: every entry keeps its slot.
        let tags = live(&self.tags, self.used);
        for (((tag, slot), new_tag), new_slot) in tags
            .iter()
            .zip(&self.slots)
            .zip(&mut copy.tags)
            .zip(&mut copy.slots)
        {
            if *tag == EMPTY {
                continue;
            }
            // SAFETY: a slot whose tag is not `EMPTY` is initialised.
            let slot = unsafe { slot.assume_init_ref() };
            new_slot.write(Slot {
                key: slot.key.clone(),
                value: slot.value.clone(),
            });
            // Marked one by one, so a panicking `clone` leaks nothing.
            new_tag.write(*tag);
            copy.len += 1;
        }
        copy
    }
}

impl<K, V, H: Default, const N: usize> Default for Map<K, V, Inline<N>, H> {
    #[inline]
    fn default() -> Self {
        Self::inline_with_hasher(H::default())
    }
}

impl<K, V, A: Allocator + Default, H: Default> Default for Map<K, V, Dynamic<A>, H> {
    /// Creates an empty map that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::with_hasher(Dynamic(A::default()), Dynamic(A::default()), H::default())
    }
}

impl<K, V, S: Storage<Slot<K, V>> + Storage<u8>> Map<K, V, S> {
    /// Creates an empty map on `slots` and `tags`, with the default
    /// [`FastHasher`](crate::hash::FastHasher). References to memory
    /// borrow it; `Memory` blocks passed by value are stored inside the
    /// map; `Dynamic(allocator)`, passed twice, grows.
    ///
    /// The capacity is the length of the shorter block. Marks every slot
    /// as empty, so creation is O(capacity).
    #[inline]
    pub fn new<BS, BT>(slots: BS, tags: BT) -> Self
    where
        BS: IntoBuffer<Slot<K, V>, Storage = S>,
        BT: IntoBuffer<u8, Storage = S>,
    {
        Self::with_hasher(slots, tags, FastState::default())
    }
}

impl<K, V, S: Storage<Slot<K, V>> + Storage<u8>, H> Map<K, V, S, H> {
    /// Creates an empty map on `slots` and `tags` with `hasher` to hash
    /// keys.
    ///
    /// The capacity is the length of the shorter block, and at most
    /// `u32::MAX`. Marks every slot as empty, so creation is O(capacity).
    pub fn with_hasher<BS, BT>(slots: BS, tags: BT, hasher: H) -> Self
    where
        BS: IntoBuffer<Slot<K, V>, Storage = S>,
        BT: IntoBuffer<u8, Storage = S>,
    {
        let slots = slots.into_buffer();
        let mut tags = tags.into_buffer();
        let used = slots
            .slots()
            .len()
            .min(tags.slots().len())
            .min(u32::MAX as usize);
        if let Some(tags) = tags.slots_mut().get_mut(..used) {
            tags.fill(MaybeUninit::new(EMPTY));
        }
        Self {
            slots,
            tags,
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

    /// Number of slots: the maximum number of entries for borrowed and
    /// inline storage. With dynamic storage the map grows before it is
    /// this full (at 3/4).
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
        let tags = live_mut(&mut self.tags, self.used);
        if mem::needs_drop::<Slot<K, V>>() {
            for (tag, slot) in tags.iter_mut().zip(self.slots.slots_mut()) {
                if *tag != EMPTY {
                    // Mark the slot empty before dropping, so a panicking
                    // `Drop` cannot lead to a double drop.
                    *tag = EMPTY;
                    // SAFETY: the slot was occupied, so it is initialised,
                    // and it is now marked empty, so it is dropped exactly
                    // once.
                    unsafe { slot.assume_init_drop() };
                }
            }
        } else {
            tags.fill(EMPTY);
        }
    }

    /// Iterates over all entries in an unspecified order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        Iter {
            slots: live(&self.tags, self.used)
                .iter()
                .zip(self.slots.slots().iter()),
        }
    }

    /// Iterates over all entries in an unspecified order, with mutable
    /// access to the values.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
        IterMut {
            slots: live(&self.tags, self.used)
                .iter()
                .zip(self.slots.slots_mut().iter_mut()),
        }
    }
}

impl<K: Hash + Eq, V, S: Storage<Slot<K, V>> + Storage<u8>, H: BuildHasher> Map<K, V, S, H> {
    /// `true` when the storage can grow.
    const GROWS: bool = <<S as Storage<u8>>::Buffer as Buffer<u8>>::GROWS;

    /// Makes sure `additional` more entries fit, growing dynamic storage
    /// when needed. Fails without changes when the storage is fixed and too
    /// small, or when the allocator refuses.
    pub fn reserve(&mut self, additional: usize) -> Result<(), Full<()>> {
        let entries = self.len.checked_add(additional).ok_or(Full(()))?;
        let crowded = Self::GROWS && entries > roomy(self.used);
        if entries <= self.used && !crowded {
            return Ok(());
        }
        if self.grow(entries) && entries <= self.used {
            Ok(())
        } else {
            Err(Full(()))
        }
    }

    /// Moves the map to bigger storage, sized so that `entries` entries
    /// leave it at most 3/4 full. Returns `false`, with nothing changed,
    /// when the storage cannot grow.
    #[cold]
    fn grow(&mut self, entries: usize) -> bool {
        let Some(min) = entries.checked_add(entries / 3 + 1) else {
            return false;
        };
        let old_used = self.used;
        let hasher = &self.hasher;
        let slots = &mut self.slots;
        let mut used = old_used;
        let grown = self.tags.replace(min, |old_tags, new_tags| {
            // SAFETY: the first `old_used` tags are initialised by the
            // map's invariant; `MaybeUninit<u8>` has the layout of `u8`.
            let old_tags = unsafe { slice::from_raw_parts(old_tags.as_ptr().cast(), old_used) };
            if size_of::<Slot<K, V>>() == 0 {
                // Entries without size live nowhere, so there is nothing
                // to move: only the tags are placed again.
                if !slots.grow(min) {
                    return false;
                }
                let old_slots = slots.slots();
                // SAFETY: a dangling pointer is valid for any number of
                // zero-sized slots.
                let new_slots = unsafe {
                    slice::from_raw_parts_mut(ptr::NonNull::dangling().as_ptr(), old_slots.len())
                };
                used = rehash(hasher, old_tags, old_slots, new_tags, new_slots);
                true
            } else {
                slots.replace(min, |old_slots, new_slots| {
                    used = rehash(hasher, old_tags, old_slots, new_tags, new_slots);
                    true
                })
            }
        });
        if grown {
            self.used = used;
        }
        grown
    }

    /// Inserts `value` under `key`.
    ///
    /// Returns `Ok(Some(old))` when the key was already present (the value
    /// is replaced, the existing key is kept), `Ok(None)` when it is new, and
    /// hands both back in [`Full`] when the key is new and there is no room.
    ///
    /// With dynamic storage the map grows when it gets more than 3/4 full,
    /// so there is no room only when the allocator refuses and every slot
    /// is taken.
    #[inline]
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, Full<(K, V)>> {
        let hash = self.hasher.hash_one(&key);
        let tag = tag(hash);
        let capacity = self.used;
        let tags = live_mut(&mut self.tags, capacity);
        let slots = self.slots.slots_mut();
        let mut position = home(hash, capacity);
        let mut left = capacity;
        while left > 0 {
            let group = group(tags, position);
            let mut hits = matches(group, tag);
            while hits != 0 {
                let index = wrap(position + first(hits), capacity);
                // SAFETY: `wrap` returns an index below `capacity`, and
                // both buffers have at least that many slots. `matches`
                // only marks tags that are not `EMPTY` (see there), so the
                // slot is initialised.
                let slot = unsafe { slots.get_unchecked_mut(index).assume_init_mut() };
                if slot.key == key {
                    return Ok(Some(mem::replace(&mut slot.value, value)));
                }
                hits &= hits - 1;
            }
            let empty = empties(group);
            if empty != 0 {
                if Self::GROWS && self.len >= roomy(capacity) {
                    // The key is new and the map is crowded: grow first.
                    break;
                }
                let index = wrap(position + first(empty), capacity);
                if let (Some(slot), Some(mark)) = (slots.get_mut(index), tags.get_mut(index)) {
                    slot.write(Slot { key, value });
                    *mark = tag;
                    self.len += 1;
                    return Ok(None);
                }
                break;
            }
            left = left.saturating_sub(GROUP);
            position = wrap(position + GROUP, capacity);
        }
        self.insert_new(hash, key, value)
    }

    /// Inserts an entry whose key is known to be absent, after trying to
    /// grow the storage.
    #[cold]
    fn insert_new(&mut self, hash: u64, key: K, value: V) -> Result<Option<V>, Full<(K, V)>> {
        // When growing fails the entry may still fit in the slots there
        // are: a map that cannot grow fills up completely.
        if !self.grow(self.len.saturating_add(1)) && self.len >= self.used {
            return Err(Full((key, value)));
        }
        let capacity = self.used;
        let tags = live_mut(&mut self.tags, capacity);
        let index = first_empty(tags, home(hash, capacity));
        let place = index
            .and_then(|index| Some((self.slots.slots_mut().get_mut(index)?, tags.get_mut(index)?)));
        match place {
            Some((slot, mark)) => {
                slot.write(Slot { key, value });
                *mark = tag(hash);
                self.len += 1;
                Ok(None)
            }
            None => Err(Full((key, value))),
        }
    }

    /// The value stored under `key`.
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.slots.slots().get(self.find(key)?)?;
        // SAFETY: `find` only returns occupied slots.
        Some(&unsafe { slot.assume_init_ref() }.value)
    }

    /// The value stored under `key`, mutably.
    #[inline]
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let index = self.find(key)?;
        let slot = self.slots.slots_mut().get_mut(index)?;
        // SAFETY: `find` only returns occupied slots.
        Some(&mut unsafe { slot.assume_init_mut() }.value)
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
    ///
    /// The entries right after it are moved up to close the gap, which
    /// hashes their keys again.
    pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let capacity = self.used;
        let mut hole = self.find(key)?;
        let tags = live_mut(&mut self.tags, capacity);
        let slots = self.slots.slots_mut();
        *tags.get_mut(hole)? = EMPTY;
        // SAFETY: `find` only returns occupied slots, so the slot is
        // initialised. It was marked empty first, so it is moved out
        // exactly once.
        let Slot { key, value } = unsafe { slots.get(hole)?.assume_init_read() };
        self.len -= 1;

        // Backward-shift deletion: close the hole by pulling later entries of
        // the same probe run back, so no tombstones are needed and every
        // entry stays reachable from its home slot.
        let mut index = hole;
        for _ in 1..capacity {
            index = next(index, capacity);
            let (Some(mark), Some(slot)) = (tags.get(index).copied(), slots.get(index)) else {
                break;
            };
            if mark == EMPTY {
                break;
            }
            // SAFETY: the slot's tag is not `EMPTY`, so it is initialised.
            let entry = unsafe { slot.assume_init_ref() };
            let home = home(self.hasher.hash_one(&entry.key), capacity);
            // An entry whose home lies cyclically in `(hole, index]` is still
            // reachable where it is; any other entry must move into the hole.
            let stays = if hole <= index {
                hole < home && home <= index
            } else {
                hole < home || home <= index
            };
            if !stays {
                if let Ok([empty, moved]) = slots.get_disjoint_mut([hole, index]) {
                    mem::swap(empty, moved);
                }
                if let Ok([empty, moved]) = tags.get_disjoint_mut([hole, index]) {
                    mem::swap(empty, moved);
                }
                hole = index;
            }
        }
        Some((key, value))
    }

    /// Index of the occupied slot holding `key`.
    #[inline]
    fn find<Q>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let hash = self.hasher.hash_one(key);
        let tag = tag(hash);
        let capacity = self.used;
        let tags = live(&self.tags, capacity);
        let slots = self.slots.slots();
        let mut position = home(hash, capacity);
        let mut left = capacity;
        while left > 0 {
            let group = group(tags, position);
            let mut hits = matches(group, tag);
            while hits != 0 {
                let index = wrap(position + first(hits), capacity);
                // SAFETY: `wrap` returns an index below `capacity`, and
                // both buffers have at least that many slots. `matches`
                // only marks tags that are not `EMPTY` (see there), so the
                // slot is initialised.
                let slot = unsafe { slots.get_unchecked(index).assume_init_ref() };
                if slot.key.borrow() == key {
                    return Some(index);
                }
                hits &= hits - 1;
            }
            if empties(group) != 0 {
                return None;
            }
            left = left.saturating_sub(GROUP);
            position = wrap(position + GROUP, capacity);
        }
        None
    }
}

/// Fills new storage with the entries of the old one and returns its
/// capacity. `old_tags` are the initialised tags of the old storage.
fn rehash<K: Hash, V, H: BuildHasher>(
    hasher: &H,
    old_tags: &[u8],
    old_slots: &[MaybeUninit<Slot<K, V>>],
    new_tags: &mut [MaybeUninit<u8>],
    new_slots: &mut [MaybeUninit<Slot<K, V>>],
) -> usize {
    let used = new_tags.len().min(new_slots.len()).min(u32::MAX as usize);
    let Some(new_tags) = new_tags.get_mut(..used) else {
        return 0;
    };
    new_tags.fill(MaybeUninit::new(EMPTY));
    // SAFETY: all of `new_tags` was just initialised; `MaybeUninit<u8>` has
    // the layout of `u8`.
    let new_tags = unsafe { slice::from_raw_parts_mut(new_tags.as_mut_ptr().cast::<u8>(), used) };
    for (mark, entry) in old_tags.iter().zip(old_slots) {
        if *mark == EMPTY {
            continue;
        }
        // SAFETY: the tag is not `EMPTY`, so the slot is initialised.
        let hash = hasher.hash_one(&unsafe { entry.assume_init_ref() }.key);
        let Some(index) = first_empty(new_tags, home(hash, used)) else {
            continue;
        };
        if let (Some(slot), Some(new_mark)) = (new_slots.get_mut(index), new_tags.get_mut(index)) {
            // SAFETY: both are valid, distinct slots. The entry is moved
            // bit by bit; the old storage is given up without being read
            // or dropped, so the new slot becomes the only owner.
            unsafe { ptr::copy_nonoverlapping(entry, slot, 1) };
            *new_mark = *mark;
        }
    }
    used
}

impl<K, V, S: Storage<Slot<K, V>> + Storage<u8>, H> Drop for Map<K, V, S, H> {
    #[inline]
    fn drop(&mut self) {
        // Storage handed back to the user needs no tidying, so plain data
        // is left as it is.
        if mem::needs_drop::<Slot<K, V>>() {
            self.clear();
        }
    }
}

impl<K: fmt::Debug, V: fmt::Debug, S: Storage<Slot<K, V>> + Storage<u8>, H> fmt::Debug
    for Map<K, V, S, H>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'m, K, V, S: Storage<Slot<K, V>> + Storage<u8>, H> IntoIterator for &'m Map<K, V, S, H> {
    type Item = (&'m K, &'m V);
    type IntoIter = Iter<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'m, K, V, S: Storage<Slot<K, V>> + Storage<u8>, H> IntoIterator for &'m mut Map<K, V, S, H> {
    type Item = (&'m K, &'m mut V);
    type IntoIter = IterMut<'m, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Iterator over the entries of a [`Map`]. See [`Map::iter`].
pub struct Iter<'m, K, V> {
    slots: Zip<slice::Iter<'m, u8>, slice::Iter<'m, MaybeUninit<Slot<K, V>>>>,
}

impl<'m, K, V> Iterator for Iter<'m, K, V> {
    type Item = (&'m K, &'m V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for (tag, slot) in self.slots.by_ref() {
            if *tag != EMPTY {
                // SAFETY: a slot whose tag is not `EMPTY` is initialised.
                let slot = unsafe { slot.assume_init_ref() };
                return Some((&slot.key, &slot.value));
            }
        }
        None
    }
}

/// Iterator over the entries of a [`Map`] with mutable values. See
/// [`Map::iter_mut`].
pub struct IterMut<'m, K, V> {
    slots: Zip<slice::Iter<'m, u8>, slice::IterMut<'m, MaybeUninit<Slot<K, V>>>>,
}

impl<'m, K, V> Iterator for IterMut<'m, K, V> {
    type Item = (&'m K, &'m mut V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for (tag, slot) in self.slots.by_ref() {
            if *tag != EMPTY {
                // SAFETY: a slot whose tag is not `EMPTY` is initialised.
                let slot = unsafe { slot.assume_init_mut() };
                return Some((&slot.key, &mut slot.value));
            }
        }
        None
    }
}

/// The initialised tags of a map.
#[inline]
fn live<B: Buffer<u8>>(tags: &B, used: usize) -> &[u8] {
    // SAFETY: every constructor initialises the first `used` tags and they
    // stay initialised; `MaybeUninit<u8>` has the layout of `u8`.
    unsafe { slice::from_raw_parts(tags.slots().as_ptr().cast::<u8>(), used) }
}

/// The initialised tags of a map, mutably. Takes the buffer rather than
/// the map so the map's other fields stay usable.
#[inline]
fn live_mut<B: Buffer<u8>>(tags: &mut B, used: usize) -> &mut [u8] {
    // SAFETY: as in `live`, and the buffer is borrowed exclusively.
    unsafe { slice::from_raw_parts_mut(tags.slots_mut().as_mut_ptr().cast::<u8>(), used) }
}
