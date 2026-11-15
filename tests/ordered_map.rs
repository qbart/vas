use core::cell::Cell;
use core::ops::Bound;
use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;
use vas::{Full, Memory, OrderedMap, Rng};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<(u32, u32), 4>::new();
    let map = OrderedMap::new(&mut memory);
    assert_eq!(map.len(), 0);
    assert_eq!(map.capacity(), 4);
    assert_eq!(map.remaining(), 4);
    assert!(map.is_empty());
    assert!(!map.is_full());
    assert_eq!(map.first(), None);
    assert_eq!(map.last(), None);
    assert_eq!(map.get(&1), None);
}

#[test]
fn keeps_keys_sorted() {
    let mut memory = Memory::<(u32, &str), 8>::new();
    let mut map = OrderedMap::new(&mut memory);
    for (key, value) in [(5, "five"), (1, "one"), (9, "nine"), (3, "three")] {
        assert_eq!(map.insert(key, value), Ok(None));
    }
    assert!(map.iter().map(|(key, _)| *key).eq([1, 3, 5, 9]));
    assert!(map.iter().rev().map(|(key, _)| *key).eq([9, 5, 3, 1]));
    assert_eq!(
        map.as_slice(),
        &[(1, "one"), (3, "three"), (5, "five"), (9, "nine")]
    );
    assert_eq!(map.first(), Some((&1, &"one")));
    assert_eq!(map.last(), Some((&9, &"nine")));
    assert_eq!(map.get(&3), Some(&"three"));
    assert_eq!(map.get(&4), None);
    assert!(map.contains_key(&9));
    assert_eq!(map.insert(3, "tres"), Ok(Some("three")));
    assert_eq!(map.len(), 4);
    if let Some(value) = map.get_mut(&5) {
        *value = "cinco";
    }
    assert_eq!(map.get(&5), Some(&"cinco"));
    for (_, value) in map.iter_mut() {
        *value = "x";
    }
    assert_eq!(map.get(&1), Some(&"x"));
}

#[test]
fn insert_when_full_returns_pair() {
    let mut memory = Memory::<(u32, u32), 2>::new();
    let mut map = OrderedMap::new(&mut memory);
    assert_eq!(map.insert(2, 20), Ok(None));
    assert_eq!(map.insert(1, 10), Ok(None));
    assert!(map.is_full());
    assert_eq!(map.insert(3, 30), Err(Full((3, 30))));
    assert_eq!(map.insert(0, 0), Err(Full((0, 0))));
    // Replacing still works when full.
    assert_eq!(map.insert(1, 11), Ok(Some(10)));
    assert_eq!(map.as_slice(), &[(1, 11), (2, 20)]);
}

#[test]
fn remove_and_pop() {
    let mut memory = Memory::<(u32, u32), 8>::new();
    let mut map = OrderedMap::new(&mut memory);
    for key in [4, 2, 8, 6] {
        assert_eq!(map.insert(key, key * 10), Ok(None));
    }
    assert_eq!(map.remove(&4), Some(40));
    assert_eq!(map.remove(&4), None);
    assert_eq!(map.remove_entry(&6), Some((6, 60)));
    assert_eq!(map.as_slice(), &[(2, 20), (8, 80)]);
    assert_eq!(map.pop_first(), Some((2, 20)));
    assert_eq!(map.pop_last(), Some((8, 80)));
    assert_eq!(map.pop_first(), None);
    assert_eq!(map.pop_last(), None);
}

#[test]
fn range_queries() {
    let mut memory = Memory::<(u32, u32), 8>::new();
    let mut map = OrderedMap::new(&mut memory);
    for key in [10, 20, 30, 40, 50] {
        assert_eq!(map.insert(key, key), Ok(None));
    }
    let keys =
        |iter: vas::ordered_map::Iter<'_, u32, u32>| iter.map(|(key, _)| *key).collect::<Vec<_>>();
    assert_eq!(keys(map.range(20..40)), [20, 30]);
    assert_eq!(keys(map.range(20..=40)), [20, 30, 40]);
    assert_eq!(keys(map.range(15..35)), [20, 30]);
    assert_eq!(keys(map.range(..25)), [10, 20]);
    assert_eq!(keys(map.range(45..)), [50]);
    assert_eq!(keys(map.range(..)), [10, 20, 30, 40, 50]);
    assert_eq!(keys(map.range(60..)), []);
    assert_eq!(keys(map.range(21..29)), []);
    // An inverted range is empty, not a panic.
    #[allow(clippy::reversed_empty_ranges)]
    let inverted = map.range(40..20);
    assert_eq!(keys(inverted), []);
}

#[test]
fn nearest_keys() {
    let mut memory = Memory::<(u32, &str), 8>::new();
    let mut frames = OrderedMap::new(&mut memory);
    for (time, pose) in [(0, "idle"), (10, "step"), (20, "jump")] {
        assert_eq!(frames.insert(time, pose), Ok(None));
    }
    assert_eq!(frames.at_or_before(&15), Some((&10, &"step")));
    assert_eq!(frames.at_or_before(&10), Some((&10, &"step")));
    assert_eq!(frames.at_or_before(&99), Some((&20, &"jump")));
    assert_eq!(frames.at_or_after(&15), Some((&20, &"jump")));
    assert_eq!(frames.at_or_after(&0), Some((&0, &"idle")));
    assert_eq!(frames.at_or_after(&21), None);
    frames.clear();
    assert_eq!(frames.at_or_before(&5), None);
    assert_eq!(frames.at_or_after(&5), None);
}

#[test]
fn nothing_before_the_first_key() {
    let mut memory = Memory::<(i32, i32), 2>::new();
    let mut map = OrderedMap::new(&mut memory);
    assert_eq!(map.insert(5, 0), Ok(None));
    assert_eq!(map.at_or_before(&4), None);
}

#[test]
fn lookup_by_borrowed_key() {
    let mut memory = Memory::<(String, u32), 4>::new();
    let mut map = OrderedMap::new(&mut memory);
    assert_eq!(map.insert(String::from("sword"), 7), Ok(None));
    assert_eq!(map.insert(String::from("axe"), 3), Ok(None));
    assert_eq!(map.get("sword"), Some(&7));
    assert_eq!(map.first().map(|(key, _)| key.as_str()), Some("axe"));
    let bounds = (Bound::Included("b"), Bound::Excluded("t"));
    assert_eq!(map.range::<str, _>(bounds).count(), 1);
    assert_eq!(map.remove("axe"), Some(3));
}

#[test]
fn drops_entries() {
    let drops = Cell::new(0);
    let mut memory = Memory::<(u32, Counted), 4>::new();
    {
        let mut map = OrderedMap::new(&mut memory);
        for key in [3, 1, 2] {
            assert!(map.insert(key, Counted(&drops)).is_ok());
        }
        drop(map.insert(2, Counted(&drops)));
        assert_eq!(drops.get(), 1);
        drop(map.remove(&1));
        assert_eq!(drops.get(), 2);
        map.clear();
        assert_eq!(drops.get(), 4);
        assert!(map.insert(9, Counted(&drops)).is_ok());
    }
    assert_eq!(drops.get(), 5);
}

#[test]
fn matches_std_btreemap() {
    const CAPACITY: usize = 24;
    let mut memory = Memory::<(u16, u32), CAPACITY>::new();
    let mut map = OrderedMap::new(&mut memory);
    let mut model: BTreeMap<u16, u32> = BTreeMap::new();
    let mut rng = Rng::new(404);
    let steps = if cfg!(miri) { 300 } else { 6000 };
    for step in 0..steps {
        let key = rng.u32_below(40) as u16;
        match rng.u32_below(6) {
            0..=2 => {
                if model.len() == CAPACITY && !model.contains_key(&key) {
                    assert_eq!(map.insert(key, step), Err(Full((key, step))));
                } else {
                    assert_eq!(map.insert(key, step), Ok(model.insert(key, step)));
                }
            }
            3 => assert_eq!(map.remove(&key), model.remove(&key)),
            4 => assert_eq!(map.pop_first(), model.pop_first()),
            _ => {
                let end = key + rng.u32_below(10) as u16;
                assert!(map.range(key..end).eq(model.range(key..end)));
                assert!(map.range(key..=end).eq(model.range(key..=end)));
                assert_eq!(map.at_or_before(&key), model.range(..=key).next_back());
                assert_eq!(map.at_or_after(&key), model.range(key..).next());
            }
        }
        assert_eq!(map.len(), model.len());
        assert_eq!(map.get(&key), model.get(&key));
        assert_eq!(map.first(), model.first_key_value());
        assert_eq!(map.last(), model.last_key_value());
    }
    assert!(map.iter().eq(model.iter()));
}
