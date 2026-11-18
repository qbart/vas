use core::cell::Cell;
use core::cmp::Ordering;
use vas::{Inline, Memory, Rng, Vector, sort};

/// Counts drops, to check that no element is lost or duplicated.
struct Tracked<'a>(u32, &'a Cell<u32>);

impl Drop for Tracked<'_> {
    fn drop(&mut self) {
        self.1.set(self.1.get() + 1);
    }
}

fn random(count: usize, below: u32, seed: u64) -> Vec<u32> {
    let mut rng = Rng::new(seed);
    (0..count).map(|_| rng.u32_below(below)).collect()
}

/// Inputs with the shapes that trouble quicksorts.
fn patterns(count: usize) -> Vec<(&'static str, Vec<u32>)> {
    let n = count as u32;
    vec![
        ("random", random(count, u32::MAX, count as u64)),
        ("few keys", random(count, 4, count as u64 + 1)),
        ("equal", vec![7; count]),
        ("sorted", (0..n).collect()),
        ("reversed", (0..n).rev().collect()),
        ("sorted, last moved", {
            let mut v: Vec<u32> = (1..=n).collect();
            if let Some(last) = v.last_mut() {
                *last = 0;
            }
            v
        }),
        ("saw", (0..n).map(|i| i % 16).collect()),
        ("organ pipe", (0..n).map(|i| i.min(n - i)).collect()),
        ("pairs swapped", (0..n).map(|i| i ^ 1).collect()),
        ("two runs", (0..n).map(|i| (i + n / 2) % n.max(1)).collect()),
    ]
}

/// Sizes above this are skipped under Miri, which is slow.
fn sizes() -> &'static [usize] {
    if cfg!(miri) {
        &[0, 1, 2, 3, 15, 16, 17, 33, 64, 65, 200]
    } else {
        &[0, 1, 2, 3, 15, 16, 17, 33, 64, 65, 200, 1000, 4097, 50_000]
    }
}

#[test]
fn unstable_sorts_every_pattern() {
    for &count in sizes() {
        for (name, input) in patterns(count) {
            let mut expected = input.clone();
            expected.sort();
            let mut v = input.clone();
            sort::unstable(&mut v);
            assert_eq!(v, expected, "{name}, {count}");
            let mut v = input.clone();
            sort::unstable_by(&mut v, |a, b| b.cmp(a));
            expected.reverse();
            assert_eq!(v, expected, "{name} reversed, {count}");
        }
    }
}

#[test]
fn unstable_by_key_on_pairs() {
    for &count in sizes() {
        let keys = random(count, 50, 3);
        let mut v: Vec<(u32, u32)> = keys.iter().copied().zip(0..).collect();
        sort::unstable_by_key(&mut v, |pair| pair.0);
        assert!(v.windows(2).all(|w| w[0].0 <= w[1].0), "{count}");
        // Same elements, possibly in another order among equal keys.
        let mut ids: Vec<u32> = v.iter().map(|pair| pair.1).collect();
        ids.sort();
        assert!(ids.iter().copied().eq(0..count as u32));
    }
}

#[test]
fn vector_methods_are_the_vas_sorts() {
    let mut v = Vector::<i32, Inline<16>>::inline();
    assert!(v.extend_from_slice(&[5, -3, 9, 0, -3, 2, 8, 1]).is_ok());
    v.sort_unstable();
    assert_eq!(v.as_slice(), &[-3, -3, 0, 1, 2, 5, 8, 9]);
    v.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(v.as_slice(), &[9, 8, 5, 2, 1, 0, -3, -3]);
    v.sort_unstable_by_key(|x| x.abs());
    assert_eq!(v.as_slice(), &[0, 1, 2, -3, -3, 5, 8, 9]);
}

#[test]
fn stable_functions_on_slices() {
    let input = [(3, 'a'), (1, 'b'), (3, 'c'), (0, 'd'), (1, 'e')];
    let mut v = input;
    sort::stable_by_key(&mut v, |pair| pair.0);
    assert_eq!(v, [(0, 'd'), (1, 'b'), (1, 'e'), (3, 'a'), (3, 'c')]);
    let mut v = input;
    sort::stable_by(&mut v, |a, b| b.0.cmp(&a.0));
    assert_eq!(v, [(3, 'a'), (3, 'c'), (1, 'b'), (1, 'e'), (0, 'd')]);
    let mut v = input;
    let mut scratch = Memory::<(i32, char), 2>::new();
    sort::stable_in(&mut v, &mut scratch, |a, b| a.0.cmp(&b.0));
    assert_eq!(v, [(0, 'd'), (1, 'b'), (1, 'e'), (3, 'a'), (3, 'c')]);
    let mut v = [3, 1, 2];
    sort::stable(&mut v);
    assert_eq!(v, [1, 2, 3]);
}

#[test]
fn zero_sized_elements() {
    let mut v = [(); 100];
    sort::unstable(&mut v);
    sort::stable(&mut v);
    sort::unstable_by(&mut v, |_, _| Ordering::Less);
    assert_eq!(v.len(), 100);
}

#[test]
fn large_elements_take_the_insertion_path() {
    // Larger than 16 bytes: the leaves are sorted by insertion.
    for &count in sizes() {
        let input = random(count, 100, 8);
        let mut v: Vec<[u64; 4]> = input.iter().map(|k| [u64::from(*k), 0, 0, 1]).collect();
        sort::unstable(&mut v);
        assert!(v.windows(2).all(|w| w[0] <= w[1]), "{count}");
        let mut v: Vec<([u64; 3], u32)> = input
            .iter()
            .copied()
            .map(|k| ([u64::from(k); 3], k))
            .zip(0..)
            .map(|((key, _), id)| (key, id))
            .collect();
        sort::stable_by_key(&mut v, |pair| pair.0);
        assert!(
            v.windows(2)
                .all(|w| w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1)),
            "{count}"
        );
    }
}

#[test]
fn works_on_a_deque_made_contiguous() {
    let mut d = vas::Deque::<u32, Inline<6>>::inline();
    for i in [4, 9, 1, 7] {
        assert!(d.push_back(i).is_ok());
    }
    let _ = d.pop_front();
    assert!(d.push_back(3).is_ok());
    assert!(d.push_back(0).is_ok());
    assert!(d.push_back(5).is_ok());
    sort::unstable(d.make_contiguous());
    assert!(d.iter().copied().eq([0, 1, 3, 5, 7, 9]));
}

/// The order of equal elements depends only on the input and on vas, not
/// on the compiler or the target. This pins it: if the sort changes so that
/// equal elements come out in another order, this fails and the change must
/// be called out in the release notes.
#[test]
fn unstable_order_of_equal_elements_is_pinned() {
    let count = if cfg!(miri) { 300 } else { 5000 };
    // About four elements per key: the leaves and the partitions both
    // decide where equal elements go.
    let keys = random(count, count as u32 / 4, 99);
    let mut v: Vec<(u32, u32)> = keys.iter().copied().zip(0..).collect();
    sort::unstable_by_key(&mut v, |pair| pair.0);
    // FNV-1a over the ids in their sorted order.
    let digest = v.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, pair| {
        (hash ^ u64::from(pair.1)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let expected = if cfg!(miri) {
        0xb997_50eb_23f8_e437
    } else {
        0x3aca_8905_a92d_02cf
    };
    assert_eq!(digest, expected, "{digest:#x}");
}

/// A comparison that is not a total order (it answers at random) must not
/// make the sort panic, loop forever or lose elements.
#[test]
fn inconsistent_comparison_keeps_every_element() {
    for &count in sizes() {
        let input = random(count, 1000, 5);
        let mut expected = input.clone();
        expected.sort();
        let mut rng = Rng::new(17);
        let mut v = input.clone();
        sort::unstable_by(&mut v, |_, _| match rng.u32_below(3) {
            0 => Ordering::Less,
            1 => Ordering::Equal,
            _ => Ordering::Greater,
        });
        v.sort();
        assert_eq!(v, expected, "unstable, {count}");
        let mut v = input.clone();
        sort::stable_by(&mut v, |_, _| {
            if rng.bool() {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        });
        v.sort();
        assert_eq!(v, expected, "stable, {count}");
        // NaN keys: `partial_cmp` is not a total order.
        let mut floats: Vec<f32> = input
            .iter()
            .map(|x| if x % 7 == 0 { f32::NAN } else { *x as f32 })
            .collect();
        sort::unstable_by(&mut floats, |a, b| {
            a.partial_cmp(b).unwrap_or(Ordering::Equal)
        });
        assert_eq!(floats.len(), count);
    }
}

#[test]
fn unstable_keeps_every_element_when_compare_panics() {
    let drops = Cell::new(0);
    let mut expected = random(300, 1000, 11);
    expected.sort_unstable();
    let mut panicked = 0;
    let step = if cfg!(miri) { 397 } else { 37 };
    for limit in (1..4000).step_by(step) {
        drops.set(0);
        let mut v = Vector::<Tracked<'_>, Inline<300>>::inline();
        for key in random(300, 1000, 11) {
            assert!(v.push(Tracked(key, &drops)).is_ok());
        }
        let mut calls = 0;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            v.sort_unstable_by(|a, b| {
                calls += 1;
                assert!(calls < limit, "comparison failed");
                a.0.cmp(&b.0)
            });
        }));
        panicked += usize::from(result.is_err());
        assert_eq!(drops.get(), 0);
        let mut keys: Vec<u32> = v.iter().map(|t| t.0).collect();
        keys.sort_unstable();
        assert_eq!(keys, expected);
        drop(v);
        assert_eq!(drops.get(), 300);
    }
    assert!(panicked > if cfg!(miri) { 5 } else { 50 });
}

/// O(n log n) comparisons in the worst case too: count them on the inputs
/// that send a plain quicksort quadratic.
#[test]
fn unstable_comparisons_stay_n_log_n() {
    let count: usize = if cfg!(miri) { 2000 } else { 100_000 };
    let bound = 3 * count * count.ilog2() as usize;
    for (name, input) in patterns(count) {
        let mut calls = 0;
        let mut v = input;
        sort::unstable_by(&mut v, |a, b| {
            calls += 1;
            a.cmp(b)
        });
        assert!(calls <= bound, "{name}: {calls} comparisons");
    }
    // Median-of-three killer: every pivot is the second smallest.
    let n = count as u32;
    let killer: Vec<u32> = (0..n)
        .map(|i| if i % 2 == 0 { i / 2 } else { n / 2 + i / 2 })
        .collect();
    let mut calls = 0;
    let mut v = killer;
    sort::unstable_by(&mut v, |a, b| {
        calls += 1;
        a.cmp(b)
    });
    assert!(v.windows(2).all(|w| w[0] <= w[1]));
    assert!(calls <= bound, "killer: {calls} comparisons");
}

#[test]
fn sorted_input_is_linear() {
    let count = if cfg!(miri) { 1000 } else { 100_000 };
    for input in [
        (0..count).collect::<Vec<u32>>(),
        (0..count).rev().collect(),
        vec![1; count as usize],
    ] {
        let mut calls = 0;
        let mut v = input;
        sort::unstable_by(&mut v, |a, b| {
            calls += 1;
            a.cmp(b)
        });
        assert!(calls < count as usize, "{calls}");
        assert!(v.windows(2).all(|w| w[0] <= w[1]));
    }
}
