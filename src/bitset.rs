//! Fixed-capacity set of bits over user-provided memory.

#![allow(unsafe_code)]

use core::iter::Enumerate;
use core::mem::MaybeUninit;
use core::{fmt, slice};

use crate::OutOfBounds;

const BITS: usize = u64::BITS as usize;

/// Number of `u64` words needed to hold `bits` bits. Use it to size the
/// memory: `Memory::<u64, { words(1000) }>`.
#[inline]
pub const fn words(bits: usize) -> usize {
    bits.div_ceil(BITS)
}

/// A fixed-capacity set of bits (equivalently, a set of small integers) that
/// lives in memory provided by the caller, one bit per possible member.
///
/// ```
/// use vas::{Bitset, Memory};
///
/// let mut memory = Memory::<u64, 4>::new(); // 256 bits
/// let mut visited = Bitset::new(&mut memory);
///
/// assert_eq!(visited.insert(42), Ok(true));
/// assert!(visited.contains(42));
/// assert_eq!(visited.count(), 1);
/// ```
pub struct Bitset<'a> {
    words: &'a mut [u64],
}

impl<'a> Bitset<'a> {
    /// Creates a set with every bit clear, using `storage` as its memory.
    ///
    /// The capacity is 64 bits per word. Clears every word, so creation is
    /// O(words).
    pub fn new(storage: &'a mut [MaybeUninit<u64>]) -> Self {
        // Cap the size so a bit index can never overflow `usize`.
        let used = storage.len().min(usize::MAX / BITS);
        let storage = match storage.get_mut(..used) {
            Some(storage) => storage,
            None => &mut [],
        };
        for word in storage.iter_mut() {
            word.write(0);
        }
        // SAFETY: every word was just initialised, and `MaybeUninit<u64>`
        // has the layout of `u64`.
        let words =
            unsafe { slice::from_raw_parts_mut(storage.as_mut_ptr().cast::<u64>(), storage.len()) };
        Self { words }
    }

    /// Number of bits the set has room for.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.words.len() * BITS
    }

    /// Number of bits that are set.
    #[inline]
    pub fn count(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// `true` when no bit is set.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    /// `true` when every bit is set.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.words.iter().all(|word| *word == u64::MAX)
    }

    /// `true` when bit `index` is set. Out-of-range indices are never set.
    #[inline]
    pub fn contains(&self, index: usize) -> bool {
        match self.words.get(index / BITS) {
            Some(word) => word & mask(index) != 0,
            None => false,
        }
    }

    /// Sets bit `index`. Returns `true` when it was clear before.
    #[inline]
    pub fn insert(&mut self, index: usize) -> Result<bool, OutOfBounds> {
        let word = self.words.get_mut(index / BITS).ok_or(OutOfBounds)?;
        let was_clear = *word & mask(index) == 0;
        *word |= mask(index);
        Ok(was_clear)
    }

    /// Clears bit `index`. Returns `true` when it was set before;
    /// out-of-range indices were never set.
    #[inline]
    pub fn remove(&mut self, index: usize) -> bool {
        match self.words.get_mut(index / BITS) {
            Some(word) => {
                let was_set = *word & mask(index) != 0;
                *word &= !mask(index);
                was_set
            }
            None => false,
        }
    }

    /// Sets bit `index` to `value`.
    #[inline]
    pub fn set(&mut self, index: usize, value: bool) -> Result<(), OutOfBounds> {
        let word = self.words.get_mut(index / BITS).ok_or(OutOfBounds)?;
        if value {
            *word |= mask(index);
        } else {
            *word &= !mask(index);
        }
        Ok(())
    }

    /// Flips bit `index` and returns its new value.
    #[inline]
    pub fn toggle(&mut self, index: usize) -> Result<bool, OutOfBounds> {
        let word = self.words.get_mut(index / BITS).ok_or(OutOfBounds)?;
        *word ^= mask(index);
        Ok(*word & mask(index) != 0)
    }

    /// Clears every bit.
    #[inline]
    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    /// Sets every bit.
    #[inline]
    pub fn fill(&mut self) {
        self.words.fill(u64::MAX);
    }

    /// Index of the lowest set bit.
    #[inline]
    pub fn first_set(&self) -> Option<usize> {
        self.words
            .iter()
            .enumerate()
            .find(|(_, word)| **word != 0)
            .map(|(index, word)| index * BITS + word.trailing_zeros() as usize)
    }

    /// Index of the lowest clear bit. Handy for finding a free slot.
    #[inline]
    pub fn first_clear(&self) -> Option<usize> {
        self.words
            .iter()
            .enumerate()
            .find(|(_, word)| **word != u64::MAX)
            .map(|(index, word)| index * BITS + word.trailing_ones() as usize)
    }

    /// Sets every bit that is set in `other`. Bits beyond this set's
    /// capacity are ignored.
    #[inline]
    pub fn union_with(&mut self, other: &Bitset<'_>) {
        for (word, theirs) in self.words.iter_mut().zip(other.words.iter()) {
            *word |= theirs;
        }
    }

    /// Keeps only the bits that are also set in `other`. Bits beyond
    /// `other`'s capacity count as clear.
    #[inline]
    pub fn intersect_with(&mut self, other: &Bitset<'_>) {
        let mut theirs = other.words.iter();
        for word in self.words.iter_mut() {
            *word &= theirs.next().copied().unwrap_or(0);
        }
    }

    /// Clears every bit that is set in `other`.
    #[inline]
    pub fn difference_with(&mut self, other: &Bitset<'_>) {
        for (word, theirs) in self.words.iter_mut().zip(other.words.iter()) {
            *word &= !theirs;
        }
    }

    /// Iterates over the indices of the set bits, lowest first.
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            words: self.words.iter().enumerate(),
            current: 0,
            base: 0,
        }
    }

    /// The underlying words, 64 bits each, lowest bits first.
    #[inline]
    pub const fn as_words(&self) -> &[u64] {
        self.words
    }
}

#[inline]
const fn mask(index: usize) -> u64 {
    1 << (index % BITS)
}

impl fmt::Debug for Bitset<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<'b> IntoIterator for &'b Bitset<'_> {
    type Item = usize;
    type IntoIter = Iter<'b>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over the indices of the set bits of a [`Bitset`]. See
/// [`Bitset::iter`].
pub struct Iter<'b> {
    words: Enumerate<slice::Iter<'b, u64>>,
    // Bits of the current word that have not been yielded yet.
    current: u64,
    // Bit index of the current word's lowest bit.
    base: usize,
}

impl Iterator for Iter<'_> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<usize> {
        loop {
            if self.current != 0 {
                let bit = self.current.trailing_zeros() as usize;
                // Clear the lowest set bit.
                self.current &= self.current - 1;
                return Some(self.base + bit);
            }
            let (index, word) = self.words.next()?;
            self.current = *word;
            self.base = index * BITS;
        }
    }
}
