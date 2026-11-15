use core::cell::Cell;
use std::collections::VecDeque;
use vas::{Deque, Full, Memory};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<u32, 4>::new();
    let d = Deque::new(&mut memory);
    assert_eq!(d.len(), 0);
    assert_eq!(d.capacity(), 4);
    assert_eq!(d.remaining(), 4);
    assert!(d.is_empty());
    assert!(!d.is_full());
    assert_eq!(d.front(), None);
    assert_eq!(d.back(), None);
    assert_eq!(d.iter().count(), 0);
}

#[test]
fn first_in_first_out() {
    let mut memory = Memory::<u32, 3>::new();
    let mut d = Deque::new(&mut memory);
    assert_eq!(d.push_back(1), Ok(()));
    assert_eq!(d.push_back(2), Ok(()));
    assert_eq!(d.push_back(3), Ok(()));
    assert!(d.is_full());
    assert_eq!(d.push_back(4), Err(Full(4)));
    assert_eq!(d.push_front(4), Err(Full(4)));
    assert_eq!(d.pop_front(), Some(1));
    assert_eq!(d.pop_front(), Some(2));
    assert_eq!(d.pop_front(), Some(3));
    assert_eq!(d.pop_front(), None);
    assert_eq!(d.pop_back(), None);
}

#[test]
fn wraps_around_the_block() {
    let mut memory = Memory::<u32, 3>::new();
    let mut d = Deque::new(&mut memory);
    for round in 0..10 {
        assert_eq!(d.push_back(round), Ok(()));
        assert_eq!(d.push_back(round + 100), Ok(()));
        assert_eq!(d.pop_front(), Some(round));
        assert_eq!(d.pop_front(), Some(round + 100));
    }
    assert!(d.is_empty());
}

#[test]
fn both_ends() {
    let mut memory = Memory::<u32, 4>::new();
    let mut d = Deque::new(&mut memory);
    assert_eq!(d.push_back(2), Ok(()));
    assert_eq!(d.push_front(1), Ok(()));
    assert_eq!(d.push_back(3), Ok(()));
    assert_eq!(d.push_front(0), Ok(()));
    assert_eq!(d.front(), Some(&0));
    assert_eq!(d.back(), Some(&3));
    assert_eq!(d.get(0), Some(&0));
    assert_eq!(d.get(2), Some(&2));
    assert_eq!(d.get(4), None);
    assert!(d.iter().copied().eq(0..4));
    assert!(d.iter().rev().copied().eq((0..4).rev()));
    if let Some(x) = d.front_mut() {
        *x = 10;
    }
    if let Some(x) = d.back_mut() {
        *x = 13;
    }
    if let Some(x) = d.get_mut(1) {
        *x = 11;
    }
    assert_eq!(d.pop_back(), Some(13));
    assert_eq!(d.pop_front(), Some(10));
    assert_eq!(d.pop_front(), Some(11));
    assert_eq!(d.pop_back(), Some(2));
    assert!(d.is_empty());
}

#[test]
fn slices_cover_contents_in_order() {
    let mut memory = Memory::<u32, 4>::new();
    let mut d = Deque::new(&mut memory);
    for i in 0..4 {
        assert_eq!(d.push_back(i), Ok(()));
    }
    assert_eq!(d.as_slices(), (&[0, 1, 2, 3][..], &[][..]));
    assert_eq!(d.pop_front(), Some(0));
    assert_eq!(d.pop_front(), Some(1));
    assert_eq!(d.push_back(4), Ok(()));
    // Wrapped: two pieces.
    assert_eq!(d.as_slices(), (&[2, 3][..], &[4][..]));
    let (first, second) = d.as_mut_slices();
    for x in first.iter_mut().chain(second.iter_mut()) {
        *x *= 2;
    }
    assert!(d.iter().copied().eq([4, 6, 8]));
    for x in d.iter_mut() {
        *x += 1;
    }
    assert!(d.iter().copied().eq([5, 7, 9]));
}

#[test]
fn push_overwrite_evicts_the_oldest() {
    let mut memory = Memory::<u32, 3>::new();
    let mut d = Deque::new(&mut memory);
    assert_eq!(d.push_overwrite(1), None);
    assert_eq!(d.push_overwrite(2), None);
    assert_eq!(d.push_overwrite(3), None);
    assert_eq!(d.push_overwrite(4), Some(1));
    assert_eq!(d.push_overwrite(5), Some(2));
    assert!(d.iter().copied().eq([3, 4, 5]));
    assert_eq!(d.len(), 3);
}

#[test]
fn zero_capacity() {
    let mut memory = Memory::<u32, 0>::new();
    let mut d = Deque::new(&mut memory);
    assert!(d.is_full());
    assert_eq!(d.push_back(1), Err(Full(1)));
    assert_eq!(d.push_front(1), Err(Full(1)));
    assert_eq!(d.push_overwrite(1), Some(1));
    assert_eq!(d.pop_front(), None);
    assert_eq!(d.as_slices(), (&[][..], &[][..]));
}

#[test]
fn zero_sized_type() {
    let mut memory = Memory::<(), 2>::new();
    let mut d = Deque::new(&mut memory);
    assert_eq!(d.push_back(()), Ok(()));
    assert_eq!(d.push_front(()), Ok(()));
    assert_eq!(d.push_back(()), Err(Full(())));
    assert_eq!(d.pop_front(), Some(()));
    assert_eq!(d.pop_back(), Some(()));
    assert_eq!(d.pop_back(), None);
}

#[test]
fn drops_elements() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Counted, 3>::new();
    {
        let mut d = Deque::new(&mut memory);
        for _ in 0..3 {
            assert!(d.push_back(Counted(&drops)).is_ok());
        }
        drop(d.pop_front());
        assert_eq!(drops.get(), 1);
        // Wrap, so the contents are in two pieces when dropped.
        assert!(d.push_back(Counted(&drops)).is_ok());
        drop(d.push_overwrite(Counted(&drops)));
        assert_eq!(drops.get(), 2);
    }
    assert_eq!(drops.get(), 5);
}

#[test]
fn clear_drops_and_resets() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Counted, 3>::new();
    let mut d = Deque::new(&mut memory);
    assert!(d.push_back(Counted(&drops)).is_ok());
    assert!(d.push_front(Counted(&drops)).is_ok());
    d.clear();
    assert_eq!(drops.get(), 2);
    assert!(d.is_empty());
    for _ in 0..3 {
        assert!(d.push_back(Counted(&drops)).is_ok());
    }
    assert!(d.is_full());
}

#[test]
fn matches_std_vecdeque() {
    let mut memory = Memory::<u32, 7>::new();
    let mut d = Deque::new(&mut memory);
    let mut model: VecDeque<u32> = VecDeque::new();
    let mut seed: u32 = 0x9e37_79b9;
    let steps: u32 = if cfg!(miri) { 500 } else { 5000 };
    for step in 0..steps {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        match (seed >> 16) & 3 {
            0 => {
                if model.len() == 7 {
                    assert_eq!(d.push_back(step), Err(Full(step)));
                } else {
                    model.push_back(step);
                    assert_eq!(d.push_back(step), Ok(()));
                }
            }
            1 => {
                if model.len() == 7 {
                    assert_eq!(d.push_front(step), Err(Full(step)));
                } else {
                    model.push_front(step);
                    assert_eq!(d.push_front(step), Ok(()));
                }
            }
            2 => assert_eq!(d.pop_front(), model.pop_front()),
            _ => assert_eq!(d.pop_back(), model.pop_back()),
        }
        assert_eq!(d.len(), model.len());
        assert!(d.iter().eq(model.iter()));
        assert_eq!(d.front(), model.front());
        assert_eq!(d.back(), model.back());
        let (a, b) = d.as_slices();
        assert_eq!(a.len() + b.len(), model.len());
    }
}
