//! One big block of memory, handed out piece by piece.

#![allow(unsafe_code)]

use core::cell::{Cell, UnsafeCell};
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::slice;
#[cfg(target_has_atomic = "8")]
use core::sync::atomic::{AtomicBool, Ordering};

use crate::Full;

/// Hands out pieces of one block of memory, so that many containers and
/// values can be packed into it.
///
/// Give it a block of bytes; each [`alloc`](Self::alloc) returns the next
/// free, correctly aligned stretch as `&mut [MaybeUninit<T>]`, which is what
/// every vas container takes as its storage. Pieces never overlap.
///
/// ```
/// use vas::pool::Slot;
/// use vas::{Arena, Memory, Pool, Vector};
///
/// let mut block = Memory::<u8, 4096>::new();
/// let arena = Arena::new(&mut block);
///
/// let positions = arena.alloc::<[f32; 2]>(100).map(Vector::new);
/// let enemies = arena.alloc::<Slot<&str>>(16).map(Pool::new);
/// assert!(positions.is_ok() && enemies.is_ok());
/// assert!(arena.used() <= 4096);
/// ```
///
/// # Lifetimes
///
/// The pieces live as long as the **block**, not as long as the arena, so
/// the arena can be dropped once everything is set up.
///
/// - With a block that lives for the whole program (see [`StaticBlock`]),
///   the pieces are `'static`: containers built on them need no lifetime
///   parameter and can be stored in any struct.
/// - With a shorter-lived block, structs holding the containers carry that
///   lifetime.
///
/// # Freeing
///
/// Individual pieces are never freed, and values stored with
/// [`alloc_value`](Self::alloc_value) are never dropped. To start over,
/// create a new arena on the same block; Rust only allows that once
/// everything that used the old pieces is gone.
pub struct Arena<'a> {
    // Invariant: `start..start + capacity` is the block, exclusively
    // borrowed for `'a`, and `start..start + used` has been handed out.
    start: *mut u8,
    capacity: usize,
    used: Cell<usize>,
    marker: PhantomData<&'a mut [MaybeUninit<u8>]>,
}

impl<'a> Arena<'a> {
    /// Creates an arena that hands out pieces of `block`.
    #[inline]
    pub const fn new(block: &'a mut [MaybeUninit<u8>]) -> Self {
        Self {
            capacity: block.len(),
            start: block.as_mut_ptr().cast::<u8>(),
            used: Cell::new(0),
            marker: PhantomData,
        }
    }

    /// Size of the block in bytes.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Bytes handed out so far, including padding added for alignment.
    #[inline]
    pub fn used(&self) -> usize {
        self.used.get()
    }

    /// Bytes still available.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.capacity - self.used.get()
    }

    /// Hands out room for `count` values of `T`, aligned for `T`.
    ///
    /// The memory is uninitialised; pass it to a container's `new`. Fails
    /// without changing the arena when it does not fit.
    pub fn alloc<T>(&self, count: usize) -> Result<&'a mut [MaybeUninit<T>], Full<()>> {
        let used = self.used.get();
        // Padding to reach the next address that is a multiple of the
        // alignment (always a power of two).
        let address = self.start.addr().wrapping_add(used);
        let padding = address.wrapping_neg() & (align_of::<T>() - 1);
        let size = size_of::<T>().checked_mul(count).ok_or(Full(()))?;
        let begin = used.checked_add(padding).ok_or(Full(()))?;
        let end = begin.checked_add(size).ok_or(Full(()))?;
        if end > self.capacity {
            return Err(Full(()));
        }
        self.used.set(end);
        // SAFETY: `begin..end` lies inside the block, starts at an address
        // aligned for `T`, and has room for `count` values. `used` only ever
        // advances, so this range is handed out exactly once and overlaps no
        // other piece. The block is exclusively borrowed for `'a`, and
        // `MaybeUninit<T>` needs no initialisation.
        Ok(unsafe {
            slice::from_raw_parts_mut(self.start.add(begin).cast::<MaybeUninit<T>>(), count)
        })
    }

    /// Stores one value in the arena and returns a reference to it, or hands
    /// the value back in [`Full`] when it does not fit.
    ///
    /// The value is **never dropped**: the arena does not run destructors.
    pub fn alloc_value<T>(&self, value: T) -> Result<&'a mut T, Full<T>> {
        match self.alloc::<T>(1) {
            Ok([slot]) => Ok(slot.write(value)),
            _ => Err(Full(value)),
        }
    }

    /// Stores a copy of `values` in the arena and returns it.
    pub fn alloc_copy<T: Copy>(&self, values: &[T]) -> Result<&'a mut [T], Full<()>> {
        let slots = self.alloc::<T>(values.len())?;
        for (slot, value) in slots.iter_mut().zip(values) {
            slot.write(*value);
        }
        // SAFETY: every slot was just initialised, and `MaybeUninit<T>` has
        // the layout of `T`.
        Ok(unsafe { slice::from_raw_parts_mut(slots.as_mut_ptr().cast::<T>(), slots.len()) })
    }

    /// Splits off `bytes` of this arena as an arena of its own, for example
    /// to give one system a fixed budget.
    #[inline]
    pub fn sub(&self, bytes: usize) -> Result<Arena<'a>, Full<()>> {
        self.alloc::<u8>(bytes).map(Arena::new)
    }
}

/// A block of `N` bytes that lives for the whole program, to be declared as
/// a `static` and taken exactly once.
///
/// This is how to get `'static` memory without an allocator and without
/// `unsafe`. Containers built on it need no lifetime parameter.
///
/// ```
/// use vas::arena::StaticBlock;
/// use vas::{Arena, Vector};
///
/// static GAME_MEMORY: StaticBlock<{ 1 << 20 }> = StaticBlock::new(); // 1 MB
///
/// struct World {
///     positions: Vector<'static, [f32; 2]>, // no lifetime parameter needed
/// }
///
/// if let Some(block) = GAME_MEMORY.take() {
///     let arena = Arena::new(block);
///     let world = arena.alloc(1024).map(|memory| World { positions: Vector::new(memory) });
///     assert!(world.is_ok());
/// }
/// assert!(GAME_MEMORY.take().is_none()); // only once
/// ```
///
/// The block is not initialised, so it takes no space in the executable
/// file.
#[cfg(target_has_atomic = "8")]
pub struct StaticBlock<const N: usize> {
    taken: AtomicBool,
    bytes: UnsafeCell<[MaybeUninit<u8>; N]>,
}

// SAFETY: the bytes are only reachable through `take`, which hands out the
// one exclusive reference to them at most once, guarded by an atomic flag.
#[cfg(target_has_atomic = "8")]
unsafe impl<const N: usize> Sync for StaticBlock<N> {}

#[cfg(target_has_atomic = "8")]
impl<const N: usize> StaticBlock<N> {
    /// Creates the block. Use it to initialise a `static`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            taken: AtomicBool::new(false),
            bytes: UnsafeCell::new([MaybeUninit::uninit(); N]),
        }
    }

    /// Size of the block in bytes.
    #[inline]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Takes the block. Returns `None` on every call after the first, from
    /// any thread.
    #[inline]
    pub fn take(&'static self) -> Option<&'static mut [MaybeUninit<u8>]> {
        if self.taken.swap(true, Ordering::AcqRel) {
            return None;
        }
        // SAFETY: the flag was clear and is now set, so this is the only
        // time a reference to the bytes is ever created, and `self` lives
        // for `'static`.
        Some(unsafe { &mut *self.bytes.get() })
    }
}

#[cfg(target_has_atomic = "8")]
impl<const N: usize> Default for StaticBlock<N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
