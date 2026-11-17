//! Pool with generational handles over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::iter::Enumerate;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::{fmt, hash, slice};

use crate::Full;
use crate::allocator::Allocator;
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};

/// Marks the end of the free list. Never a valid slot index, because the
/// capacity is capped at `u32::MAX`.
const NONE: u32 = u32::MAX;

/// One slot of pool storage. Opaque; it only exists so the user can size the
/// memory for a [`Pool`]: `Memory::<Slot<T>, N>`.
pub struct Slot<T> {
    // Odd while the slot holds a value, even while it is free.
    generation: u32,
    // Next slot in the free list. Only meaningful while the slot is free.
    next_free: u32,
    // Initialised exactly while `generation` is odd.
    value: MaybeUninit<T>,
}

impl<T> Slot<T> {
    #[inline]
    const fn is_occupied(&self) -> bool {
        self.generation & 1 == 1
    }
}

/// A stable reference to a value in a [`Pool`].
///
/// A handle is two `u32`s: a slot index and a generation. Once the value is
/// removed the handle goes stale and every lookup with it returns `None`,
/// even after the slot has been reused.
///
/// A handle is only meaningful for the pool that issued it.
pub struct Handle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    #[inline]
    const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            marker: PhantomData,
        }
    }

    /// The index of the slot this handle points at: a small dense number
    /// below the pool's capacity, reused after the value is removed.
    ///
    /// Useful for keeping extra per-value data in a parallel array, or for
    /// algorithms that address nodes by index.
    #[inline]
    pub const fn index(&self) -> u32 {
        self.index
    }
}

impl<T> Clone for Handle<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> hash::Hash for Handle<T> {
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

/// A fixed-capacity pool of values addressed by generational [`Handle`]s.
///
/// Insert, lookup and remove are O(1). Values never move, and a handle to a
/// removed value is detected instead of silently pointing at whatever reused
/// the slot.
///
/// ```
/// use vas::{Inline, Pool};
///
/// let mut pool = Pool::<&str, Inline<64>>::inline();
///
/// if let Ok(player) = pool.insert("player") {
///     assert_eq!(pool.get(player), Some(&"player"));
///     assert_eq!(pool.remove(player), Some("player"));
///     assert_eq!(pool.get(player), None); // stale handle
/// }
/// ```
///
/// The storage parameter `S` decides where the slots live (see
/// [`storage`](crate::storage)): `Pool<T, Inline<N>>` holds them itself,
/// `Pool<T, Borrowed<'a>>` uses memory you pass to [`new`](Self::new), sized
/// in [`Slot<T>`]s.
///
/// With [`Dynamic`] storage the capacity is not fixed: the
/// memory comes from an allocator and the container grows when it runs out.
pub struct Pool<T, S: Storage<Slot<T>>> {
    // Invariants:
    // - the first `initialized` slots of the buffer are initialised `Slot`s.
    // - a slot's value is initialised exactly while its generation is odd.
    // - the free list (`free_head`, `next_free`) links free slots inside the
    //   initialised prefix.
    buffer: S::Buffer,
    initialized: u32,
    free_head: u32,
    len: u32,
}

impl<T, const N: usize> Pool<T, Inline<N>> {
    /// Creates an empty pool with room for `N` values inside itself.
    ///
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            buffer: [const { MaybeUninit::uninit() }; N],
            initialized: 0,
            free_head: NONE,
            len: 0,
        }
    }
}

impl<T, const N: usize> Default for Pool<T, Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Clones the values into a pool of the same capacity. Every value keeps
/// its slot and generation, so handles of the original refer to the same
/// values in the copy, and stale handles stay stale. Only pools with inline
/// storage can be cloned.
impl<T: Clone, const N: usize> Clone for Pool<T, Inline<N>> {
    fn clone(&self) -> Self {
        let mut copy = Self::inline();
        for (raw, slot) in copy.buffer.iter_mut().zip(self.slots()) {
            let value = if slot.is_occupied() {
                // SAFETY: an occupied slot holds an initialised value.
                MaybeUninit::new(unsafe { slot.value.assume_init_ref() }.clone())
            } else {
                MaybeUninit::uninit()
            };
            raw.write(Slot {
                generation: slot.generation,
                next_free: slot.next_free,
                value,
            });
            // Counted one by one, so a panicking `clone` leaks nothing.
            copy.initialized += 1;
            copy.len += u32::from(slot.is_occupied());
        }
        // The free list only links slots that were all copied above.
        copy.free_head = self.free_head;
        copy
    }
}

impl<T, A: Allocator + Default> Default for Pool<T, Dynamic<A>> {
    /// Creates an empty pool that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<T, S: Storage<Slot<T>>> Pool<T, S> {
    /// Creates an empty pool on `buffer`: a reference to memory borrows it,
    /// a `Memory` passed by value is stored inside the pool.
    ///
    /// Creation is O(1): slots are set up lazily on first use. At most
    /// `u32::MAX` slots are used.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<Slot<T>, Storage = S>,
    {
        Self {
            buffer: buffer.into_buffer(),
            initialized: 0,
            free_head: NONE,
            len: 0,
        }
    }

    /// Number of values in the pool.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    /// Number of values the pool can hold without growing. For borrowed
    /// and inline storage this is the maximum.
    #[inline]
    pub fn capacity(&self) -> usize {
        let slots = self.buffer.slots().len();
        if slots < NONE as usize {
            slots
        } else {
            NONE as usize
        }
    }

    /// `true` when the pool holds no values.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// `true` when the next insert needs more memory: it fails with
    /// borrowed and inline storage, and grows with dynamic storage.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.free_head == NONE && self.initialized as usize >= self.capacity()
    }

    /// Makes sure `additional` more values fit, growing dynamic storage
    /// when needed. Fails without changes when the storage is fixed and too
    /// small, or when the allocator refuses.
    #[inline]
    pub fn reserve(&mut self, additional: usize) -> Result<(), Full<()>> {
        match self.len().checked_add(additional) {
            Some(capacity) if capacity <= self.capacity() => Ok(()),
            Some(capacity) if capacity <= NONE as usize && self.buffer.grow(capacity) => Ok(()),
            _ => Err(Full(())),
        }
    }

    /// Stores `value` and returns its handle. With dynamic storage the pool
    /// grows when needed; handles stay valid.
    ///
    /// Hands the value back in [`Full`] when there is no room: the storage
    /// is fixed and full, or the allocator refused.
    #[inline]
    pub fn insert(&mut self, value: T) -> Result<Handle<T>, Full<T>> {
        let index = self.free_head;
        if index != NONE {
            // Reuse a free slot.
            let Some(slot) = self.slots_mut().get_mut(index as usize) else {
                return Err(Full(value));
            };
            slot.generation = slot.generation.wrapping_add(1);
            slot.value.write(value);
            let handle = Handle::new(index, slot.generation);
            self.free_head = slot.next_free;
            self.len += 1;
            return Ok(handle);
        }
        // Set up a fresh slot.
        let index = self.initialized;
        if index == NONE {
            return Err(Full(value));
        }
        if index as usize >= self.buffer.slots().len() {
            // No slot left: grow, or fail below when that is not possible.
            let _ = self.buffer.grow(index as usize + 1);
        }
        match self.buffer.slots_mut().get_mut(index as usize) {
            Some(raw) => {
                raw.write(Slot {
                    generation: 1,
                    next_free: NONE,
                    value: MaybeUninit::new(value),
                });
                self.initialized += 1;
                self.len += 1;
                Ok(Handle::new(index, 1))
            }
            None => Err(Full(value)),
        }
    }

    /// Removes and returns the value for `handle`, or `None` when the handle
    /// is stale.
    #[inline]
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let free_head = self.free_head;
        let slot = self.slot_mut(handle)?;
        // SAFETY: `slot_mut` only returns occupied slots, whose value is
        // initialised. The generation is bumped to even right after, so the
        // value is moved out exactly once.
        let value = unsafe { slot.value.assume_init_read() };
        slot.generation = slot.generation.wrapping_add(1);
        // A slot whose generation wrapped around is retired instead of
        // reused, so an old handle can never match a new value.
        if slot.generation != 0 {
            slot.next_free = free_head;
            self.free_head = handle.index;
        }
        self.len -= 1;
        Some(value)
    }

    /// The value for `handle`, or `None` when the handle is stale.
    #[inline]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        let slot = self.slots().get(handle.index as usize)?;
        if slot.generation != handle.generation {
            return None;
        }
        // SAFETY: handles are only issued with odd generations, so a match
        // means the slot is occupied and its value initialised.
        Some(unsafe { slot.value.assume_init_ref() })
    }

    /// The value for `handle`, mutably, or `None` when the handle is stale.
    #[inline]
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        let slot = self.slot_mut(handle)?;
        // SAFETY: `slot_mut` only returns occupied slots.
        Some(unsafe { slot.value.assume_init_mut() })
    }

    /// `true` when `handle` still refers to a value in the pool.
    #[inline]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Removes and drops all values. Every existing handle goes stale.
    pub fn clear(&mut self) {
        let mut free_head = self.free_head;
        for (index, slot) in self.slots_mut().iter_mut().enumerate() {
            if !slot.is_occupied() {
                continue;
            }
            // Mark the slot free before dropping, so a panicking `Drop`
            // cannot lead to a double drop.
            slot.generation = slot.generation.wrapping_add(1);
            if slot.generation != 0 {
                slot.next_free = free_head;
                // `index < initialized <= u32::MAX`, so this never truncates.
                free_head = index as u32;
            }
            // SAFETY: the slot was occupied, so its value is initialised,
            // and it is now marked free, so it is dropped exactly once.
            unsafe { slot.value.assume_init_drop() };
        }
        self.free_head = free_head;
        self.len = 0;
    }

    /// Iterates over the live values and their handles, in slot order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            slots: self.slots().iter().enumerate(),
        }
    }

    /// Iterates mutably over the live values and their handles, in slot
    /// order.
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            slots: self.slots_mut().iter_mut().enumerate(),
        }
    }

    /// The live value in slot `index`, whatever its generation. For
    /// structures built on the pool that keep their own links as indices.
    #[inline]
    pub(crate) fn get_at(&self, index: u32) -> Option<&T> {
        let slot = self.slots().get(index as usize)?;
        // SAFETY: an occupied slot holds an initialised value.
        slot.is_occupied()
            .then(|| unsafe { slot.value.assume_init_ref() })
    }

    /// Like [`get_at`](Self::get_at), mutably.
    #[inline]
    pub(crate) fn get_at_mut(&mut self, index: u32) -> Option<&mut T> {
        let slot = self.slots_mut().get_mut(index as usize)?;
        // SAFETY: an occupied slot holds an initialised value.
        slot.is_occupied()
            .then(|| unsafe { slot.value.assume_init_mut() })
    }

    /// The current handle of the live value in slot `index`.
    #[inline]
    pub(crate) fn handle_at(&self, index: u32) -> Option<Handle<T>> {
        let slot = self.slots().get(index as usize)?;
        slot.is_occupied()
            .then(|| Handle::new(index, slot.generation))
    }

    #[inline]
    fn slots(&self) -> &[Slot<T>] {
        // SAFETY: the first `initialized` slots are initialised by the
        // invariant, and `MaybeUninit<Slot<T>>` has the layout of `Slot<T>`.
        unsafe {
            slice::from_raw_parts(
                self.buffer.slots().as_ptr().cast::<Slot<T>>(),
                self.initialized as usize,
            )
        }
    }

    #[inline]
    fn slots_mut(&mut self) -> &mut [Slot<T>] {
        // SAFETY: as in `slots`, and `self` is borrowed exclusively.
        unsafe {
            slice::from_raw_parts_mut(
                self.buffer.slots_mut().as_mut_ptr().cast::<Slot<T>>(),
                self.initialized as usize,
            )
        }
    }

    /// The occupied slot `handle` refers to, if the handle is not stale.
    #[inline]
    fn slot_mut(&mut self, handle: Handle<T>) -> Option<&mut Slot<T>> {
        let slot = self.slots_mut().get_mut(handle.index as usize)?;
        // Handles are only issued with odd generations, so a match means
        // the slot is occupied.
        (slot.generation == handle.generation).then_some(slot)
    }
}

impl<T, S: Storage<Slot<T>>> Drop for Pool<T, S> {
    fn drop(&mut self) {
        for slot in self.slots_mut() {
            if slot.is_occupied() {
                // SAFETY: an occupied slot holds an initialised value, and
                // the pool is going away, so it is dropped exactly once.
                unsafe { slot.value.assume_init_drop() };
            }
        }
    }
}

impl<T: fmt::Debug, S: Storage<Slot<T>>> fmt::Debug for Pool<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<'p, T, S: Storage<Slot<T>>> IntoIterator for &'p Pool<T, S> {
    type Item = (Handle<T>, &'p T);
    type IntoIter = Iter<'p, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'p, T, S: Storage<Slot<T>>> IntoIterator for &'p mut Pool<T, S> {
    type Item = (Handle<T>, &'p mut T);
    type IntoIter = IterMut<'p, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

/// Iterator over the live values of a [`Pool`]. See [`Pool::iter`].
pub struct Iter<'p, T> {
    slots: Enumerate<slice::Iter<'p, Slot<T>>>,
}

impl<'p, T> Iterator for Iter<'p, T> {
    type Item = (Handle<T>, &'p T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for (index, slot) in self.slots.by_ref() {
            if slot.is_occupied() {
                // `index < u32::MAX`, so the cast never truncates.
                let handle = Handle::new(index as u32, slot.generation);
                // SAFETY: an occupied slot holds an initialised value.
                return Some((handle, unsafe { slot.value.assume_init_ref() }));
            }
        }
        None
    }
}

/// Mutable iterator over the live values of a [`Pool`]. See
/// [`Pool::iter_mut`].
pub struct IterMut<'p, T> {
    slots: Enumerate<slice::IterMut<'p, Slot<T>>>,
}

impl<'p, T> Iterator for IterMut<'p, T> {
    type Item = (Handle<T>, &'p mut T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        for (index, slot) in self.slots.by_ref() {
            if slot.is_occupied() {
                // `index < u32::MAX`, so the cast never truncates.
                let handle = Handle::new(index as u32, slot.generation);
                // SAFETY: an occupied slot holds an initialised value.
                return Some((handle, unsafe { slot.value.assume_init_mut() }));
            }
        }
        None
    }
}
