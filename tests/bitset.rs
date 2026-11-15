use vas::bitset::words;
use vas::{Bitset, Memory, OutOfBounds};

#[test]
fn new_is_all_clear() {
    let mut memory = Memory::<u64, 2>::new();
    let bits = Bitset::new(&mut memory);
    assert_eq!(bits.capacity(), 128);
    assert_eq!(bits.count(), 0);
    assert!(bits.is_empty());
    assert!(!bits.contains(0));
    assert!(!bits.contains(127));
    assert_eq!(bits.first_set(), None);
    assert_eq!(bits.first_clear(), Some(0));
    assert_eq!(bits.iter().count(), 0);
}

#[test]
fn insert_contains_remove() {
    let mut memory = Memory::<u64, 2>::new();
    let mut bits = Bitset::new(&mut memory);
    assert_eq!(bits.insert(3), Ok(true));
    assert_eq!(bits.insert(3), Ok(false));
    assert_eq!(bits.insert(64), Ok(true));
    assert_eq!(bits.insert(127), Ok(true));
    assert!(bits.contains(3));
    assert!(bits.contains(64));
    assert!(bits.contains(127));
    assert!(!bits.contains(4));
    assert_eq!(bits.count(), 3);
    assert!(bits.remove(64));
    assert!(!bits.remove(64));
    assert!(!bits.contains(64));
    assert_eq!(bits.count(), 2);
}

#[test]
fn out_of_bounds_is_reported_not_panicked() {
    let mut memory = Memory::<u64, 1>::new();
    let mut bits = Bitset::new(&mut memory);
    assert_eq!(bits.insert(64), Err(OutOfBounds));
    assert_eq!(bits.set(64, true), Err(OutOfBounds));
    assert_eq!(bits.toggle(usize::MAX), Err(OutOfBounds));
    assert!(!bits.contains(64));
    assert!(!bits.remove(64));
    assert_eq!(bits.count(), 0);
}

#[test]
fn set_and_toggle() {
    let mut memory = Memory::<u64, 1>::new();
    let mut bits = Bitset::new(&mut memory);
    assert_eq!(bits.set(5, true), Ok(()));
    assert!(bits.contains(5));
    assert_eq!(bits.set(5, false), Ok(()));
    assert!(!bits.contains(5));
    assert_eq!(bits.toggle(9), Ok(true));
    assert_eq!(bits.toggle(9), Ok(false));
    assert!(bits.is_empty());
}

#[test]
fn fill_clear_and_first() {
    let mut memory = Memory::<u64, 2>::new();
    let mut bits = Bitset::new(&mut memory);
    bits.fill();
    assert_eq!(bits.count(), 128);
    assert!(bits.is_full());
    assert_eq!(bits.first_clear(), None);
    assert_eq!(bits.first_set(), Some(0));
    assert!(bits.remove(70));
    assert_eq!(bits.first_clear(), Some(70));
    assert!(!bits.is_full());
    bits.clear();
    assert!(bits.is_empty());
    assert_eq!(bits.insert(100), Ok(true));
    assert_eq!(bits.first_set(), Some(100));
}

#[test]
fn iterates_set_bits_in_order() {
    let mut memory = Memory::<u64, 3>::new();
    let mut bits = Bitset::new(&mut memory);
    for index in [190, 0, 63, 64, 5, 128] {
        assert_eq!(bits.insert(index), Ok(true));
    }
    assert!(bits.iter().eq([0, 5, 63, 64, 128, 190]));
    assert!((&bits).into_iter().eq([0, 5, 63, 64, 128, 190]));
}

#[test]
fn set_operations() {
    let mut memory_a = Memory::<u64, 2>::new();
    let mut memory_b = Memory::<u64, 2>::new();
    let mut a = Bitset::new(&mut memory_a);
    let mut b = Bitset::new(&mut memory_b);
    for index in [1, 2, 3, 100] {
        assert_eq!(a.insert(index), Ok(true));
    }
    for index in [2, 3, 4, 101] {
        assert_eq!(b.insert(index), Ok(true));
    }
    a.union_with(&b);
    assert!(a.iter().eq([1, 2, 3, 4, 100, 101]));
    a.intersect_with(&b);
    assert!(a.iter().eq([2, 3, 4, 101]));
    assert!(a.remove(4));
    b.difference_with(&a);
    assert!(b.iter().eq([4]));
}

#[test]
fn set_operations_with_different_sizes() {
    let mut memory_small = Memory::<u64, 1>::new();
    let mut memory_big = Memory::<u64, 2>::new();
    let mut small = Bitset::new(&mut memory_small);
    let mut big = Bitset::new(&mut memory_big);
    assert_eq!(small.insert(1), Ok(true));
    assert_eq!(big.insert(1), Ok(true));
    assert_eq!(big.insert(2), Ok(true));
    assert_eq!(big.insert(100), Ok(true));
    // Bits beyond the smaller set do not exist in it.
    small.union_with(&big);
    assert!(small.iter().eq([1, 2]));
    big.intersect_with(&small);
    assert!(big.iter().eq([1, 2]));
}

#[test]
fn zero_capacity() {
    let mut memory = Memory::<u64, 0>::new();
    let mut bits = Bitset::new(&mut memory);
    assert_eq!(bits.capacity(), 0);
    assert_eq!(bits.insert(0), Err(OutOfBounds));
    assert!(bits.is_empty());
    assert!(bits.is_full());
    assert_eq!(bits.first_clear(), None);
}

#[test]
fn words_helper_sizes_memory() {
    assert_eq!(words(0), 0);
    assert_eq!(words(1), 1);
    assert_eq!(words(64), 1);
    assert_eq!(words(65), 2);
    let mut memory = Memory::<u64, { words(1000) }>::new();
    let bits = Bitset::new(&mut memory);
    assert!(bits.capacity() >= 1000);
    assert_eq!(bits.as_words().len(), 16);
}

#[test]
fn matches_a_bool_array() {
    let mut memory = Memory::<u64, 4>::new();
    let mut bits = Bitset::new(&mut memory);
    let mut model = [false; 256];
    let mut seed: u32 = 0x0bad_cafe;
    let steps: u32 = if cfg!(miri) { 500 } else { 10_000 };
    for _ in 0..steps {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let index = ((seed >> 16) & 255) as usize;
        if let Some(flag) = model.get_mut(index) {
            if (seed >> 8) & 1 == 0 {
                assert_eq!(bits.insert(index), Ok(!*flag));
                *flag = true;
            } else {
                assert_eq!(bits.remove(index), *flag);
                *flag = false;
            }
        }
    }
    assert_eq!(bits.count(), model.iter().filter(|flag| **flag).count());
    for (index, flag) in model.iter().enumerate() {
        assert_eq!(bits.contains(index), *flag);
    }
    assert_eq!(bits.first_set(), model.iter().position(|flag| *flag));
    assert_eq!(bits.first_clear(), model.iter().position(|flag| !*flag));
}
