//! A small, fast, non-cryptographic hasher.

use core::hash::{BuildHasherDefault, Hasher};

/// Odd multiplier with well-mixed bits.
const K: u64 = 0xf135_7aea_2e62_a9c5;

/// A fast multiplicative hasher for in-memory tables.
///
/// It is deterministic (no random seed) and **not** resistant to deliberately
/// crafted colliding keys. Do not use it for keys that come from untrusted
/// input; plug a different [`BuildHasher`](core::hash::BuildHasher) into the
/// container instead.
#[derive(Clone, Copy, Default)]
pub struct FastHasher {
    state: u64,
}

impl FastHasher {
    #[inline]
    const fn add(&mut self, word: u64) {
        self.state = self.state.wrapping_add(word).wrapping_mul(K);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn finish(&self) -> u64 {
        // Fold the high half down and multiply once more, so both the high
        // and the low bits of the result depend on every input bit.
        (self.state ^ (self.state >> 32)).wrapping_mul(K)
    }

    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while let Some((head, rest)) = bytes.split_first_chunk::<8>() {
            self.add(u64::from_le_bytes(*head));
            bytes = rest;
        }
        if !bytes.is_empty() {
            let mut word = 0u64;
            for (i, byte) in bytes.iter().enumerate() {
                word |= (*byte as u64) << (8 * i);
            }
            self.add(word);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.add(i as u64);
        self.add((i >> 64) as u64);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
}

/// Builds [`FastHasher`]s. The default hasher of vas containers.
pub type FastState = BuildHasherDefault<FastHasher>;
