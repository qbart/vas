use core::cell::Cell;
use std::collections::VecDeque;
use std::error::Error;
use std::vec::Vec;
use vas::lru::Entry;
use vas::{Arena, Dynamic, Full, Inline, LruCache, Memory, Rng};

type Test = Result<(), Box<dyn Error>>;

#[test]
fn evicts_the_least_recently_used() -> Test {
    let mut entries = Memory::<Entry<u32, &str>, 3>::new();
    let mut table = Memory::<u32, 8>::new();
    let mut cache = LruCache::new(&mut entries, &mut table, 3)?;
    assert_eq!(cache.capacity(), 3);
    assert_eq!(cache.insert(1, "one"), None);
    assert_eq!(cache.insert(2, "two"), None);
    assert_eq!(cache.insert(3, "three"), None);
    // Using 1 makes 2 the oldest.
    assert_eq!(cache.get(&1), Some(&"one"));
    assert_eq!(cache.insert(4, "four"), Some((2, "two")));
    assert!(!cache.contains(&2));
    assert_eq!(cache.len(), 3);
    // Most recent first.
    let order: Vec<_> = cache.iter().map(|(k, _)| *k).collect();
    assert_eq!(order, [4, 1, 3]);
    assert_eq!(cache.peek_lru(), Some((&3, &"three")));
    // `peek` does not count as a use.
    assert_eq!(cache.peek(&3), Some(&"three"));
    assert_eq!(cache.insert(5, "five"), Some((3, "three")));
    Ok(())
}

#[test]
fn insert_replaces_and_refreshes() -> Test {
    let mut cache = LruCache::<u32, u32, Inline<2>, Inline<4>>::inline();
    assert_eq!(cache.capacity(), 2);
    cache.insert(1, 10);
    cache.insert(2, 20);
    assert_eq!(cache.insert(1, 11), Some((1, 10)));
    assert_eq!(cache.insert(3, 30), Some((2, 20)));
    assert_eq!(cache.get(&1), Some(&11));
    if let Some(value) = cache.get_mut(&3) {
        *value += 1;
    }
    assert_eq!(cache.peek(&3), Some(&31));
    Ok(())
}

#[test]
fn remove_and_pop() -> Test {
    let mut cache = LruCache::<u32, u32, Inline<4>, Inline<8>>::inline();
    for key in 0..4 {
        cache.insert(key, key * 10);
    }
    assert_eq!(cache.remove(&2), Some(20));
    assert_eq!(cache.remove(&2), None);
    assert_eq!(cache.pop_lru(), Some((0, 0)));
    assert_eq!(cache.pop_lru(), Some((1, 10)));
    assert_eq!(cache.len(), 1);
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.pop_lru(), None);
    assert_eq!(cache.peek_lru(), None);
    cache.insert(7, 70);
    assert_eq!(cache.get(&7), Some(&70));
    Ok(())
}

#[test]
fn capacity_must_fit_the_memory() {
    let mut entries = Memory::<Entry<u8, u8>, 4>::new();
    let mut table = Memory::<u32, 8>::new();
    assert!(LruCache::new(&mut entries, &mut table, 5).is_err());
    let mut entries = Memory::<Entry<u8, u8>, 8>::new();
    let mut table = Memory::<u32, 8>::new();
    // The table must have a third more room than entries: 6 of 8.
    assert_eq!(
        LruCache::new(&mut entries, &mut table, 7).err(),
        Some(Full(()))
    );
    let mut entries = Memory::<Entry<u8, u8>, 8>::new();
    let mut table = Memory::<u32, 8>::new();
    assert!(LruCache::new(&mut entries, &mut table, 6).is_ok());
    // Inline picks the largest capacity that fits.
    let cache = LruCache::<u8, u8, Inline<64>, Inline<20>>::inline();
    assert_eq!(cache.capacity(), 12);
}

#[test]
fn zero_capacity_keeps_nothing() -> Test {
    let mut cache = LruCache::<u8, u8, Inline<0>, Inline<0>>::inline();
    assert_eq!(cache.insert(1, 1), Some((1, 1)));
    assert!(cache.is_empty());
    assert_eq!(cache.get(&1), None);
    Ok(())
}

/// Random operations against a model: a list, most recent first.
#[test]
fn behaves_like_a_model() -> Test {
    let mut rng = Rng::new(31);
    let mut cache = LruCache::<u32, u64, Inline<24>, Inline<64>>::inline();
    let capacity = cache.capacity();
    assert_eq!(capacity, 24);
    let mut model: VecDeque<(u32, u64)> = VecDeque::new();
    let steps = if cfg!(miri) { 400 } else { 30_000 };
    for step in 0..steps {
        let key = rng.u32_below(48);
        let at = model.iter().position(|(k, _)| *k == key);
        match rng.u32_below(4) {
            0 | 1 => {
                let out = cache.insert(key, step);
                let expected = match at {
                    Some(i) => model.remove(i),
                    None if model.len() == capacity => model.pop_back(),
                    None => None,
                };
                model.push_front((key, step));
                assert_eq!(out, expected);
            }
            2 => {
                let got = cache.get(&key).copied();
                if let Some(i) = at {
                    let entry = model.remove(i).ok_or("in model")?;
                    model.push_front(entry);
                    assert_eq!(got, Some(entry.1));
                } else {
                    assert_eq!(got, None);
                }
            }
            _ => {
                let got = cache.remove(&key);
                assert_eq!(got, at.and_then(|i| model.remove(i)).map(|(_, v)| v));
            }
        }
        if step % 53 == 0 {
            let order: Vec<_> = cache.iter().map(|(k, v)| (*k, *v)).collect();
            assert_eq!(order, model.iter().copied().collect::<Vec<_>>());
        }
    }
    Ok(())
}

#[test]
fn drops_each_entry_once() -> Test {
    struct Counted<'a>(&'a Cell<usize>);
    impl Drop for Counted<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let drops = Cell::new(0);
    {
        let mut cache = LruCache::<u32, Counted<'_>, Inline<2>, Inline<4>>::inline();
        drop(cache.insert(1, Counted(&drops)));
        drop(cache.insert(2, Counted(&drops)));
        // Evicts 1; the caller drops it.
        drop(cache.insert(3, Counted(&drops)));
        assert_eq!(drops.get(), 1);
        drop(cache.remove(&2));
        assert_eq!(drops.get(), 2);
    }
    assert_eq!(drops.get(), 3);
    Ok(())
}

#[test]
fn dynamic_storage_takes_its_memory_up_front() -> Test {
    let mut block = Memory::<u8, 65536>::new();
    let arena = Arena::new(&mut block);
    let mut cache = LruCache::new(Dynamic(&arena), Dynamic(&arena), 500)?;
    for key in 0..2000u32 {
        cache.insert(key, u64::from(key));
    }
    assert_eq!(cache.len(), 500);
    assert_eq!(cache.peek_lru(), Some((&1500, &1500)));
    assert!(LruCache::<u32, u64, _, _>::new(Dynamic(&arena), Dynamic(&arena), 100_000).is_err());
    Ok(())
}

#[test]
fn string_keys() -> Test {
    let mut cache = LruCache::<&str, u32, Inline<2>, Inline<4>>::inline();
    cache.insert("grass.png", 1);
    cache.insert("stone.png", 2);
    assert_eq!(cache.get(&"grass.png"), Some(&1));
    assert_eq!(cache.insert("water.png", 3), Some(("stone.png", 2)));
    assert_eq!(format!("{cache:?}"), r#"{"water.png": 3, "grass.png": 1}"#);
    Ok(())
}

/// A hasher with only four outcomes: long probe runs that wrap around the
/// table, exercising removal's backward shift.
#[derive(Default)]
struct Clumpy(u64);

impl core::hash::Hasher for Clumpy {
    fn finish(&self) -> u64 {
        // Lands near the end of a 16-slot table, so runs wrap.
        13 + self.0 % 4
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.wrapping_mul(31).wrapping_add(u64::from(*byte));
        }
    }
}

#[test]
fn colliding_hashes_stay_findable() -> Test {
    type State = core::hash::BuildHasherDefault<Clumpy>;
    let mut entries = Memory::<Entry<u32, u32>, 12>::new();
    let mut table = Memory::<u32, 16>::new();
    let mut cache = LruCache::with_hasher(&mut entries, &mut table, 12, State::default())?;
    let mut rng = Rng::new(8);
    let mut model: VecDeque<(u32, u32)> = VecDeque::new();
    let steps = if cfg!(miri) { 300 } else { 20_000 };
    for step in 0..steps {
        let key = rng.u32_below(30);
        let at = model.iter().position(|(k, _)| *k == key);
        if rng.bool() {
            let out = cache.insert(key, step);
            let expected = match at {
                Some(i) => model.remove(i),
                None if model.len() == 12 => model.pop_back(),
                None => None,
            };
            model.push_front((key, step));
            assert_eq!(out, expected);
        } else {
            assert_eq!(
                cache.remove(&key),
                at.and_then(|i| model.remove(i)).map(|(_, v)| v)
            );
        }
        for (key, value) in &model {
            assert_eq!(cache.peek(key), Some(value), "step {step}");
        }
    }
    Ok(())
}

/// A hasher that hashes `u32` keys to `key << SHIFT`: with a large shift
/// every key has the same table slot and only the upper bits differ; with
/// none, keys spread over the slots and the upper bits are all zero.
#[derive(Default)]
struct Shifted<const SHIFT: u32>(u64);

impl<const SHIFT: u32> core::hash::Hasher for Shifted<SHIFT> {
    fn finish(&self) -> u64 {
        self.0 << SHIFT
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes.iter().rev() {
            self.0 = (self.0 << 8) | u64::from(*byte);
        }
    }
    fn write_u32(&mut self, value: u32) {
        self.0 = u64::from(value);
    }
}

/// Inserts, gets and removes against a model, with a table larger than
/// needed, and checks every cached key is found.
fn churn<H: core::hash::BuildHasher>(hasher: H) -> Test {
    let mut entries = Memory::<Entry<u32, u32>, 20>::new();
    let mut table = Memory::<u32, 64>::new();
    let mut cache = LruCache::with_hasher(&mut entries, &mut table, 20, hasher)?;
    let mut rng = Rng::new(21);
    let mut model: VecDeque<(u32, u32)> = VecDeque::new();
    let steps = if cfg!(miri) { 300 } else { 20_000 };
    for step in 0..steps {
        let key = rng.u32_below(50);
        let at = model.iter().position(|(k, _)| *k == key);
        match rng.u32_below(3) {
            0 => {
                let out = cache.insert(key, step);
                let expected = match at {
                    Some(i) => model.remove(i),
                    None if model.len() == 20 => model.pop_back(),
                    None => None,
                };
                model.push_front((key, step));
                assert_eq!(out, expected);
            }
            1 => {
                let got = cache.get(&key).copied();
                if let Some(i) = at {
                    let entry = model.remove(i).ok_or("in model")?;
                    model.push_front(entry);
                    assert_eq!(got, Some(entry.1));
                } else {
                    assert_eq!(got, None);
                }
            }
            _ => {
                assert_eq!(
                    cache.remove(&key),
                    at.and_then(|i| model.remove(i)).map(|(_, v)| v)
                );
            }
        }
        for (key, value) in &model {
            assert_eq!(cache.peek(key), Some(value), "step {step}");
        }
        if step % 97 == 0 && cache.pop_lru() != model.pop_back() {
            return Err("pop_lru".into());
        }
    }
    Ok(())
}

#[test]
fn keys_alike_in_the_low_or_high_hash_bits_stay_apart() -> Test {
    use core::hash::BuildHasherDefault;
    // Same slot for every key, different upper bits.
    churn(BuildHasherDefault::<Shifted<16>>::default())?;
    // Different slots, upper bits all zero.
    churn(BuildHasherDefault::<Shifted<0>>::default())?;
    // Upper bits that differ only above the slot bits of a 64-slot table.
    churn(BuildHasherDefault::<Shifted<6>>::default())
}

#[test]
fn a_table_too_large_to_keep_home_bits_still_works() -> Test {
    use core::mem::MaybeUninit;
    // 131,072 table slots: the index takes 17 bits, too many to keep the
    // home slot's bits beside it, so removal reads the entries instead.
    let capacity = 49_153;
    let mut entries: Vec<MaybeUninit<Entry<u32, u32>>> =
        (0..capacity).map(|_| MaybeUninit::uninit()).collect();
    let mut table: Vec<MaybeUninit<u32>> = (0..1 << 17).map(|_| MaybeUninit::uninit()).collect();
    let mut cache = LruCache::new(&mut entries[..], &mut table[..], capacity)?;
    let count = if cfg!(miri) { 600 } else { 60_000u32 };
    let mut rng = Rng::new(5);
    let mut model = std::collections::HashMap::new();
    for key in 0..count {
        let key = key.wrapping_mul(2_654_435_761) ^ rng.u32_below(4);
        if let Some((old, _)) = cache.insert(key, key / 2) {
            model.remove(&old);
        }
        model.insert(key, key / 2);
        if key.is_multiple_of(7) {
            assert_eq!(cache.remove(&key), model.remove(&key));
        }
    }
    assert_eq!(cache.len(), model.len());
    for (key, value) in &model {
        assert_eq!(cache.peek(key), Some(value));
    }
    Ok(())
}
