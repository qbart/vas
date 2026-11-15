//! A small, fast, seedable pseudo-random number generator.

use core::mem;

/// A deterministic pseudo-random number generator (xoshiro256++).
///
/// The same seed always produces the same sequence, on every platform, so
/// replays, tests and procedural generation are reproducible. It holds 32
/// bytes of state and needs no storage.
///
/// **Not** cryptographically secure: do not use it for secrets, tokens or
/// anything an adversary must not predict.
///
/// ```
/// use vas::Rng;
///
/// let mut rng = Rng::new(42);
/// let roll = rng.u32_below(6) + 1; // 1..=6
/// assert!((1..=6).contains(&roll));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    // Invariant: never all zero (the generator would then only output zero).
    state: [u64; 4],
}

impl Rng {
    /// Creates a generator from a 64-bit seed. Any seed is fine, including
    /// zero; similar seeds give unrelated sequences.
    #[inline]
    pub const fn new(seed: u64) -> Self {
        // Expand the seed with SplitMix64, as the xoshiro authors recommend.
        let mut x = seed;
        let mut state = [0u64; 4];
        let mut i = 0;
        while i < 4 {
            x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            state[i] = z ^ (z >> 31);
            i += 1;
        }
        Self::from_state(state)
    }

    /// Restores a generator from a state obtained with [`state`](Self::state),
    /// for example from a save file. An all-zero state is invalid and is
    /// replaced by the state of `Rng::new(0)`.
    #[inline]
    pub const fn from_state(state: [u64; 4]) -> Self {
        if state[0] == 0 && state[1] == 0 && state[2] == 0 && state[3] == 0 {
            return Self::new(0);
        }
        Self { state }
    }

    /// The current internal state. Feeding it to
    /// [`from_state`](Self::from_state) continues the sequence exactly.
    #[inline]
    pub const fn state(&self) -> [u64; 4] {
        self.state
    }

    /// Creates a new generator seeded from this one. Use it to give a
    /// subsystem its own stream without sharing this generator.
    #[inline]
    pub const fn fork(&mut self) -> Self {
        Self::new(self.u64())
    }

    /// A uniformly random `u64`.
    #[inline]
    pub const fn u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = self.state;
        let result = s0.wrapping_add(s3).rotate_left(23).wrapping_add(s0);
        let t = s1 << 17;
        let s2 = s2 ^ s0;
        let s3 = s3 ^ s1;
        let s1 = s1 ^ s2;
        let s0 = s0 ^ s3;
        let s2 = s2 ^ t;
        let s3 = s3.rotate_left(45);
        self.state = [s0, s1, s2, s3];
        result
    }

    /// A uniformly random `u32`.
    #[inline]
    pub const fn u32(&mut self) -> u32 {
        (self.u64() >> 32) as u32
    }

    /// `true` or `false` with equal probability.
    #[inline]
    pub const fn bool(&mut self) -> bool {
        self.u64() >> 63 == 1
    }

    /// A uniformly random `f32` in `0.0..1.0` (1.0 is never returned).
    #[inline]
    pub const fn f32(&mut self) -> f32 {
        // 24 random bits, the precision of an f32 mantissa.
        (self.u64() >> 40) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// A uniformly random `f64` in `0.0..1.0` (1.0 is never returned).
    #[inline]
    pub const fn f64(&mut self) -> f64 {
        // 53 random bits, the precision of an f64 mantissa.
        (self.u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// `true` with probability `probability` (0.0 = never, 1.0 = always).
    #[inline]
    pub fn chance(&mut self, probability: f32) -> bool {
        self.f32() < probability
    }

    /// A uniformly random `u32` in `0..bound`, without bias. Returns 0 when
    /// `bound` is 0.
    #[inline]
    pub const fn u32_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        // Lemire's multiply-shift method with rejection of the biased zone.
        let mut product = self.u32() as u64 * bound as u64;
        if (product as u32) < bound {
            let threshold = bound.wrapping_neg() % bound;
            while (product as u32) < threshold {
                product = self.u32() as u64 * bound as u64;
            }
        }
        (product >> 32) as u32
    }

    /// A uniformly random `u64` in `0..bound`, without bias. Returns 0 when
    /// `bound` is 0.
    #[inline]
    pub const fn u64_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        let mut product = self.u64() as u128 * bound as u128;
        if (product as u64) < bound {
            let threshold = bound.wrapping_neg() % bound;
            while (product as u64) < threshold {
                product = self.u64() as u128 * bound as u128;
            }
        }
        (product >> 64) as u64
    }

    /// A uniformly random `usize` in `0..bound`, without bias. Returns 0
    /// when `bound` is 0.
    #[inline]
    pub const fn usize_below(&mut self, bound: usize) -> usize {
        // The result is below `bound`, so it always fits back into `usize`.
        self.u64_below(bound as u64) as usize
    }

    /// A uniformly random `i32` in `min..max` (`max` excluded). Returns
    /// `min` when the range is empty.
    #[inline]
    pub const fn i32_range(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        let span = max.wrapping_sub(min) as u32;
        min.wrapping_add(self.u32_below(span) as i32)
    }

    /// A uniformly random `f32` in `min..max` (`max` excluded). Returns
    /// `min` when the range is empty.
    #[inline]
    pub fn f32_range(&mut self, min: f32, max: f32) -> f32 {
        if max > min {
            let value = min + (max - min) * self.f32();
            // Rounding can land exactly on `max`; keep it excluded.
            if value < max { value } else { min }
        } else {
            min
        }
    }

    /// A uniformly random element of `items`, or `None` when it is empty.
    #[inline]
    pub fn pick<'s, T>(&mut self, items: &'s [T]) -> Option<&'s T> {
        items.get(self.usize_below(items.len()))
    }

    /// Shuffles `items` in place so that every order is equally likely
    /// (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.usize_below(i + 1);
            if let Ok([a, b]) = items.get_disjoint_mut([i, j]) {
                mem::swap(a, b);
            }
        }
    }
}
