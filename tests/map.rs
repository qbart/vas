use core::cell::Cell;
use core::hash::{BuildHasher, Hasher};
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::string::String;
use vas::hash::{FastHasher, FastState};
use vas::map::Slot;
use vas::{Full, Map, Memory};

struct Counted<'a>(&'a Cell<usize>);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

/// Sends every key to the same bucket: the worst case for probing.
#[derive(Clone, Copy, Default)]
struct Colliding;

struct CollidingHasher;

impl Hasher for CollidingHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _: &[u8]) {}
}

impl BuildHasher for Colliding {
    type Hasher = CollidingHasher;
    fn build_hasher(&self) -> CollidingHasher {
        CollidingHasher
    }
}

#[test]
fn new_is_empty() {
    let mut memory = Memory::<Slot<u32, u32>, 8>::new();
    let map = Map::new(&mut memory);
    assert_eq!(map.len(), 0);
    assert_eq!(map.capacity(), 8);
    assert!(map.is_empty());
    assert!(!map.is_full());
    assert_eq!(map.get(&1), None);
    assert_eq!(map.iter().count(), 0);
}

#[test]
fn insert_get_and_replace() {
    let mut memory = Memory::<Slot<u32, &str>, 8>::new();
    let mut map = Map::new(&mut memory);
    assert_eq!(map.insert(1, "one"), Ok(None));
    assert_eq!(map.insert(2, "two"), Ok(None));
    assert_eq!(map.get(&1), Some(&"one"));
    assert_eq!(map.get(&2), Some(&"two"));
    assert_eq!(map.get(&3), None);
    assert!(map.contains_key(&1));
    assert!(!map.contains_key(&3));
    assert_eq!(map.insert(1, "uno"), Ok(Some("one")));
    assert_eq!(map.get(&1), Some(&"uno"));
    assert_eq!(map.len(), 2);
    if let Some(value) = map.get_mut(&2) {
        *value = "dos";
    }
    assert_eq!(map.get(&2), Some(&"dos"));
}

#[test]
fn insert_when_full_returns_pair() {
    let mut memory = Memory::<Slot<u32, u32>, 2>::new();
    let mut map = Map::new(&mut memory);
    assert_eq!(map.insert(1, 10), Ok(None));
    assert_eq!(map.insert(2, 20), Ok(None));
    assert!(map.is_full());
    assert_eq!(map.insert(3, 30), Err(Full((3, 30))));
    // Replacing an existing key still works in a full map.
    assert_eq!(map.insert(2, 21), Ok(Some(20)));
    // Looking up a missing key in a full map terminates.
    assert_eq!(map.get(&3), None);
    assert_eq!(map.remove(&3), None);
    assert_eq!(map.len(), 2);
}

#[test]
fn zero_capacity() {
    let mut memory = Memory::<Slot<u32, u32>, 0>::new();
    let mut map = Map::new(&mut memory);
    assert!(map.is_full());
    assert_eq!(map.insert(1, 1), Err(Full((1, 1))));
    assert_eq!(map.get(&1), None);
    assert_eq!(map.remove(&1), None);
}

#[test]
fn remove_returns_value_and_frees_room() {
    let mut memory = Memory::<Slot<u32, u32>, 2>::new();
    let mut map = Map::new(&mut memory);
    assert_eq!(map.insert(1, 10), Ok(None));
    assert_eq!(map.insert(2, 20), Ok(None));
    assert_eq!(map.remove(&1), Some(10));
    assert_eq!(map.remove(&1), None);
    assert_eq!(map.get(&1), None);
    assert_eq!(map.get(&2), Some(&20));
    assert_eq!(map.insert(3, 30), Ok(None));
    assert_eq!(map.remove_entry(&3), Some((3, 30)));
    assert_eq!(map.len(), 1);
}

#[test]
fn lookup_by_borrowed_key() {
    let mut memory = Memory::<Slot<String, u32>, 4>::new();
    let mut map = Map::new(&mut memory);
    assert_eq!(map.insert(String::from("sword"), 7), Ok(None));
    assert_eq!(map.get("sword"), Some(&7));
    assert_eq!(map.get("shield"), None);
    assert_eq!(map.remove("sword"), Some(7));
    assert!(map.is_empty());
}

#[test]
fn iterates_all_entries() {
    let mut memory = Memory::<Slot<u32, u32>, 16>::new();
    let mut map = Map::new(&mut memory);
    for key in 0..10 {
        assert_eq!(map.insert(key, key * 2), Ok(None));
    }
    assert_eq!(map.remove(&4), Some(8));
    let mut count = 0;
    let mut key_sum = 0;
    for (key, value) in &map {
        assert_eq!(*value, key * 2);
        key_sum += key;
        count += 1;
    }
    assert_eq!(count, 9);
    assert_eq!(key_sum, 45 - 4);
    for (_, value) in &mut map {
        *value += 1;
    }
    assert_eq!(map.get(&3), Some(&7));
}

#[test]
fn drops_keys_and_values() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Slot<u32, Counted>, 4>::new();
    {
        let mut map = Map::new(&mut memory);
        assert!(map.insert(1, Counted(&drops)).is_ok());
        assert!(map.insert(2, Counted(&drops)).is_ok());
        assert!(map.insert(3, Counted(&drops)).is_ok());
        // Replacing hands the old value back.
        drop(map.insert(1, Counted(&drops)));
        assert_eq!(drops.get(), 1);
        drop(map.remove(&2));
        assert_eq!(drops.get(), 2);
    }
    assert_eq!(drops.get(), 4);
}

#[test]
fn clear_empties_and_allows_reuse() {
    let drops = Cell::new(0);
    let mut memory = Memory::<Slot<u32, Counted>, 2>::new();
    let mut map = Map::new(&mut memory);
    assert!(map.insert(1, Counted(&drops)).is_ok());
    assert!(map.insert(2, Counted(&drops)).is_ok());
    map.clear();
    assert_eq!(drops.get(), 2);
    assert!(map.is_empty());
    assert_eq!(map.get(&1).map(|_| ()), None);
    assert!(map.insert(3, Counted(&drops)).is_ok());
    assert!(map.insert(4, Counted(&drops)).is_ok());
    assert!(map.is_full());
}

/// Runs a pseudo-random mix of operations against std's HashMap.
fn check_against_std<S: BuildHasher>(hasher: S, capacity_is_32: &mut Memory<Slot<u16, u32>, 32>) {
    let mut map = Map::with_hasher(capacity_is_32, hasher);
    let mut model: HashMap<u16, u32> = HashMap::new();
    let mut seed: u32 = 0x1234_5678;
    let steps: u32 = if cfg!(miri) { 400 } else { 3000 };
    for step in 0..steps {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let key = ((seed >> 16) % 48) as u16;
        if (seed >> 8).is_multiple_of(3) {
            assert_eq!(map.remove(&key), model.remove(&key));
        } else if model.len() == 32 && !model.contains_key(&key) {
            assert_eq!(map.insert(key, step), Err(Full((key, step))));
        } else {
            assert_eq!(map.insert(key, step), Ok(model.insert(key, step)));
        }
        assert_eq!(map.len(), model.len());
        assert_eq!(map.get(&key), model.get(&key));
    }
    for key in 0..48 {
        assert_eq!(map.get(&key), model.get(&key));
    }
    assert_eq!(map.iter().count(), model.len());
    for (key, value) in &map {
        assert_eq!(model.get(key), Some(value));
    }
}

#[test]
fn matches_std_with_default_hasher() {
    let mut memory = Memory::new();
    check_against_std(FastState::default(), &mut memory);
}

#[test]
fn matches_std_when_every_key_collides() {
    let mut memory = Memory::new();
    check_against_std(Colliding, &mut memory);
}

#[test]
fn matches_std_with_std_hasher() {
    let mut memory = Memory::new();
    check_against_std(RandomState::new(), &mut memory);
}

#[test]
fn fast_hasher_is_deterministic_and_spreads() {
    let state = FastState::default();
    assert_eq!(state.hash_one(42u32), state.hash_one(42u32));
    assert_ne!(state.hash_one(1u32), state.hash_one(2u32));
    assert_ne!(state.hash_one("ab"), state.hash_one("ba"));
    assert_ne!(state.hash_one("12345678"), state.hash_one("123456789"));
    assert_ne!(state.hash_one([1u8, 0]), state.hash_one([1u8]));
    let mut a = FastHasher::default();
    let mut b = FastHasher::default();
    a.write(b"hello world, hello");
    b.write(b"hello world, hello");
    assert_eq!(a.finish(), b.finish());
}
