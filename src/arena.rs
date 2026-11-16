//! One big block of memory, handed out piece by piece.

#![allow(unsafe_code)]

use core::cell::{Cell, UnsafeCell};
use core::marker::PhantomData;
#[cfg(feature = "alloc")]
use core::mem::ManuallyDrop;
use core::mem::MaybeUninit;
#[cfg(feature = "alloc")]
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use core::slice;
#[cfg(target_has_atomic = "8")]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "alloc")]
use alloc::alloc::{alloc, dealloc};
use core::alloc::Layout;

use crate::Full;
use crate::allocator::Allocator;

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
///   parameter and can be stored in any struct. With the `alloc` feature,
///   [`Arena::from_heap`] gives the same from the heap in one call.
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
        let size = size_of::<T>().checked_mul(count).ok_or(Full(()))?;
        let begin = self.bump(size, align_of::<T>())?;
        // SAFETY: `bump` returned the start of `size` bytes inside the
        // block, aligned for `T`, that overlap no other piece. The block is
        // exclusively borrowed for `'a`, and `MaybeUninit<T>` needs no
        // initialisation.
        Ok(unsafe {
            slice::from_raw_parts_mut(self.start.add(begin).cast::<MaybeUninit<T>>(), count)
        })
    }

    /// Reserves the next `size` bytes aligned to `align` (a power of two)
    /// and returns their offset in the block. Fails without changing the
    /// arena when they do not fit.
    ///
    /// The returned range lies inside the block and overlaps no piece that
    /// is still in use.
    #[inline]
    fn bump(&self, size: usize, align: usize) -> Result<usize, Full<()>> {
        let used = self.used.get();
        // Padding to reach the next address that is a multiple of the
        // alignment.
        let address = self.start.addr().wrapping_add(used);
        let padding = address.wrapping_neg() & (align - 1);
        let begin = used.checked_add(padding).ok_or(Full(()))?;
        let end = begin.checked_add(size).ok_or(Full(()))?;
        if end > self.capacity {
            return Err(Full(()));
        }
        self.used.set(end);
        Ok(begin)
    }

    /// Offset in the block of `piece`, when its `size` bytes are the most
    /// recently handed out ones.
    #[inline]
    fn offset_of_last(&self, piece: NonNull<u8>, size: usize) -> Option<usize> {
        let offset = piece.addr().get().checked_sub(self.start.addr())?;
        (offset.checked_add(size)? == self.used.get()).then_some(offset)
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

/// An arena is an [`Allocator`]: containers with
/// [`Dynamic`](crate::Dynamic) storage can grow inside it.
///
/// Memory comes back only in reverse order: freeing or growing the most
/// recently handed out piece reuses its space, and a piece that is no
/// longer the last one is left behind when its container grows or is
/// dropped. This suits arenas that are thrown away as a whole, such as one
/// per frame or per level.
///
/// ```
/// use vas::{Arena, Dynamic, Memory, Vector};
///
/// let mut block = Memory::<u8, 4096>::new();
/// let arena = Arena::new(&mut block);
/// let mut visible = Vector::new(Dynamic(&arena));
/// for id in 0..500u32 {
///     assert!(visible.push(id).is_ok());
/// }
/// // Grown in place: no copies were left behind.
/// assert!(arena.used() < visible.capacity() * 4 + 4);
/// ```
// SAFETY: blocks are distinct ranges of the arena's block, handed out by
// `bump`, which never returns bytes that are still in use: `used` moves
// back only over the piece being freed. The memory belongs to the borrowed
// block, not to the arena value, so moving the arena does not move it.
unsafe impl Allocator for Arena<'_> {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Full<()>> {
        let begin = self.bump(layout.size(), layout.align())?;
        // SAFETY: `begin` is an offset inside the block, so the address is
        // in bounds of it and not null.
        Ok(unsafe { NonNull::new_unchecked(self.start.add(begin)) })
    }

    #[inline]
    unsafe fn free(&self, block: NonNull<u8>, layout: Layout) {
        // The last piece can be handed out again; the caller promises not
        // to use it any more.
        if let Some(offset) = self.offset_of_last(block, layout.size()) {
            self.used.set(offset);
        }
    }

    #[inline]
    unsafe fn grow(
        &self,
        block: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Result<NonNull<u8>, Full<()>> {
        // The last piece grows in place when the block has room.
        if let Some(offset) = self.offset_of_last(block, old.size())
            && let Some(end) = offset.checked_add(new.size())
            && end <= self.capacity
        {
            self.used.set(end);
            return Ok(block);
        }
        let grown = self.allocate(new)?;
        // SAFETY: the caller guarantees `block` is valid for `old.size()`
        // bytes; `grown` is a different piece of at least that size.
        unsafe { core::ptr::copy_nonoverlapping(block.as_ptr(), grown.as_ptr(), old.size()) };
        Ok(grown)
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
/// use vas::{Arena, Borrowed, MB, Vector};
///
/// static GAME_MEMORY: StaticBlock<{ MB(1) }> = StaticBlock::new();
///
/// struct World {
///     positions: Vector<[f32; 2], Borrowed<'static>>, // no lifetime parameter needed
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
    // Handing out `&mut` from `&self` is the point: the atomic flag makes
    // sure it happens at most once.
    #[allow(clippy::mut_from_ref)]
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

#[cfg(feature = "alloc")]
impl Arena<'static> {
    /// Takes `bytes` from the heap, once, for the rest of the program, and
    /// creates an arena on them. Requires the `alloc` feature.
    ///
    /// The pieces are `'static`, so containers built on them need no
    /// lifetime parameter. The block is **never returned** to the system;
    /// for memory to give back, use [`HeapBlock`].
    ///
    /// Fails when the system has no such block to give.
    ///
    /// ```
    /// use vas::{Arena, Borrowed, MB, Vector};
    ///
    /// struct Particles {
    ///     x: Vector<f32, Borrowed<'static>>,
    ///     y: Vector<f32, Borrowed<'static>>,
    /// }
    ///
    /// # fn main() -> Result<(), vas::Full<()>> {
    /// let arena = Arena::from_heap(MB(20))?;
    /// let particles = Particles {
    ///     x: Vector::new(arena.alloc(100_000)?),
    ///     y: Vector::new(arena.alloc(100_000)?),
    /// };
    /// assert_eq!(particles.x.capacity() + particles.y.capacity(), 200_000);
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_heap(bytes: usize) -> Result<Self, Full<()>> {
        // Never dropped, so the block stays valid for `'static`.
        let block = ManuallyDrop::new(HeapBlock::new(bytes)?);
        Ok(Self {
            start: block.start.as_ptr(),
            capacity: block.layout.size(),
            used: Cell::new(0),
            marker: PhantomData,
        })
    }
}

/// A block of bytes taken from the heap and returned to it when dropped.
/// Requires the `alloc` feature.
///
/// Use it for memory with a shorter life than the program, such as a level:
/// build an [`Arena`] on it, and when the level is over either drop the
/// block or build a new arena on it for the next one.
///
/// ```
/// use vas::arena::HeapBlock;
/// use vas::{Arena, MB, Vector};
///
/// # fn main() -> Result<(), vas::Full<()>> {
/// let mut block = HeapBlock::new(MB(1))?;
/// for level in 0..3 {
///     let arena = Arena::new(&mut block);
///     let mut enemies = Vector::new(arena.alloc::<u32>(1000)?);
///     assert!(enemies.push(level).is_ok());
/// }
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "alloc")]
pub struct HeapBlock {
    // Invariant: when `layout.size() > 0`, `start` was returned by the
    // global allocator for `layout` and is owned by this block. Otherwise
    // `start` is dangling and nothing was allocated.
    start: NonNull<u8>,
    layout: Layout,
}

/// Only its alignment is used: that of every [`HeapBlock`].
#[cfg(feature = "alloc")]
#[repr(align(16))]
struct Aligned(#[allow(dead_code)] [u8; 16]);

// SAFETY: the block owns its bytes exclusively and they are plain memory.
#[cfg(feature = "alloc")]
unsafe impl Send for HeapBlock {}
// SAFETY: shared access only reads the pointer and the size.
#[cfg(feature = "alloc")]
unsafe impl Sync for HeapBlock {}

#[cfg(feature = "alloc")]
impl HeapBlock {
    /// Alignment of the block itself. The arena aligns every piece on its
    /// own, so this only saves a little padding at the start.
    const ALIGN: usize = align_of::<Aligned>();

    /// Takes `bytes` from the heap. Fails when the system has no such block
    /// to give.
    pub fn new(bytes: usize) -> Result<Self, Full<()>> {
        let layout = Layout::from_size_align(bytes, Self::ALIGN).map_err(|_| Full(()))?;
        if bytes == 0 {
            return Ok(Self {
                // Aligned like a real block, so empty requests still work.
                start: NonNull::<Aligned>::dangling().cast(),
                layout,
            });
        }
        // SAFETY: the layout has a non-zero size.
        let start = NonNull::new(unsafe { alloc(layout) }).ok_or(Full(()))?;
        Ok(Self { start, layout })
    }

    /// Size of the block in bytes.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.layout.size()
    }
}

#[cfg(feature = "alloc")]
impl Deref for HeapBlock {
    type Target = [MaybeUninit<u8>];

    #[inline]
    fn deref(&self) -> &Self::Target {
        // SAFETY: `start` is valid for `layout.size()` bytes (or dangling
        // with size zero), and `MaybeUninit<u8>` needs no initialisation.
        unsafe { slice::from_raw_parts(self.start.as_ptr().cast(), self.layout.size()) }
    }
}

#[cfg(feature = "alloc")]
impl DerefMut for HeapBlock {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: as in `deref`, and `&mut self` makes the access exclusive.
        unsafe { slice::from_raw_parts_mut(self.start.as_ptr().cast(), self.layout.size()) }
    }
}

#[cfg(feature = "alloc")]
impl Drop for HeapBlock {
    fn drop(&mut self) {
        if self.layout.size() > 0 {
            // SAFETY: `start` came from `alloc` with this same layout and is
            // freed exactly once.
            unsafe { dealloc(self.start.as_ptr(), self.layout) };
        }
    }
}
