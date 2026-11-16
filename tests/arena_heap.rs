#![cfg(feature = "alloc")]

use vas::arena::HeapBlock;
use vas::{Arena, Borrowed, Full, Vector};

struct Particles {
    x: Vector<f32, Borrowed<'static>>,
    y: Vector<f32, Borrowed<'static>>,
}

#[test]
fn heap_block_has_the_requested_size() {
    let block = HeapBlock::new(4096);
    assert!(block.is_ok());
    if let Ok(mut block) = block {
        assert_eq!(block.capacity(), 4096);
        assert_eq!(block.len(), 4096);
        let arena = Arena::new(&mut block);
        assert_eq!(arena.capacity(), 4096);
    }
}

#[test]
fn heap_block_backs_containers() {
    let block = HeapBlock::new(1 << 16);
    assert!(block.is_ok());
    if let Ok(mut block) = block {
        let arena = Arena::new(&mut block);
        let numbers = arena.alloc::<u64>(1000).map(Vector::new);
        let bytes = arena.alloc::<u8>(3).map(Vector::new);
        let floats = arena.alloc::<f64>(1000).map(Vector::new);
        assert!(numbers.is_ok() && bytes.is_ok() && floats.is_ok());
        if let (Ok(mut numbers), Ok(mut floats)) = (numbers, floats) {
            for i in 0..1000u32 {
                assert!(numbers.push(u64::from(i)).is_ok());
                assert!(floats.push(f64::from(i)).is_ok());
            }
            assert_eq!(numbers.iter().sum::<u64>(), 499_500);
            assert_eq!(floats.last(), Some(&999.0));
        }
        assert_eq!(arena.alloc::<u8>(1 << 16).err(), Some(Full(())));
    }
}

#[test]
fn heap_block_can_be_reused() {
    let block = HeapBlock::new(64);
    assert!(block.is_ok());
    if let Ok(mut block) = block {
        for round in 0..3u8 {
            let arena = Arena::new(&mut block);
            let piece = arena.alloc_copy(&[round; 64]);
            assert_eq!(piece.map(|piece| piece[63]), Ok(round));
            assert_eq!(arena.remaining(), 0);
        }
    }
}

#[test]
fn empty_heap_block() {
    let block = HeapBlock::new(0);
    assert!(block.is_ok());
    if let Ok(mut block) = block {
        assert_eq!(block.capacity(), 0);
        let arena = Arena::new(&mut block);
        assert!(arena.alloc::<u64>(0).is_ok());
        assert_eq!(arena.alloc::<u8>(1).err(), Some(Full(())));
    }
}

#[test]
fn impossible_size_is_an_error() {
    assert!(HeapBlock::new(usize::MAX).is_err());
    assert!(Arena::from_heap(usize::MAX).is_err());
}

#[test]
fn heap_block_moves_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HeapBlock>();
}

// The block is never freed, on purpose; Miri reports that as a leak.
#[test]
#[cfg_attr(miri, ignore)]
fn from_heap_is_static() {
    // The arena goes out of scope here; its pieces stay valid.
    let particles = {
        let arena = Arena::from_heap(1 << 16);
        assert!(arena.is_ok());
        arena.map(|arena| {
            assert_eq!(arena.capacity(), 1 << 16);
            (arena.alloc(1000), arena.alloc(1000))
        })
    };
    if let Ok((Ok(x), Ok(y))) = particles {
        let mut particles = Particles {
            x: Vector::new(x),
            y: Vector::new(y),
        };
        assert!(particles.x.push(1.0).is_ok());
        assert!(particles.y.push(2.0).is_ok());
        assert_eq!(particles.x.capacity(), 1000);
        assert_eq!(particles.y.as_slice(), &[2.0]);
    } else {
        assert!(particles.is_ok_and(|(x, y)| x.is_ok() && y.is_ok()));
    }
}

#[test]
fn vec_and_box_of_slots_can_be_borrowed_directly() {
    use core::mem::MaybeUninit;
    let mut memory: Vec<MaybeUninit<u32>> = Vec::new();
    memory.resize_with(8, MaybeUninit::uninit);
    let mut v = Vector::new(&mut memory);
    assert!(v.push(1).is_ok());
    assert_eq!(v.capacity(), 8);

    let mut boxed: Box<[MaybeUninit<u32>]> = Box::new_uninit_slice(4);
    let w = Vector::new(&mut boxed);
    assert_eq!(w.capacity(), 4);
}
