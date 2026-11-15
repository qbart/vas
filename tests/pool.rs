use core::cell::Cell;
use vas::pool::Slot;
use vas::{Full, Handle, Memory, Pool};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let pool = Pool::new(&mut memory);
    assert_eq!(pool.len(), 0);
    assert_eq!(pool.capacity(), 4);
    assert!(pool.is_empty());
    assert!(!pool.is_full());
    assert_eq!(pool.iter().count(), 0);
}

#[test]
fn insert_and_get() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let mut pool = Pool::new(&mut memory);
    let a = pool.insert(10);
    let b = pool.insert(20);
    assert!(a.is_ok() && b.is_ok());
    if let (Ok(a), Ok(b)) = (a, b) {
        assert_ne!(a, b);
        assert_eq!(pool.get(a), Some(&10));
        assert_eq!(pool.get(b), Some(&20));
        assert!(pool.contains(a));
        assert_eq!(pool.len(), 2);
        if let Some(value) = pool.get_mut(a) {
            *value = 11;
        }
        assert_eq!(pool.get(a), Some(&11));
    }
}

#[test]
fn insert_when_full_returns_value() {
    let mut memory = Memory::<Slot<u32>, 1>::new();
    let mut pool = Pool::new(&mut memory);
    assert!(pool.insert(1).is_ok());
    assert!(pool.is_full());
    assert_eq!(pool.insert(2), Err(Full(2)));
    assert_eq!(pool.len(), 1);
}

#[test]
fn zero_capacity() {
    let mut memory = Memory::<Slot<u32>, 0>::new();
    let mut pool = Pool::new(&mut memory);
    assert!(pool.is_full());
    assert_eq!(pool.insert(1), Err(Full(1)));
}

#[test]
fn remove_makes_handle_stale() {
    let mut memory = Memory::<Slot<u32>, 2>::new();
    let mut pool = Pool::new(&mut memory);
    let handle = pool.insert(1);
    assert!(handle.is_ok());
    if let Ok(handle) = handle {
        assert_eq!(pool.remove(handle), Some(1));
        assert_eq!(pool.remove(handle), None);
        assert_eq!(pool.get(handle), None);
        assert_eq!(pool.get_mut(handle), None);
        assert!(!pool.contains(handle));
        assert!(pool.is_empty());
    }
}

#[test]
fn reused_slot_gets_a_new_handle() {
    let mut memory = Memory::<Slot<u32>, 1>::new();
    let mut pool = Pool::new(&mut memory);
    let old = pool.insert(1);
    assert!(old.is_ok());
    if let Ok(old) = old {
        assert_eq!(pool.remove(old), Some(1));
        let new = pool.insert(2);
        assert!(new.is_ok());
        if let Ok(new) = new {
            assert_ne!(old, new);
            assert_eq!(pool.get(old), None);
            assert_eq!(pool.get(new), Some(&2));
        }
    }
}

#[test]
fn free_slots_are_reused_before_fresh_ones() {
    let mut memory = Memory::<Slot<u32>, 3>::new();
    let mut pool = Pool::new(&mut memory);
    let mut handles: [Option<Handle<u32>>; 3] = [None; 3];
    for (i, slot) in handles.iter_mut().enumerate() {
        *slot = pool.insert(i as u32).ok();
    }
    assert!(pool.is_full());
    for handle in handles.iter().flatten() {
        assert!(pool.remove(*handle).is_some());
    }
    assert!(pool.is_empty());
    for i in 0..3 {
        assert!(pool.insert(i).is_ok());
    }
    assert!(pool.is_full());
    assert_eq!(pool.insert(9), Err(Full(9)));
    for handle in handles.iter().flatten() {
        assert_eq!(pool.get(*handle), None);
    }
}

#[test]
fn iter_visits_live_values_with_their_handles() {
    let mut memory = Memory::<Slot<u32>, 4>::new();
    let mut pool = Pool::new(&mut memory);
    let a = pool.insert(1).ok();
    let b = pool.insert(2).ok();
    let c = pool.insert(3).ok();
    if let Some(b) = b {
        assert_eq!(pool.remove(b), Some(2));
    }
    let mut seen = 0;
    for (handle, value) in &pool {
        assert_eq!(pool.get(handle), Some(value));
        assert!(Some(handle) == a || Some(handle) == c);
        seen += 1;
    }
    assert_eq!(seen, 2);
    for (_, value) in &mut pool {
        *value *= 10;
    }
    assert_eq!(pool.iter().map(|(_, v)| *v).sum::<u32>(), 40);
}

#[test]
fn clear_drops_values_and_invalidates_handles() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Slot<Counted>, 3>::new();
    let mut pool = Pool::new(&mut memory);
    let a = pool.insert(Counted(&drops)).ok();
    let b = pool.insert(Counted(&drops)).ok();
    pool.clear();
    assert_eq!(drops.get(), 2);
    assert!(pool.is_empty());
    for handle in [a, b].into_iter().flatten() {
        assert!(!pool.contains(handle));
    }
    for _ in 0..3 {
        assert!(pool.insert(Counted(&drops)).is_ok());
    }
    assert!(pool.is_full());
}

#[test]
fn drop_drops_live_values_only() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Slot<Counted>, 3>::new();
    {
        let mut pool = Pool::new(&mut memory);
        let a = pool.insert(Counted(&drops)).ok();
        assert!(pool.insert(Counted(&drops)).is_ok());
        if let Some(a) = a {
            drop(pool.remove(a));
        }
        assert_eq!(drops.get(), 1);
    }
    assert_eq!(drops.get(), 2);
}

#[test]
fn handle_is_small_and_copy() {
    fn assert_copy<T: Copy + Send + Sync>() {}
    assert_copy::<Handle<*const u8>>();
    assert_eq!(size_of::<Handle<[u8; 100]>>(), 8);
}
