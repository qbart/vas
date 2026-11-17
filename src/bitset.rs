//! Set of bits over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::iter::Enumerate;
use core::mem::MaybeUninit;
use core::{fmt, slice};

use crate::OutOfBounds;
use crate::allocator::Allocator;
use crate::storage::{Buffer, Dynamic, Inline, IntoBuffer, Storage};

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
///
/// With [`Dynamic`] storage the capacity is not fixed: the
/// memory comes from an allocator and the container grows when it runs out.
pub struct Bitset<S: Storage<u64>> {
    // Invariant: `used <= buffer.slots().len()` and the first `used` words
    // are initialised.
    buffer: S::Buffer,
    used: usize,
}

impl<const N: usize> Bitset<Inline<N>> {
    /// Creates a set of `64 * N` bits, all clear, held inside itself.
    ///
    /// `N` counts 64-bit words; use [`words`] to get it from a number of
    /// bits: `Bitset<Inline<{ words(1000) }>>`. This is `const`, so it can
    /// initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            buffer: [MaybeUninit::new(0); N],
            used: N,
        }
    }
}

impl<const N: usize> Default for Bitset<Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Only bitsets with inline storage can be cloned.
impl<const N: usize> Clone for Bitset<Inline<N>> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer,
            used: self.used,
        }
    }
}

impl<A: Allocator + Default> Default for Bitset<Dynamic<A>> {
    /// Creates an empty set that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

impl<S: Storage<u64>> Bitset<S> {
    /// Creates a set with every bit clear on `buffer`: a reference to
    /// memory borrows it, a `Memory` passed by value is stored inside the
    /// set.
    ///
    /// The capacity is 64 bits per word. Clears every word, so creation is
    /// O(words).
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<u64, Storage = S>,
    {
        let mut buffer = buffer.into_buffer();
        let slots = buffer.slots_mut();
        // Cap the size so a bit index can never overflow `usize`.
        let used = slots.len().min(usize::MAX / BITS);
        if let Some(words) = slots.get_mut(..used) {
            for word in words {
                word.write(0);
            }
        }
        Self { buffer, used }
    }

    #[inline]
    fn words(&self) -> &[u64] {
        // SAFETY: the first `used` words are initialised by the invariant,
        // and `MaybeUninit<u64>` has the layout of `u64`.
        unsafe { slice::from_raw_parts(self.buffer.slots().as_ptr().cast::<u64>(), self.used) }
    }

    #[inline]
    fn words_mut(&mut self) -> &mut [u64] {
        // SAFETY: as in `words`, and `self` is borrowed exclusively.
        unsafe {
            slice::from_raw_parts_mut(
                self.buffer.slots_mut().as_mut_ptr().cast::<u64>(),
                self.used,
            )
        }
    }

    /// The word holding bit `index`. With dynamic storage the set grows to
    /// reach it; with fixed storage an index past the end is `None`.
    #[inline]
    fn word_mut(&mut self, index: usize) -> Option<&mut u64> {
        let word = index / BITS;
        if word >= self.used && !self.grow(word + 1) {
            return None;
        }
        self.words_mut().get_mut(word)
    }

    /// Grows the storage to at least `words` words, all new bits clear.
    /// Returns `false`, with nothing changed, when the storage cannot grow.
    #[cold]
    fn grow(&mut self, words: usize) -> bool {
        if words > usize::MAX / BITS || !self.buffer.grow(words) {
            return false;
        }
        let slots = self.buffer.slots_mut();
        let used = slots.len().min(usize::MAX / BITS);
        if let Some(fresh) = slots.get_mut(self.used..used) {
            for word in fresh {
                word.write(0);
            }
            self.used = used;
        }
        true
    }

    /// Number of bits the set has room for. With dynamic storage, setting
    /// a higher bit grows the set.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.words().len() * BITS
    }

    /// Number of bits that are set.
    #[inline]
    pub fn count(&self) -> usize {
        self.words()
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    /// `true` when no bit is set.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.words().iter().all(|word| *word == 0)
    }

    /// `true` when every bit is set.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.words().iter().all(|word| *word == u64::MAX)
    }

    /// `true` when bit `index` is set. Out-of-range indices are never set.
    #[inline]
    pub fn contains(&self, index: usize) -> bool {
        match self.words().get(index / BITS) {
            Some(word) => word & mask(index) != 0,
            None => false,
        }
    }

    /// Sets bit `index`. Returns `true` when it was clear before.
    ///
    /// With dynamic storage the set grows to hold the bit. Reports
    /// [`OutOfBounds`] when the index is past the end of fixed storage, or
    /// when the allocator refuses.
    #[inline]
    pub fn insert(&mut self, index: usize) -> Result<bool, OutOfBounds> {
        let word = self.word_mut(index).ok_or(OutOfBounds)?;
        let was_clear = *word & mask(index) == 0;
        *word |= mask(index);
        Ok(was_clear)
    }

    /// Clears bit `index`. Returns `true` when it was set before;
    /// out-of-range indices were never set.
    #[inline]
    pub fn remove(&mut self, index: usize) -> bool {
        match self.words_mut().get_mut(index / BITS) {
            Some(word) => {
                let was_set = *word & mask(index) != 0;
                *word &= !mask(index);
                was_set
            }
            None => false,
        }
    }

    /// Sets bit `index` to `value`. With dynamic storage, setting a bit
    /// past the end grows the set, and clearing one changes nothing.
    #[inline]
    pub fn set(&mut self, index: usize, value: bool) -> Result<(), OutOfBounds> {
        if !value && <S::Buffer as Buffer<u64>>::GROWS && index / BITS >= self.used {
            return Ok(());
        }
        let word = self.word_mut(index).ok_or(OutOfBounds)?;
        if value {
            *word |= mask(index);
        } else {
            *word &= !mask(index);
        }
        Ok(())
    }

    /// Flips bit `index` and returns its new value. With dynamic storage
    /// the set grows to hold the bit.
    #[inline]
    pub fn toggle(&mut self, index: usize) -> Result<bool, OutOfBounds> {
        let word = self.word_mut(index).ok_or(OutOfBounds)?;
        *word ^= mask(index);
        Ok(*word & mask(index) != 0)
    }

    /// Clears every bit.
    #[inline]
    pub fn clear(&mut self) {
        self.words_mut().fill(0);
    }

    /// Sets every bit.
    #[inline]
    pub fn fill(&mut self) {
        self.words_mut().fill(u64::MAX);
    }

    /// Index of the lowest set bit.
    #[inline]
    pub fn first_set(&self) -> Option<usize> {
        self.words()
            .iter()
            .enumerate()
            .find(|(_, word)| **word != 0)
            .map(|(index, word)| index * BITS + word.trailing_zeros() as usize)
    }

    /// Index of the lowest clear bit. Handy for finding a free slot.
    #[inline]
    pub fn first_clear(&self) -> Option<usize> {
        self.words()
            .iter()
            .enumerate()
            .find(|(_, word)| **word != u64::MAX)
            .map(|(index, word)| index * BITS + word.trailing_ones() as usize)
    }

    /// Sets every bit that is set in `other`. With dynamic storage the
    /// set grows to hold them; bits beyond the capacity of a set that
    /// cannot grow are ignored.
    #[inline]
    pub fn union_with<O: Storage<u64>>(&mut self, other: &Bitset<O>) {
        if <S::Buffer as Buffer<u64>>::GROWS {
            let needed = other.words().iter().rposition(|word| *word != 0);
            if let Some(last) = needed
                && last >= self.used
            {
                let _ = self.grow(last + 1);
            }
        }
        for (word, theirs) in self.words_mut().iter_mut().zip(other.words().iter()) {
            *word |= theirs;
        }
    }

    /// Keeps only the bits that are also set in `other`. Bits beyond
    /// `other`'s capacity count as clear.
    #[inline]
    pub fn intersect_with<O: Storage<u64>>(&mut self, other: &Bitset<O>) {
        let mut theirs = other.words().iter();
        for word in self.words_mut().iter_mut() {
            *word &= theirs.next().copied().unwrap_or(0);
        }
    }

    /// Clears every bit that is set in `other`.
    #[inline]
    pub fn difference_with<O: Storage<u64>>(&mut self, other: &Bitset<O>) {
        for (word, theirs) in self.words_mut().iter_mut().zip(other.words().iter()) {
            *word &= !theirs;
        }
    }

    /// Iterates over the indices of the set bits, lowest first.
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            words: self.words().iter().enumerate(),
            current: 0,
            base: 0,
        }
    }

    /// The underlying words, 64 bits each, lowest bits first.
    #[inline]
    pub fn as_words(&self) -> &[u64] {
        self.words()
    }
}

#[inline]
const fn mask(index: usize) -> u64 {
    1 << (index % BITS)
}

impl<S: Storage<u64>> fmt::Debug for Bitset<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<'b, S: Storage<u64>> IntoIterator for &'b Bitset<S> {
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
