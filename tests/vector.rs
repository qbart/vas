use core::cell::Cell;
use core::mem::MaybeUninit;
use vas::{Full, InsertError, Memory, Vector};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn new_is_empty() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let v = Vector::new(&mut memory);
    assert_eq!(v.len(), 0);
    assert_eq!(v.capacity(), 4);
    assert_eq!(v.remaining(), 4);
    assert!(v.is_empty());
    assert!(!v.is_full());
    assert_eq!(v.as_slice(), &[] as &[u32]);
}

#[test]
fn push_and_pop() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 2];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.push(1), Ok(()));
    assert_eq!(v.push(2), Ok(()));
    assert!(v.is_full());
    assert_eq!(v.as_slice(), &[1, 2]);
    assert_eq!(v.pop(), Some(2));
    assert_eq!(v.pop(), Some(1));
    assert_eq!(v.pop(), None);
    assert!(v.is_empty());
}

#[test]
fn push_when_full_returns_value() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 1];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.push(1), Ok(()));
    assert_eq!(v.push(2), Err(Full(2)));
    assert_eq!(v.as_slice(), &[1]);
}

#[test]
fn zero_capacity() {
    let mut memory: [MaybeUninit<u32>; 0] = [];
    let mut v = Vector::new(&mut memory);
    assert!(v.is_empty());
    assert!(v.is_full());
    assert_eq!(v.push(1), Err(Full(1)));
    assert_eq!(v.pop(), None);
}

#[test]
fn zero_sized_type() {
    let mut memory = [MaybeUninit::<()>::uninit(); 3];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.push(()), Ok(()));
    assert_eq!(v.push(()), Ok(()));
    assert_eq!(v.len(), 2);
    assert_eq!(v.pop(), Some(()));
    assert_eq!(v.swap_remove(0), Some(()));
    assert!(v.is_empty());
}

#[test]
fn extend_from_slice_is_all_or_nothing() {
    let mut memory = [MaybeUninit::<u8>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.extend_from_slice(&[1, 2, 3]), Ok(()));
    assert_eq!(v.extend_from_slice(&[4, 5]), Err(Full(())));
    assert_eq!(v.as_slice(), &[1, 2, 3]);
    assert_eq!(v.extend_from_slice(&[]), Ok(()));
    assert_eq!(v.extend_from_slice(&[4]), Ok(()));
    assert_eq!(v.as_slice(), &[1, 2, 3, 4]);
}

#[test]
fn swap_remove() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.extend_from_slice(&[10, 20, 30, 40]), Ok(()));
    assert_eq!(v.swap_remove(4), None);
    assert_eq!(v.swap_remove(0), Some(10));
    assert_eq!(v.as_slice(), &[40, 20, 30]);
    assert_eq!(v.swap_remove(2), Some(30));
    assert_eq!(v.as_slice(), &[40, 20]);
}

#[test]
fn truncate_and_clear() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.extend_from_slice(&[1, 2, 3, 4]), Ok(()));
    v.truncate(10);
    assert_eq!(v.len(), 4);
    v.truncate(2);
    assert_eq!(v.as_slice(), &[1, 2]);
    v.clear();
    assert!(v.is_empty());
}

#[test]
fn slice_access_and_mutation() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.extend_from_slice(&[3, 1, 2]), Ok(()));
    v.as_mut_slice().sort_unstable();
    assert_eq!(v.as_slice(), &[1, 2, 3]);
    // Deref gives slice methods directly.
    assert_eq!(v.iter().sum::<u32>(), 6);
    assert_eq!(v.first(), Some(&1));
    if let Some(x) = v.get_mut(0) {
        *x = 9;
    }
    assert_eq!(&*v, &[9, 2, 3]);
}

#[test]
fn contiguous_block_for_renderer() {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Vertex {
        position: [f32; 2],
    }
    let mut memory = [MaybeUninit::<Vertex>::uninit(); 3];
    let base = memory.as_ptr() as usize;
    let mut v = Vector::new(&mut memory);
    assert!(
        v.push(Vertex {
            position: [0.0, 1.0]
        })
        .is_ok()
    );
    assert!(
        v.push(Vertex {
            position: [2.0, 3.0]
        })
        .is_ok()
    );
    // The contents live in the user's memory, with no header or copy.
    assert_eq!(v.as_slice().as_ptr() as usize, base);
    assert_eq!(size_of_val(v.as_slice()), 2 * size_of::<Vertex>());
}

#[test]
fn drops_elements() {
    let drops = Cell::new(0);
    let mut memory = [const { MaybeUninit::<Counted>::uninit() }; 5];
    {
        let mut v = Vector::new(&mut memory);
        for _ in 0..5 {
            assert!(v.push(Counted(&drops)).is_ok());
        }
        drop(v.pop());
        assert_eq!(drops.get(), 1);
        drop(v.swap_remove(0));
        assert_eq!(drops.get(), 2);
        v.truncate(2);
        assert_eq!(drops.get(), 3);
        // Rejected value is handed back, not dropped by the vector.
    }
    assert_eq!(drops.get(), 5);
}

#[test]
fn storage_is_reusable_after_drop() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 2];
    {
        let mut v = Vector::new(&mut memory);
        assert_eq!(v.push(1), Ok(()));
    }
    let mut v = Vector::new(&mut memory);
    assert!(v.is_empty());
    assert_eq!(v.push(7), Ok(()));
    assert_eq!(v.as_slice(), &[7]);
}

#[test]
fn full_error_is_displayable() {
    extern crate std;
    use std::string::ToString;
    assert_eq!(Full(1).to_string(), "container is full");
}

#[test]
fn resize_grows_shrinks_and_rejects() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.resize(3, 7), Ok(()));
    assert_eq!(v.as_slice(), &[7, 7, 7]);
    assert_eq!(v.resize(1, 0), Ok(()));
    assert_eq!(v.as_slice(), &[7]);
    assert_eq!(v.resize(5, 0), Err(Full(())));
    assert_eq!(v.as_slice(), &[7]);
    assert_eq!(v.resize(4, 9), Ok(()));
    assert_eq!(v.as_slice(), &[7, 9, 9, 9]);
}

#[test]
fn resize_with_calls_closure_per_new_element() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    let mut next = 0;
    assert_eq!(
        v.resize_with(3, || {
            next += 1;
            next
        }),
        Ok(())
    );
    assert_eq!(v.as_slice(), &[1, 2, 3]);
    assert_eq!(v.resize_with(5, || 0), Err(Full(())));
    assert_eq!(v.resize_with(0, || 0), Ok(()));
    assert!(v.is_empty());
}

#[test]
fn resize_drops_removed_elements() {
    let drops = Cell::new(0);
    let mut memory = [const { MaybeUninit::<Counted>::uninit() }; 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.resize_with(4, || Counted(&drops)), Ok(()));
    assert_eq!(v.resize_with(1, || Counted(&drops)), Ok(()));
    assert_eq!(drops.get(), 3);
}

#[test]
fn module_path() {
    let mut memory = [MaybeUninit::<u8>::uninit(); 1];
    let v = vas::vector::Vector::new(&mut memory);
    assert!(v.is_empty());
}

#[test]
fn insert_keeps_order() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.insert(0, 3), Ok(()));
    assert_eq!(v.insert(0, 1), Ok(()));
    assert_eq!(v.insert(1, 2), Ok(()));
    assert_eq!(v.insert(3, 4), Ok(()));
    assert_eq!(v.as_slice(), &[1, 2, 3, 4]);
}

#[test]
fn insert_rejects_and_returns_value() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 2];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.insert(1, 9), Err(InsertError::OutOfBounds(9)));
    assert_eq!(v.insert(0, 1), Ok(()));
    assert_eq!(v.insert(1, 2), Ok(()));
    assert_eq!(v.insert(0, 9), Err(InsertError::Full(9)));
    assert_eq!(v.as_slice(), &[1, 2]);
}

#[test]
fn remove_keeps_order() {
    let mut memory = [MaybeUninit::<u32>::uninit(); 4];
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.extend_from_slice(&[1, 2, 3, 4]), Ok(()));
    assert_eq!(v.remove(4), None);
    assert_eq!(v.remove(1), Some(2));
    assert_eq!(v.as_slice(), &[1, 3, 4]);
    assert_eq!(v.remove(2), Some(4));
    assert_eq!(v.remove(0), Some(1));
    assert_eq!(v.as_slice(), &[3]);
}

#[test]
fn insert_and_remove_drop_correctly() {
    let drops = Cell::new(0);
    let mut memory = [const { MaybeUninit::<Counted>::uninit() }; 3];
    {
        let mut v = Vector::new(&mut memory);
        assert!(v.push(Counted(&drops)).is_ok());
        assert!(v.push(Counted(&drops)).is_ok());
        assert!(v.insert(1, Counted(&drops)).is_ok());
        drop(v.remove(0));
        assert_eq!(drops.get(), 1);
        // A rejected value comes back to the caller and is dropped there.
        drop(v.insert(9, Counted(&drops)));
        assert_eq!(drops.get(), 2);
    }
    assert_eq!(drops.get(), 4);
}

#[test]
fn move_into_bigger_block() {
    let mut small = Memory::<u32, 2>::new();
    let mut big = Memory::<u32, 4>::new();
    {
        let mut v = Vector::new(&mut small);
        assert_eq!(v.extend_from_slice(&[1, 2]), Ok(()));
        let moved = v.move_into(&mut big);
        assert!(moved.is_ok());
        if let Ok(mut v) = moved {
            assert_eq!(v.capacity(), 4);
            assert_eq!(v.as_slice(), &[1, 2]);
            assert_eq!(v.push(3), Ok(()));
        }
    }
    // The old block is free to reuse.
    let v = Vector::new(&mut small);
    assert!(v.is_empty());
}

#[test]
fn move_into_too_small_block_returns_vector() {
    let mut memory = Memory::<u32, 3>::new();
    let mut tiny = Memory::<u32, 2>::new();
    let mut v = Vector::new(&mut memory);
    assert_eq!(v.extend_from_slice(&[1, 2, 3]), Ok(()));
    let moved = v.move_into(&mut tiny);
    assert!(moved.is_err());
    if let Err(Full(v)) = moved {
        assert_eq!(v.as_slice(), &[1, 2, 3]);
    }
}

#[test]
fn move_into_moves_without_dropping() {
    let drops = Cell::new(0);
    let mut a = Memory::<Counted, 2>::new();
    let mut b = Memory::<Counted, 2>::new();
    {
        let mut v = Vector::new(&mut a);
        assert!(v.push(Counted(&drops)).is_ok());
        assert!(v.push(Counted(&drops)).is_ok());
        let moved = v.move_into(&mut b);
        assert_eq!(drops.get(), 0);
        assert!(moved.is_ok());
    }
    assert_eq!(drops.get(), 2);
}
