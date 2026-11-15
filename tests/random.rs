use vas::Rng;

#[test]
fn matches_xoshiro256plusplus_reference_output() {
    // Reference vector for xoshiro256++ with state [1, 2, 3, 4].
    let mut rng = Rng::from_state([1, 2, 3, 4]);
    let expected: [u64; 10] = [
        41943041,
        58720359,
        3588806011781223,
        3591011842654386,
        9228616714210784205,
        9973669472204895162,
        14011001112246962877,
        12406186145184390807,
        15849039046786891736,
        10450023813501588000,
    ];
    for value in expected {
        assert_eq!(rng.u64(), value);
    }
}

#[test]
fn seed_is_expanded_with_splitmix64() {
    // SplitMix64 from seed 0 yields these four words as the initial state.
    let rng = Rng::new(0);
    assert_eq!(
        rng.state(),
        [
            0xe220_a839_7b1d_cdaf,
            0x6e78_9e6a_a1b9_65f4,
            0x06c4_5d18_8009_454f,
            0xf88b_b8a8_724c_81ec,
        ]
    );
    let mut rng = Rng::new(42);
    assert_eq!(rng.u64(), 15021278609987233951);
    assert_eq!(rng.u64(), 5881210131331364753);
    assert_eq!(rng.u64(), 18149643915985481100);
}

#[test]
fn same_seed_same_sequence() {
    let mut a = Rng::new(7);
    let mut b = Rng::new(7);
    let mut c = Rng::new(8);
    let mut differs = false;
    for _ in 0..100 {
        let value = a.u64();
        assert_eq!(value, b.u64());
        differs |= value != c.u64();
    }
    assert!(differs);
}

#[test]
fn state_round_trips() {
    let mut rng = Rng::new(99);
    for _ in 0..10 {
        rng.u64();
    }
    let mut restored = Rng::from_state(rng.state());
    assert_eq!(rng, restored);
    for _ in 0..10 {
        assert_eq!(rng.u64(), restored.u64());
    }
}

#[test]
fn all_zero_state_is_replaced() {
    let mut rng = Rng::from_state([0; 4]);
    assert_ne!(rng.state(), [0; 4]);
    let first = rng.u64();
    assert!(first != 0 || rng.u64() != 0);
}

#[test]
fn floats_are_in_unit_interval() {
    let mut rng = Rng::new(1);
    let mut low = false;
    let mut high = false;
    for _ in 0..10_000 {
        let x = rng.f32();
        assert!((0.0..1.0).contains(&x));
        let y = rng.f64();
        assert!((0.0..1.0).contains(&y));
        low |= x < 0.1;
        high |= x > 0.9;
    }
    assert!(low && high);
}

#[test]
fn below_stays_in_range_and_handles_zero() {
    let mut rng = Rng::new(2);
    assert_eq!(rng.u32_below(0), 0);
    assert_eq!(rng.u64_below(0), 0);
    assert_eq!(rng.usize_below(0), 0);
    assert_eq!(rng.u32_below(1), 0);
    for _ in 0..10_000 {
        assert!(rng.u32_below(6) < 6);
        assert!(rng.u64_below(1_000_000_007) < 1_000_000_007);
        assert!(rng.usize_below(3) < 3);
        assert!(rng.u32_below(u32::MAX) < u32::MAX);
    }
}

#[test]
fn below_is_roughly_uniform() {
    let mut rng = Rng::new(3);
    let mut buckets = [0u32; 6];
    for _ in 0..60_000 {
        if let Some(bucket) = buckets.get_mut(rng.u32_below(6) as usize) {
            *bucket += 1;
        }
    }
    for count in buckets {
        // Expected 10,000 each; this is many standard deviations wide.
        assert!((9_500..10_500).contains(&count), "count {count}");
    }
}

#[test]
fn ranges() {
    let mut rng = Rng::new(4);
    let mut seen_min = false;
    let mut seen_max = false;
    for _ in 0..10_000 {
        let i = rng.i32_range(-3, 3);
        assert!((-3..3).contains(&i));
        seen_min |= i == -3;
        seen_max |= i == 2;
        let f = rng.f32_range(-2.0, 5.0);
        assert!((-2.0..5.0).contains(&f));
    }
    assert!(seen_min && seen_max);
    // Empty or inverted ranges return the lower bound.
    assert_eq!(rng.i32_range(5, 5), 5);
    assert_eq!(rng.i32_range(5, 1), 5);
    assert_eq!(rng.f32_range(2.0, 2.0), 2.0);
    // The full i32 span does not overflow.
    let wide = rng.i32_range(i32::MIN, i32::MAX);
    assert!(wide < i32::MAX);
}

#[test]
fn chance_and_bool() {
    let mut rng = Rng::new(5);
    let mut heads = 0;
    let mut hits = 0;
    for _ in 0..10_000 {
        assert!(!rng.chance(0.0));
        assert!(rng.chance(1.0));
        heads += u32::from(rng.bool());
        hits += u32::from(rng.chance(0.25));
    }
    assert!((4_700..5_300).contains(&heads), "heads {heads}");
    assert!((2_200..2_800).contains(&hits), "hits {hits}");
}

#[test]
fn pick_and_shuffle() {
    let mut rng = Rng::new(6);
    let empty: [u32; 0] = [];
    assert_eq!(rng.pick(&empty), None);
    assert_eq!(rng.pick(&[9]), Some(&9));
    let items = [1, 2, 3, 4];
    for _ in 0..100 {
        assert!(rng.pick(&items).is_some_and(|item| items.contains(item)));
    }

    let mut deck: [u32; 52] = core::array::from_fn(|i| i as u32);
    rng.shuffle(&mut deck);
    assert!(deck.iter().enumerate().any(|(i, card)| *card != i as u32));
    let mut sorted = deck;
    sorted.sort_unstable();
    assert!(sorted.iter().enumerate().all(|(i, card)| *card == i as u32));
    // Shuffling nothing or one element is fine.
    rng.shuffle(&mut [0u8; 0]);
    rng.shuffle(&mut [1u8]);
}

#[test]
fn fork_gives_an_independent_stream() {
    let mut parent = Rng::new(10);
    let mut child = parent.fork();
    assert_ne!(parent, child);
    let mut same = 0;
    for _ in 0..100 {
        same += u32::from(parent.u64() == child.u64());
    }
    assert_eq!(same, 0);
}
