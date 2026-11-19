use core::cell::Cell;
use std::error::Error;
use std::vec::Vec;
use vas::{Inline, Rng, Vector, search, sort};

type Test = Result<(), Box<dyn Error>>;

#[test]
fn binary_search_finds_the_first_match() {
    let items = [1, 3, 3, 3, 5, 8, 8];
    assert_eq!(search::binary_search(&items, &3), Ok(1));
    assert_eq!(search::binary_search(&items, &8), Ok(5));
    assert_eq!(search::binary_search(&items, &1), Ok(0));
    assert_eq!(search::binary_search(&items, &0), Err(0));
    assert_eq!(search::binary_search(&items, &4), Err(4));
    assert_eq!(search::binary_search(&items, &9), Err(7));
    assert_eq!(search::binary_search::<u8>(&[], &1), Err(0));
    let pairs = [(1, 'a'), (2, 'b'), (2, 'c'), (4, 'd')];
    assert_eq!(search::binary_search_by_key(&pairs, &2, |p| p.0), Ok(1));
    assert_eq!(search::binary_search_by(&pairs, |p| p.0.cmp(&3)), Err(3));
    assert_eq!(search::partition_point(&items, |x| *x < 5), 4);
    assert_eq!(search::partition_point(&items, |_| true), 7);
    assert_eq!(search::partition_point(&items, |_| false), 0);
}

#[test]
fn binary_search_matches_a_linear_scan() {
    let mut rng = Rng::new(41);
    let rounds = if cfg!(miri) { 30 } else { 2000 };
    for _ in 0..rounds {
        let len = rng.usize_below(70);
        let mut items: Vec<u32> = (0..len).map(|_| rng.u32_below(20)).collect();
        items.sort_unstable();
        for target in 0..21 {
            let first = items.iter().position(|x| *x >= target).unwrap_or(len);
            let expected = if items.get(first) == Some(&target) {
                Ok(first)
            } else {
                Err(first)
            };
            assert_eq!(search::binary_search(&items, &target), expected);
        }
    }
}

#[test]
fn binary_search_never_panics_on_bad_order() {
    let mut rng = Rng::new(2);
    let items: Vec<u32> = (0..100).map(|_| rng.u32_below(50)).collect();
    for target in 0..60 {
        let (Ok(index) | Err(index)) = search::binary_search(&items, &target);
        assert!(index <= items.len());
        let flips = Cell::new(0u32);
        let found = search::binary_search_by(&items, |_| {
            flips.set(flips.get() + 1);
            [core::cmp::Ordering::Less, core::cmp::Ordering::Greater][flips.get() as usize % 2]
        });
        let (Ok(index) | Err(index)) = found;
        assert!(index <= items.len());
    }
}

#[test]
fn select_puts_the_nth_in_place() -> Test {
    let mut rng = Rng::new(9);
    let rounds = if cfg!(miri) { 20 } else { 600 };
    for round in 0..rounds {
        let len = 1 + rng.usize_below(if round % 10 == 0 { 3000 } else { 80 });
        let keys = 1 + rng.u32_below(if round % 3 == 0 { 4 } else { 1000 });
        let mut items: Vec<u32> = (0..len).map(|_| rng.u32_below(keys)).collect();
        let mut sorted = items.clone();
        sorted.sort_unstable();
        let index = rng.usize_below(len);
        let (left, nth, right) = sort::select_nth_unstable(&mut items, index).ok_or("in range")?;
        assert_eq!(*nth, sorted[index]);
        assert!(left.iter().all(|x| x <= nth));
        assert!(right.iter().all(|x| x >= nth));
        items.sort_unstable();
        assert_eq!(items, sorted, "same elements");
    }
    Ok(())
}

#[test]
fn select_out_of_range_is_none() {
    let mut items = [3, 1, 2];
    assert!(sort::select_nth_unstable(&mut items, 3).is_none());
    assert_eq!(items, [3, 1, 2]);
    assert!(sort::select_nth_unstable::<u8>(&mut [], 0).is_none());
    let mut units = [(), (), ()];
    assert!(sort::select_nth_unstable(&mut units, 2).is_some());
}

#[test]
fn select_by_key_and_by() {
    let mut units = [(5, "e"), (1, "a"), (4, "d"), (2, "b"), (3, "c")];
    let found = sort::select_nth_unstable_by_key(&mut units, 1, |u| u.0).map(|(_, n, _)| *n);
    assert_eq!(found, Some((2, "b")));
    let found =
        sort::select_nth_unstable_by(&mut units, 0, |a, b| b.0.cmp(&a.0)).map(|(_, n, _)| *n);
    assert_eq!(found, Some((5, "e")));
}

/// Inputs built to defeat a median-of-three pivot, and runs of equal keys,
/// still take O(n) comparisons.
#[test]
fn select_comparisons_stay_linear() {
    let n = if cfg!(miri) { 400 } else { 100_000 };
    let mut killer: Vec<u32> = (0..n as u32).collect();
    // Organ pipe and sawtooth patterns.
    let patterns: [Vec<u32>; 4] = [
        killer.clone(),
        (0..n as u32).map(|i| i.min(n as u32 - i)).collect(),
        (0..n as u32).map(|i| i % 64).collect(),
        vec![7; n],
    ];
    killer.reverse();
    for mut items in patterns.into_iter().chain([killer]) {
        for index in [0, n / 3, n / 2, n - 1] {
            let comparisons = Cell::new(0u64);
            let mut copy = items.clone();
            let found = sort::select_nth_unstable_by(&mut copy, index, |a, b| {
                comparisons.set(comparisons.get() + 1);
                a.cmp(b)
            });
            assert!(found.is_some());
            assert!(comparisons.get() < 30 * n as u64, "{}", comparisons.get());
        }
        items.clear();
    }
}

#[test]
fn select_keeps_every_element_when_compare_panics() {
    let mut rng = Rng::new(4);
    let mut items: Vec<std::string::String> =
        (0..500).map(|_| rng.u32_below(100).to_string()).collect();
    let mut expected = items.clone();
    expected.sort_unstable();
    let calls = Cell::new(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sort::select_nth_unstable_by(&mut items, 250, |a, b| {
            calls.set(calls.get() + 1);
            assert!(calls.get() < 700, "stop");
            a.cmp(b)
        });
    }));
    assert!(result.is_err());
    items.sort_unstable();
    assert_eq!(items, expected);
}

#[test]
fn vector_methods_are_the_vas_ones() -> Test {
    let mut v = Vector::<u32, Inline<16>>::inline();
    v.extend_from_slice(&[9, 4, 4, 7, 1, 4])?;
    let median = v.select_nth_unstable(3).map(|(_, n, _)| *n);
    assert_eq!(median, Some(4));
    assert!(v.select_nth_unstable(6).is_none());
    v.sort_unstable();
    assert_eq!(v.binary_search(&4), Ok(1));
    assert_eq!(v.binary_search_by(|x| x.cmp(&8)), Err(5));
    assert_eq!(v.binary_search_by_key(&18, |x| x * 2), Ok(5));
    assert_eq!(v.partition_point(|x| *x < 7), 4);
    let top = v
        .select_nth_unstable_by(0, |a, b| b.cmp(a))
        .map(|(_, n, _)| *n);
    assert_eq!(top, Some(9));
    let low = v.select_nth_unstable_by_key(0, |x| *x).map(|(_, n, _)| *n);
    assert_eq!(low, Some(1));
    Ok(())
}

/// McIlroy's adversary ("A killer adversary for quicksort"): it decides the
/// values only as they are compared, always against the pivot candidate.
/// Selection must still take O(n) comparisons.
#[test]
fn select_survives_an_adversary() {
    use core::cell::RefCell;
    let n: usize = if cfg!(miri) { 300 } else { 50_000 };
    for index in [0, n / 2, n - 1] {
        let gas = n;
        let state = RefCell::new((vec![gas; n], 0usize, 0usize)); // values, solid count, candidate
        let comparisons = Cell::new(0u64);
        let mut items: Vec<usize> = (0..n).collect();
        let found = sort::select_nth_unstable_by(&mut items, index, |&x, &y| {
            comparisons.set(comparisons.get() + 1);
            let mut guard = state.borrow_mut();
            let (values, solid, candidate) = &mut *guard;
            if values[x] == gas && values[y] == gas {
                let frozen = if x == *candidate { x } else { y };
                values[frozen] = *solid;
                *solid += 1;
            }
            if values[x] == gas {
                *candidate = x;
            } else if values[y] == gas {
                *candidate = y;
            }
            values[x].cmp(&values[y])
        });
        let nth = found.map(|(_, nth, _)| *nth);
        assert!(nth.is_some());
        assert!(
            comparisons.get() < 25 * n as u64,
            "{} for {n}",
            comparisons.get()
        );
        // The order the adversary settled on is respected.
        let values = state.borrow().0.clone();
        let mut ranks: Vec<usize> = items.iter().map(|&item| values[item]).collect();
        let at = ranks[index];
        assert!(ranks[..index].iter().all(|v| *v <= at));
        assert!(ranks[index + 1..].iter().all(|v| *v >= at));
        ranks.clear();
    }
}
