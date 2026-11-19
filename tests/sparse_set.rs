use core::cell::Cell;
use std::collections::BTreeMap;
use std::error::Error;
use std::vec::Vec;
use vas::{Arena, Dynamic, Full, Inline, Memory, Rng, SparseSet};

type Test = Result<(), Box<dyn Error>>;

#[test]
fn insert_get_remove() -> Test {
    let mut sparse = Memory::<u32, 100>::new();
    let mut ids = Memory::<u32, 8>::new();
    let mut values = Memory::<&str, 8>::new();
    let mut set = SparseSet::new(&mut sparse, &mut ids, &mut values);
    assert!(set.is_empty());
    assert_eq!((set.capacity(), set.id_capacity()), (8, 100));

    assert_eq!(set.insert(42, "a"), Ok(None));
    assert_eq!(set.insert(7, "b"), Ok(None));
    assert_eq!(set.insert(99, "c"), Ok(None));
    assert_eq!(set.len(), 3);
    assert_eq!(set.get(7), Some(&"b"));
    assert!(set.contains(99));
    assert!(!set.contains(8));
    assert!(!set.contains(1000));

    // Inserting again replaces.
    assert_eq!(set.insert(7, "B"), Ok(Some("b")));
    assert_eq!(set.len(), 3);
    *set.get_mut(42).ok_or("present")? = "A";

    // Values are one dense slice, in insertion order until a removal.
    assert_eq!(set.values(), &["A", "B", "c"]);
    assert_eq!(set.ids(), &[42, 7, 99]);
    assert_eq!(set.index_of(99), Some(2));

    // Removing moves the last value into the hole.
    assert_eq!(set.remove(42), Some("A"));
    assert_eq!(set.remove(42), None);
    assert_eq!(set.values(), &["c", "B"]);
    assert_eq!(set.ids(), &[99, 7]);
    assert_eq!(set.get(99), Some(&"c"));
    assert_eq!(set.index_of(99), Some(0));
    Ok(())
}

#[test]
fn full_hands_the_value_back() {
    let mut sparse = Memory::<u32, 10>::new();
    let mut ids = Memory::<u32, 2>::new();
    let mut values = Memory::<u8, 2>::new();
    let mut set = SparseSet::new(&mut sparse, &mut ids, &mut values);
    // An id past the sparse memory.
    assert_eq!(set.insert(10, 1), Err(Full(1)));
    assert_eq!(set.insert(u32::MAX, 1), Err(Full(1)));
    assert_eq!(set.insert(0, 1), Ok(None));
    assert_eq!(set.insert(9, 2), Ok(None));
    // No room left for values.
    assert_eq!(set.insert(5, 3), Err(Full(3)));
    // Replacing needs no room.
    assert_eq!(set.insert(9, 4), Ok(Some(2)));
    assert_eq!(set.values(), &[1, 4]);
    // Values and ids may have different sizes: the smaller is the capacity.
    let mut sparse = Memory::<u32, 10>::new();
    let mut ids = Memory::<u32, 4>::new();
    let mut values = Memory::<u8, 1>::new();
    let mut set = SparseSet::new(&mut sparse, &mut ids, &mut values);
    assert_eq!(set.capacity(), 1);
    assert_eq!(set.insert(3, 1), Ok(None));
    assert_eq!(set.insert(4, 2), Err(Full(2)));
    assert_eq!(set.ids(), &[3]);
    assert!(!set.contains(4));
}

#[test]
fn clear_forgets_everything() -> Test {
    let mut set = SparseSet::<u32, Inline<16>, Inline<4>>::inline();
    for id in [3, 1, 15] {
        set.insert(id, id * 10)?;
    }
    set.clear();
    assert!(set.is_empty());
    for id in 0..16 {
        assert!(!set.contains(id));
    }
    set.insert(15, 1)?;
    assert_eq!(set.get(15), Some(&1));
    assert_eq!(set.get(3), None);
    Ok(())
}

#[test]
fn iter_and_retain() -> Test {
    let mut set = SparseSet::<u32, Inline<32>, Inline<16>>::inline();
    for id in 0..10 {
        set.insert(id * 3, id)?;
    }
    let pairs: Vec<_> = set.iter().map(|(id, value)| (id, *value)).collect();
    assert_eq!(pairs.len(), 10);
    assert!(pairs.iter().all(|&(id, value)| id == value * 3));
    for (id, value) in set.iter_mut() {
        *value += id;
    }
    assert_eq!(set.get(9), Some(&12));
    set.retain(|id, value| {
        *value += 1;
        id % 2 == 0
    });
    assert_eq!(set.len(), 5);
    for id in 0..30 {
        assert_eq!(set.contains(id), id % 6 == 0, "{id}");
    }
    assert_eq!(set.get(6), Some(&9));
    let ids: std::collections::BTreeSet<_> = set.ids().iter().copied().collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(set.values_mut().len(), 5);
    Ok(())
}

#[test]
fn behaves_like_a_map() -> Test {
    let mut rng = Rng::new(21);
    let mut set = SparseSet::<u64, Inline<64>, Inline<40>>::inline();
    let mut model = BTreeMap::new();
    let steps = if cfg!(miri) { 300 } else { 20_000 };
    for step in 0..steps {
        let id = rng.u32_below(64);
        match rng.u32_below(3) {
            0 | 1 => {
                let result = set.insert(id, step);
                if model.len() == 40 && !model.contains_key(&id) {
                    assert_eq!(result, Err(Full(step)));
                } else {
                    assert_eq!(result, Ok(model.insert(id, step)));
                }
            }
            _ => assert_eq!(set.remove(id), model.remove(&id)),
        }
        assert_eq!(set.len(), model.len());
        if step % 97 == 0 {
            for id in 0..64 {
                assert_eq!(set.get(id), model.get(&id));
            }
            for (index, id) in set.ids().iter().enumerate() {
                assert_eq!(set.index_of(*id), Some(index));
                assert_eq!(set.values().get(index), model.get(id));
            }
        }
    }
    Ok(())
}

#[test]
fn drops_each_value_once() -> Test {
    struct Counted<'a>(&'a Cell<usize>);
    impl Drop for Counted<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Cell::new(0);
    {
        let mut set = SparseSet::<Counted<'_>, Inline<8>, Inline<4>>::inline();
        for id in 0..4 {
            set.insert(id, Counted(&drops)).map_err(|_| "full")?;
        }
        // Replacing returns the old value, which is dropped here.
        drop(set.insert(1, Counted(&drops)).map_err(|_| "full")?);
        assert_eq!(drops.get(), 1);
        drop(set.remove(2));
        assert_eq!(drops.get(), 2);
        set.retain(|id, _| id != 3);
        assert_eq!(drops.get(), 3);
    }
    // Two left in the set.
    assert_eq!(drops.get(), 5);
    Ok(())
}

#[test]
fn dynamic_storage_grows() -> Test {
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut set = SparseSet::new(Dynamic(&arena), Dynamic(&arena), Dynamic(&arena));
    for id in (0..2000).rev().step_by(3) {
        set.insert(id, u64::from(id))?;
    }
    assert_eq!(set.len(), 667);
    assert!(set.id_capacity() >= 2000);
    assert!(set.iter().all(|(id, value)| u64::from(id) == *value));
    Ok(())
}

#[test]
fn clone_is_independent() -> Test {
    let mut set = SparseSet::<String, Inline<8>, Inline<4>>::inline();
    set.insert(5, "five".into()).map_err(|_| "full")?;
    let copy = set.clone();
    set.insert(5, "FIVE".into()).map_err(|_| "full")?;
    assert_eq!(copy.get(5).map(String::as_str), Some("five"));
    assert_eq!(set.get(5).map(String::as_str), Some("FIVE"));
    assert_eq!(format!("{copy:?}"), r#"{5: "five"}"#);
    Ok(())
}
