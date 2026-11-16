#![allow(unsafe_code)]

use core::alloc::Layout;
use core::cell::Cell;
use core::ptr::NonNull;
use vas::allocator::Allocator;
use vas::storage::Storage;
use vas::{Arena, Dynamic, Full, InsertError, Inline, Memory, Vector};

/// Counts drops, to check that elements are dropped exactly once.
struct Tracked<'a>(&'a Cell<u32>);

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

/// An allocator written by the library user: forwards to an arena and
/// counts the calls.
struct Counting<'a> {
    arena: Arena<'a>,
    allocations: Cell<u32>,
    frees: Cell<u32>,
}

// SAFETY: every call is forwarded unchanged to the arena, which upholds the
// contract.
unsafe impl Allocator for Counting<'_> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<u8>, Full<()>> {
        self.allocations.set(self.allocations.get() + 1);
        self.arena.allocate(layout)
    }

    unsafe fn free(&self, block: NonNull<u8>, layout: Layout) {
        self.frees.set(self.frees.get() + 1);
        // SAFETY: the caller passes a block from `allocate` with its layout.
        unsafe { self.arena.free(block, layout) }
    }
}

/// Works with a vector over any storage.
fn fill<S: Storage<u32>>(out: &mut Vector<u32, S>, count: u32) -> bool {
    (0..count).all(|i| out.push(i).is_ok())
}

#[test]
fn starts_empty_and_grows_on_push() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut v = Vector::new(Dynamic(&arena));
    assert_eq!((v.len(), v.capacity()), (0, 0));
    assert_eq!(arena.used(), 0);
    for i in 0..100u32 {
        assert!(v.push(i).is_ok());
    }
    assert_eq!(v.len(), 100);
    assert!(v.capacity() >= 100);
    assert!(v.iter().copied().eq(0..100));
}

#[test]
fn push_fails_only_when_the_allocator_refuses() {
    let mut block = Memory::<u8, 64>::new();
    let arena = Arena::new(&mut block);
    let mut v = Vector::new(Dynamic(&arena));
    let mut pushed = 0u64;
    let rejected = loop {
        match v.push(pushed) {
            Ok(()) => pushed += 1,
            Err(Full(value)) => break value,
        }
    };
    assert_eq!(rejected, pushed);
    assert!(pushed >= 4 && pushed <= 8);
    assert!(v.iter().copied().eq(0..pushed));
    assert!(v.is_full());
}

#[test]
fn memory_is_returned_on_drop() {
    let mut block = Memory::<u8, 1024>::new();
    let arena = Arena::new(&mut block);
    {
        let mut v = Vector::new(Dynamic(&arena));
        assert!(fill(&mut v, 50));
        assert!(arena.used() >= 200);
    }
    assert_eq!(arena.used(), 0);
}

#[test]
fn last_piece_of_an_arena_grows_in_place() {
    let mut block = Memory::<u8, 1024>::new();
    let arena = Arena::new(&mut block);
    let mut v = Vector::new(Dynamic(&arena));
    assert!(fill(&mut v, 100));
    // Nothing was copied to a new place, so only the live block is in use.
    assert_eq!(arena.used(), v.capacity() * size_of::<u32>());
}

#[test]
fn several_vectors_share_one_allocator() {
    let mut block = Memory::<u8, 8192>::new();
    let arena = Arena::new(&mut block);
    let mut a = Vector::new(Dynamic(&arena));
    let mut b = Vector::new(Dynamic(&arena));
    for i in 0..200u32 {
        assert!(a.push(i).is_ok());
        assert!(b.push(u64::from(i) * 3).is_ok());
    }
    assert!(a.iter().copied().eq(0..200));
    assert!(b.iter().copied().eq((0..200).map(|i| i * 3)));
}

#[test]
fn reserve_makes_room_up_front() {
    let mut block = Memory::<u8, 1024>::new();
    let arena = Arena::new(&mut block);
    let mut v: Vector<u16, Dynamic<&Arena<'_>>> = Vector::new(Dynamic(&arena));
    assert_eq!(v.reserve(100), Ok(()));
    let capacity = v.capacity();
    assert!(capacity >= 100);
    assert!((0..100).all(|i| v.push(i).is_ok()));
    assert_eq!(v.capacity(), capacity);
    assert_eq!(v.reserve(usize::MAX), Err(Full(())));
    assert_eq!(v.reserve(100_000), Err(Full(())));
    assert_eq!(v.len(), 100);
}

#[test]
fn reserve_on_fixed_storage_only_checks() {
    let mut v = Vector::<u8, Inline<4>>::inline();
    assert_eq!(v.reserve(4), Ok(()));
    assert_eq!(v.reserve(5), Err(Full(())));
    assert!(v.push(1).is_ok());
    assert_eq!(v.reserve(3), Ok(()));
    assert_eq!(v.reserve(4), Err(Full(())));
    assert_eq!(v.capacity(), 4);
}

#[test]
fn bulk_operations_grow() {
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    let mut v = Vector::new(Dynamic(&arena));
    assert_eq!(v.extend_from_slice(&[1u32, 2, 3]), Ok(()));
    assert_eq!(v.insert(0, 0).ok(), Some(()));
    assert_eq!(v.resize(10, 7), Ok(()));
    assert_eq!(v.as_slice(), &[0, 1, 2, 3, 7, 7, 7, 7, 7, 7]);
    assert_eq!(v.resize_with(12, || 9), Ok(()));
    assert_eq!(v.len(), 12);
    assert!(matches!(v.insert(99, 1), Err(InsertError::OutOfBounds(1))));
    assert_eq!(v.extend_from_slice(&[0; 5000]), Err(Full(())));
    assert_eq!(v.len(), 12);
}

#[test]
fn elements_are_dropped_exactly_once() {
    let drops = Cell::new(0);
    let mut block = Memory::<u8, 4096>::new();
    let arena = Arena::new(&mut block);
    {
        let mut v = Vector::new(Dynamic(&arena));
        let mut other = Vector::new(Dynamic(&arena));
        for i in 0..40u8 {
            assert!(v.push(Tracked(&drops)).is_ok());
            // Forces `v` to move when it grows next.
            assert!(other.push(i).is_ok());
        }
        assert_eq!(drops.get(), 0);
        drop(v.pop());
        assert_eq!(drops.get(), 1);
    }
    assert_eq!(drops.get(), 40);
}

#[test]
fn move_into_changes_the_storage_kind() {
    let mut block = Memory::<u8, 1024>::new();
    let arena = Arena::new(&mut block);
    let mut fixed = Vector::<u32, Inline<4>>::inline();
    assert!(fixed.extend_from_slice(&[1, 2, 3, 4]).is_ok());
    assert_eq!(fixed.push(5), Err(Full(5)));

    let grown = fixed.move_into(Dynamic(&arena));
    assert!(grown.is_ok());
    if let Ok(mut grown) = grown {
        assert!(grown.push(5).is_ok());
        assert_eq!(grown.as_slice(), &[1, 2, 3, 4, 5]);

        let back = grown.move_into(Memory::<u32, 8>::new());
        assert_eq!(back.ok().map(|v| v.len()), Some(5));
    }
    assert_eq!(arena.used(), 0);
}

#[test]
fn move_into_fails_when_the_allocator_refuses() {
    let mut block = Memory::<u8, 8>::new();
    let arena = Arena::new(&mut block);
    let mut fixed = Vector::<u64, Inline<4>>::inline();
    assert!(fixed.extend_from_slice(&[1, 2, 3, 4]).is_ok());
    let result = fixed.move_into(Dynamic(&arena));
    assert_eq!(result.err().map(|Full(v)| v.len()), Some(4));
}

#[test]
fn zero_sized_elements_need_no_memory() {
    let mut block = Memory::<u8, 0>::new();
    let arena = Arena::new(&mut block);
    let mut v = Vector::new(Dynamic(&arena));
    for _ in 0..1000 {
        assert!(v.push(()).is_ok());
    }
    assert_eq!(v.len(), 1000);
    assert_eq!(arena.used(), 0);
}

#[test]
fn user_written_allocator() {
    let mut block = Memory::<u8, 4096>::new();
    let allocator = Counting {
        arena: Arena::new(&mut block),
        allocations: Cell::new(0),
        frees: Cell::new(0),
    };
    {
        let mut a = Vector::new(Dynamic(&allocator));
        let mut b = Vector::new(Dynamic(&allocator));
        for i in 0..100u32 {
            assert!(a.push(i).is_ok());
            assert!(b.push(i as u8).is_ok());
        }
        assert!(a.iter().copied().eq(0..100));
        assert_eq!(b.last(), Some(&99));
    }
    // The default `grow` allocates a new block and frees the old one.
    assert!(allocator.allocations.get() > 2);
    assert_eq!(allocator.allocations.get(), allocator.frees.get());
}

#[test]
fn allocator_can_be_owned_by_the_vector() {
    let mut block = Memory::<u8, 256>::new();
    let mut v = Vector::new(Dynamic(Arena::new(&mut block)));
    assert!(fill(&mut v, 20));
    assert_eq!(v.len(), 20);
}

#[cfg(feature = "alloc")]
mod global {
    use vas::{Dynamic, Global, Vector};

    #[derive(Default)]
    struct Particles {
        x: Vector<f32, Dynamic>,
        y: Vector<f32, Dynamic>,
        names: Vector<String, Dynamic>,
    }

    #[test]
    fn struct_fields_need_no_size_and_no_lifetime() {
        let mut particles = Particles::default();
        for i in 0..1000u16 {
            assert!(particles.x.push(f32::from(i)).is_ok());
            assert!(particles.y.push(-f32::from(i)).is_ok());
            assert!(particles.names.push(format!("p{i}")).is_ok());
        }
        assert_eq!(particles.x.len(), 1000);
        assert_eq!(particles.y.last(), Some(&-999.0));
        assert_eq!(particles.names.get(7).map(String::as_str), Some("p7"));
    }

    #[test]
    fn global_is_the_default_allocator() {
        let mut v: Vector<u64, Dynamic> = Vector::new(Dynamic(Global));
        assert_eq!(v.reserve(1 << 16), Ok(()));
        assert!(v.capacity() >= 1 << 16);
        assert!(v.extend_from_slice(&[1, 2, 3]).is_ok());
        assert_eq!(v.iter().sum::<u64>(), 6);
    }

    #[test]
    fn impossible_sizes_are_errors() {
        let mut v = Vector::<u64, Dynamic>::default();
        assert!(v.reserve(usize::MAX / 4).is_err());
        assert!(v.push(1).is_ok());
    }

    #[test]
    fn vector_moves_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Vector<u32, Dynamic>>();
    }
}
