use core::cell::Cell;
use core::mem::MaybeUninit;
use vas::storage::Storage;
use vas::{Arena, Borrowed, Full, Inline, Memory, Vector};

/// Counts drops, to check that elements are dropped exactly once.
struct Tracked<'a>(&'a Cell<u32>);

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

/// A struct of arrays that owns its memory: no lifetime, no arena.
struct Particles {
    x: Vector<f32, Inline<8>>,
    y: Vector<f32, Inline<8>>,
    life: Vector<u16, Inline<8>>,
}

/// Works with a vector over any storage.
fn fill<S: Storage<u32>>(out: &mut Vector<u32, S>) -> usize {
    let mut next = 0;
    while out.push(next).is_ok() {
        next += 1;
    }
    out.len()
}

/// Generic over the element type with borrowed storage; needs no bounds.
fn capacity_of<T>(vector: &Vector<T, Borrowed<'_>>) -> usize {
    vector.capacity()
}

static SHARED: Vector<u8, Inline<4>> = Vector::inline();

#[test]
fn inline_vector_owns_its_memory() {
    let mut v = Vector::<u32, Inline<4>>::inline();
    assert_eq!((v.len(), v.capacity()), (0, 4));
    for i in 0..4 {
        assert!(v.push(i).is_ok());
    }
    assert_eq!(v.push(4), Err(Full(4)));
    assert_eq!(v.as_slice(), &[0, 1, 2, 3]);
    assert_eq!(v.pop(), Some(3));
    assert_eq!(v.insert(0, 9).ok(), Some(()));
    assert_eq!(v.as_slice(), &[9, 0, 1, 2]);
}

#[test]
fn passing_memory_by_value_makes_an_inline_vector() {
    let mut v = Vector::new(Memory::<u8, 3>::new());
    assert!(v.extend_from_slice(&[1, 2, 3]).is_ok());
    assert!(v.is_full());

    let mut w = Vector::new([MaybeUninit::<u8>::uninit(); 2]);
    assert!(w.push(7).is_ok());
    assert_eq!(w.capacity(), 2);

    let d = Vector::<u64, Inline<16>>::default();
    assert_eq!((d.len(), d.capacity()), (0, 16));
}

#[test]
fn passing_memory_by_reference_borrows_it() {
    let mut memory = Memory::<u8, 3>::new();
    let mut array = [MaybeUninit::<u8>::uninit(); 5];
    let mut slice = [MaybeUninit::<u8>::uninit(); 7];
    let a: Vector<u8, Borrowed<'_>> = Vector::new(&mut memory);
    let b: Vector<u8, Borrowed<'_>> = Vector::new(&mut array);
    let c: Vector<u8, Borrowed<'_>> = Vector::new(&mut slice[..]);
    assert_eq!((a.capacity(), b.capacity(), c.capacity()), (3, 5, 7));
    assert_eq!(capacity_of(&a), 3);
}

#[test]
fn inline_vectors_live_in_structs_and_move_with_them() {
    let mut particles = Particles {
        x: Vector::inline(),
        y: Vector::new(Memory::new()),
        life: Vector::default(),
    };
    for i in 0..8u16 {
        assert!(particles.x.push(f32::from(i)).is_ok());
        assert!(particles.y.push(f32::from(i) * 2.0).is_ok());
        assert!(particles.life.push(i).is_ok());
    }
    // Move the whole struct; the contents move with it.
    let moved = core::hint::black_box(particles);
    assert_eq!(moved.x.as_slice()[7], 7.0);
    assert_eq!(moved.y.iter().sum::<f32>(), 56.0);
    assert_eq!(moved.life.as_slice(), &[0, 1, 2, 3, 4, 5, 6, 7]);
    assert!(moved.x.is_full());
}

#[test]
fn inline_vector_works_in_a_static() {
    assert_eq!((SHARED.len(), SHARED.capacity()), (0, 4));
}

#[test]
fn one_function_takes_any_storage() {
    let mut inline = Vector::<u32, Inline<5>>::inline();
    let mut memory = Memory::<u32, 9>::new();
    let mut borrowed = Vector::new(&mut memory);
    assert_eq!(fill(&mut inline), 5);
    assert_eq!(fill(&mut borrowed), 9);

    let mut block = Memory::<u8, 256>::new();
    let arena = Arena::new(&mut block);
    let from_arena = arena.alloc(12).map(Vector::new);
    assert_eq!(from_arena.map(|mut v| fill(&mut v)).ok(), Some(12));
}

#[test]
fn elements_are_dropped_once_in_inline_storage() {
    let drops = Cell::new(0);
    {
        let mut v = Vector::<Tracked<'_>, Inline<4>>::inline();
        for _ in 0..3 {
            assert!(v.push(Tracked(&drops)).is_ok());
        }
        drop(v.pop());
        assert_eq!(drops.get(), 1);
        let moved = core::hint::black_box(v);
        assert_eq!(drops.get(), 1);
        assert_eq!(moved.len(), 2);
    }
    assert_eq!(drops.get(), 3);
}

#[test]
fn move_into_changes_the_kind_of_storage() {
    let drops = Cell::new(0);
    {
        // Inline -> borrowed (bigger).
        let mut small = Vector::<Tracked<'_>, Inline<2>>::inline();
        assert!(small.push(Tracked(&drops)).is_ok());
        assert!(small.push(Tracked(&drops)).is_ok());
        let mut memory = Memory::<Tracked<'_>, 4>::new();
        let bigger = small.move_into(&mut memory);
        assert!(bigger.is_ok());
        if let Ok(mut bigger) = bigger {
            assert_eq!((bigger.len(), bigger.capacity()), (2, 4));
            assert!(bigger.push(Tracked(&drops)).is_ok());

            // Borrowed -> inline (owned).
            let owned = bigger.move_into(Memory::<Tracked<'_>, 8>::new());
            assert!(owned.is_ok());
            if let Ok(owned) = owned {
                assert_eq!((owned.len(), owned.capacity()), (3, 8));

                // Too small: handed back unchanged.
                let back = owned.move_into(Memory::<Tracked<'_>, 2>::new());
                assert!(back.is_err());
                if let Err(Full(owned)) = back {
                    assert_eq!((owned.len(), owned.capacity()), (3, 8));
                }
            }
        }
        assert_eq!(drops.get(), 3);
    }
    assert_eq!(drops.get(), 3);
}

#[test]
fn zero_capacity_and_zero_sized_inline() {
    let mut none = Vector::<u32, Inline<0>>::inline();
    assert_eq!(none.push(1), Err(Full(1)));
    let mut units = Vector::<(), Inline<3>>::inline();
    assert!(units.push(()).is_ok());
    assert_eq!(units.len(), 1);
}

#[test]
fn vectors_are_send_and_sync_when_their_elements_are() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Vector<u32, Inline<4>>>();
    assert_send_sync::<Vector<u32, Borrowed<'static>>>();
}

#[test]
fn inline_matches_std_vec() {
    let mut v = Vector::<u32, Inline<32>>::inline();
    let mut model: Vec<u32> = Vec::new();
    let mut state = 0x9E37_79B9u32;
    let rounds = if cfg!(miri) { 300 } else { 5000 };
    for _ in 0..rounds {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let value = state >> 8;
        match state >> 29 {
            0..=2 => {
                let fits = model.len() < 32;
                assert_eq!(v.push(value).is_ok(), fits);
                if fits {
                    model.push(value);
                }
            }
            3 => assert_eq!(v.pop(), model.pop()),
            4 => {
                let index = value as usize % 33;
                let fits = model.len() < 32 && index <= model.len();
                assert_eq!(v.insert(index, value).is_ok(), fits);
                if fits {
                    model.insert(index, value);
                }
            }
            5 => {
                let index = value as usize % 33;
                let expected = (index < model.len()).then(|| model.remove(index));
                assert_eq!(v.remove(index), expected);
            }
            6 => {
                let index = value as usize % 33;
                let expected = (index < model.len()).then(|| model.swap_remove(index));
                assert_eq!(v.swap_remove(index), expected);
            }
            _ => {
                let len = value as usize % 33;
                v.truncate(len);
                model.truncate(len);
            }
        }
        assert_eq!(v.as_slice(), model.as_slice());
    }
}
