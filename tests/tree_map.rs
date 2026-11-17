use core::cell::Cell;
use std::collections::BTreeMap;
use std::ops::Bound;
use vas::tree_map::Node;
use vas::{Arena, Dynamic, Full, Inline, Memory, Rng, TreeMap};

/// Counts drops, to check that entries are dropped exactly once.
struct Tracked<'a>(&'a Cell<u32>);

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn starts_empty() {
    let mut memory = Memory::<Node<u32, u32>, 8>::new();
    let mut map = TreeMap::new(&mut memory);
    assert_eq!((map.len(), map.capacity(), map.remaining()), (0, 8, 8));
    assert!(map.is_empty());
    assert!(!map.is_full());
    assert_eq!(map.first(), None);
    assert_eq!(map.last(), None);
    assert_eq!(map.pop_first(), None);
    assert_eq!(map.pop_last(), None);
    assert_eq!(map.get(&1), None);
    assert_eq!(map.remove(&1), None);
    assert_eq!(map.iter().next(), None);
    assert_eq!(map.iter_mut().next_back(), None);
    assert_eq!(map.range(..).next(), None);
    assert_eq!(map.at_or_after(&0), None);
}

#[test]
fn insert_get_and_replace() {
    let mut map = TreeMap::<u32, &str, Inline<8>>::inline();
    assert_eq!(map.insert(2, "two"), Ok(None));
    assert_eq!(map.insert(1, "one"), Ok(None));
    assert_eq!(map.insert(3, "three"), Ok(None));
    assert_eq!(map.insert(2, "deux"), Ok(Some("two")));
    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&2), Some(&"deux"));
    assert!(map.contains_key(&3));
    assert!(!map.contains_key(&4));
    if let Some(value) = map.get_mut(&1) {
        *value = "uno";
    }
    assert!(map.iter().eq([(&1, &"uno"), (&2, &"deux"), (&3, &"three")]));
}

#[test]
fn full_map_hands_the_entry_back() {
    let mut memory = Memory::<Node<u32, String>, 3>::new();
    let mut map = TreeMap::new(&mut memory);
    for key in [5, 1, 3] {
        assert_eq!(map.insert(key, key.to_string()), Ok(None));
    }
    assert!(map.is_full());
    assert_eq!(
        map.insert(4, "four".to_string()),
        Err(Full((4, "four".to_string())))
    );
    // Replacing needs no room.
    assert_eq!(
        map.insert(3, "three".to_string()),
        Ok(Some("3".to_string()))
    );
    assert_eq!(map.reserve(0), Ok(()));
    assert_eq!(map.reserve(1), Err(Full(())));
    assert_eq!(map.remove(&1).as_deref(), Some("1"));
    assert_eq!(map.insert(4, "four".to_string()), Ok(None));
    assert!(map.iter().map(|(key, _)| *key).eq([3, 4, 5]));
}

#[test]
fn zero_capacity() {
    let mut map = TreeMap::<u32, u32, Inline<0>>::inline();
    assert!(map.is_full());
    assert_eq!(map.insert(1, 1), Err(Full((1, 1))));
    assert_eq!(map.get(&1), None);
}

#[test]
fn borrowed_keys_look_up_owned_ones() {
    let mut map = TreeMap::<String, u32, Inline<4>>::inline();
    assert!(map.insert("sword".to_string(), 3).is_ok());
    assert!(map.insert("bow".to_string(), 2).is_ok());
    assert_eq!(map.get("sword"), Some(&3));
    assert_eq!(
        map.at_or_after("c").map(|(key, _)| key.as_str()),
        Some("sword")
    );
    let early = (Bound::Included("a"), Bound::Excluded("c"));
    assert!(map.range::<str, _>(early).map(|(_, v)| *v).eq([2]));
    assert_eq!(map.remove_entry("bow"), Some(("bow".to_string(), 2)));
}

#[test]
fn ends_and_pops() {
    let mut map = TreeMap::<u32, u32, Inline<16>>::inline();
    for key in [7, 3, 9, 1, 5] {
        assert!(map.insert(key, key * 10).is_ok());
    }
    assert_eq!(map.first(), Some((&1, &10)));
    assert_eq!(map.last(), Some((&9, &90)));
    assert_eq!(map.pop_first(), Some((1, 10)));
    assert_eq!(map.pop_last(), Some((9, 90)));
    assert!(map.iter().map(|(key, _)| *key).eq([3, 5, 7]));
}

#[test]
fn nearest_keys() {
    let mut map = TreeMap::<u32, char, Inline<8>>::inline();
    for (key, value) in [(10, 'a'), (20, 'b'), (30, 'c')] {
        assert!(map.insert(key, value).is_ok());
    }
    assert_eq!(map.at_or_before(&5), None);
    assert_eq!(map.at_or_before(&10), Some((&10, &'a')));
    assert_eq!(map.at_or_before(&25), Some((&20, &'b')));
    assert_eq!(map.at_or_before(&99), Some((&30, &'c')));
    assert_eq!(map.at_or_after(&5), Some((&10, &'a')));
    assert_eq!(map.at_or_after(&20), Some((&20, &'b')));
    assert_eq!(map.at_or_after(&21), Some((&30, &'c')));
    assert_eq!(map.at_or_after(&31), None);
}

#[test]
fn iterates_from_both_ends() {
    let mut map = TreeMap::<u32, u32, Inline<16>>::inline();
    for key in [4, 2, 6, 1, 3, 5, 7] {
        assert!(map.insert(key, key).is_ok());
    }
    assert!(map.iter().rev().map(|(key, _)| *key).eq((1..=7).rev()));
    let mut iter = map.iter();
    assert_eq!(iter.next().map(|(key, _)| *key), Some(1));
    assert_eq!(iter.next_back().map(|(key, _)| *key), Some(7));
    assert_eq!(iter.next_back().map(|(key, _)| *key), Some(6));
    assert!(iter.by_ref().map(|(key, _)| *key).eq([2, 3, 4, 5]));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
    assert!((&map).into_iter().count() == 7);
}

#[test]
fn iter_mut_changes_values_in_key_order() {
    let mut map = TreeMap::<u32, u32, Inline<16>>::inline();
    for key in [4, 2, 6, 1, 3, 5, 7] {
        assert!(map.insert(key, 0).is_ok());
    }
    // Hold on to all the references at once.
    let mut entries: Vec<(&u32, &mut u32)> = map.iter_mut().collect();
    for (position, (_, value)) in entries.iter_mut().enumerate() {
        **value = position as u32;
    }
    assert!(entries.iter().map(|(key, _)| **key).eq(1..=7));
    assert!(map.iter().all(|(key, value)| *value + 1 == *key));

    let mut iter = map.iter_mut();
    let back = iter.next_back();
    let front = iter.next();
    assert_eq!(front.map(|(key, _)| *key), Some(1));
    assert_eq!(back.map(|(key, _)| *key), Some(7));
    assert_eq!(iter.count(), 5);
    for (_, value) in &mut map {
        *value += 100;
    }
    assert_eq!(map.get(&4), Some(&103));
}

#[test]
fn ranges() {
    let mut map = TreeMap::<u32, u32, Inline<16>>::inline();
    for key in (10..=90).step_by(10) {
        assert!(map.insert(key, key).is_ok());
    }
    let keys = |range: (Bound<u32>, Bound<u32>)| -> Vec<u32> {
        map.range(range).map(|(key, _)| *key).collect()
    };
    assert_eq!(
        keys((Bound::Included(20), Bound::Excluded(50))),
        [20, 30, 40]
    );
    assert_eq!(
        keys((Bound::Excluded(20), Bound::Included(50))),
        [30, 40, 50]
    );
    assert_eq!(keys((Bound::Included(25), Bound::Included(45))), [30, 40]);
    assert_eq!(keys((Bound::Unbounded, Bound::Excluded(30))), [10, 20]);
    assert_eq!(keys((Bound::Included(80), Bound::Unbounded)), [80, 90]);
    assert_eq!(keys((Bound::Included(31), Bound::Included(39))), []);
    assert_eq!(keys((Bound::Included(50), Bound::Included(20))), []);
    assert_eq!(keys((Bound::Included(95), Bound::Unbounded)), []);
    assert_eq!(keys((Bound::Unbounded, Bound::Excluded(10))), []);
    assert_eq!(keys((Bound::Excluded(40), Bound::Excluded(50))), []);
    assert!(map.range(..).count() == 9);
    assert!(
        map.range(30..=70)
            .rev()
            .map(|(key, _)| *key)
            .eq([70, 60, 50, 40, 30])
    );
}

#[test]
fn matches_a_reference_model() {
    let mut rng = Rng::new(42);
    let mut memory = Memory::<Node<u32, u32>, 512>::new();
    let mut map = TreeMap::new(&mut memory);
    let mut model = BTreeMap::new();
    // Fewer steps under Miri, which is slow.
    let steps = if cfg!(miri) { 500 } else { 30_000 };
    for step in 0..steps {
        let key = rng.u32_below(700);
        match rng.u32_below(8) {
            0..=2 => assert_eq!(map.remove(&key), model.remove(&key)),
            3 => assert_eq!(map.pop_first(), model.pop_first()),
            4 => {
                let (low, high) = (key, key + rng.u32_below(60));
                assert!(map.range(low..high).eq(model.range(low..high)));
                assert!(
                    map.range(low..=high)
                        .rev()
                        .eq(model.range(low..=high).rev())
                );
                assert_eq!(map.at_or_before(&key), model.range(..=key).next_back());
                assert_eq!(map.at_or_after(&key), model.range(key..).next());
            }
            _ => {
                if model.len() < map.capacity() || model.contains_key(&key) {
                    assert_eq!(map.insert(key, step), Ok(model.insert(key, step)));
                } else {
                    assert_eq!(map.insert(key, step), Err(Full((key, step))));
                }
            }
        }
        assert_eq!(map.len(), model.len());
        assert_eq!(map.get(&key), model.get(&key));
        assert_eq!(map.first(), model.first_key_value());
        assert_eq!(map.last(), model.last_key_value());
    }
    assert!(map.iter().eq(model.iter()));
    assert!(map.iter().rev().eq(model.iter().rev()));
    while let Some(entry) = map.pop_last() {
        assert_eq!(Some(entry), model.pop_last());
    }
    assert!(model.is_empty());
}

#[test]
fn entries_are_dropped_exactly_once() {
    let drops = Cell::new(0);
    {
        let mut map = TreeMap::<u32, Tracked<'_>, Inline<64>>::inline();
        for key in 0..50 {
            assert!(map.insert(key * 37 % 50, Tracked(&drops)).is_ok());
        }
        assert_eq!(drops.get(), 0);
        // Replacing hands the old value back.
        drop(map.insert(7, Tracked(&drops)));
        assert_eq!(drops.get(), 1);
        for key in 0..20 {
            drop(map.remove(&key));
        }
        assert_eq!(drops.get(), 21);
        drop(map.pop_first());
        assert_eq!(drops.get(), 22);
        assert_eq!(map.len(), 29);
    }
    assert_eq!(drops.get(), 51);

    let mut map = TreeMap::<u32, Tracked<'_>, Inline<8>>::inline();
    for key in 0..8 {
        assert!(map.insert(key, Tracked(&drops)).is_ok());
    }
    map.clear();
    assert_eq!(drops.get(), 59);
    assert!(map.is_empty());
    assert!(map.insert(1, Tracked(&drops)).is_ok());
    assert_eq!(map.len(), 1);
}

#[test]
fn grows_with_dynamic_storage() {
    let mut block = Memory::<u8, { 64 * 1024 }>::new();
    let arena = Arena::new(&mut block);
    let mut map = TreeMap::new(Dynamic(&arena));
    assert_eq!(map.capacity(), 0);
    // Fewer keys under Miri, which is slow.
    let count = if cfg!(miri) { 200u32 } else { 2000 };
    for key in 0..count {
        assert_eq!(map.insert(key * 7919 % count, key), Ok(None));
    }
    assert_eq!(map.len(), count as usize);
    assert!(map.iter().map(|(key, _)| *key).eq(0..count));
    assert_eq!(map.reserve(1_000_000), Err(Full(())));
    for key in (0..count).step_by(2) {
        assert!(map.remove(&key).is_some());
    }
    assert!(map.iter().map(|(key, _)| *key).eq((1..count).step_by(2)));
}

#[test]
fn clone_is_independent() {
    let mut map = TreeMap::<u32, String, Inline<16>>::inline();
    for key in [5, 2, 8, 1, 9, 3] {
        assert!(map.insert(key, key.to_string()).is_ok());
    }
    let mut copy = map.clone();
    assert!(copy.iter().eq(map.iter()));
    assert!(copy.remove(&5).is_some());
    assert!(copy.insert(4, "four".to_string()).is_ok());
    assert!(copy.iter().map(|(key, _)| *key).eq([1, 2, 3, 4, 8, 9]));
    assert!(map.iter().map(|(key, _)| *key).eq([1, 2, 3, 5, 8, 9]));
}

#[test]
fn usable_in_statics_and_structs() {
    static EMPTY: TreeMap<u32, u32, Inline<4>> = TreeMap::inline();
    assert!(EMPTY.is_empty());

    #[derive(Default)]
    struct Timeline {
        events: TreeMap<u32, &'static str, Inline<8>>,
    }
    let mut timeline = Timeline::default();
    assert!(timeline.events.insert(3, "boom").is_ok());
    assert_eq!(format!("{:?}", timeline.events), r#"{3: "boom"}"#);
}

#[cfg(feature = "alloc")]
#[test]
fn global_allocator_by_default() {
    let mut map = TreeMap::<u32, u32, Dynamic>::default();
    let count = if cfg!(miri) { 500u32 } else { 10_000 };
    for key in 0..count {
        assert!(map.insert(key * 7919 % count, key).is_ok());
    }
    assert_eq!(map.len(), count as usize);
    assert_eq!(map.first().map(|(key, _)| *key), Some(0));
}
