//! UTF-8 string over borrowed, inline or growing memory.

#![allow(unsafe_code)]

use core::borrow::Borrow;
use core::hash::{Hash, Hasher};
use core::ops::{Deref, DerefMut};
use core::{cmp, fmt, str};

use crate::allocator::Allocator;
use crate::storage::{Dynamic, Inline, IntoBuffer, Storage};
use crate::{Full, InsertError, Vector};

/// A UTF-8 string in memory provided by the caller: names, labels, chat
/// lines, text input, formatted HUD text.
///
/// It reads as a `&str` (through `Deref`, so every `str` method works) and
/// formats with `write!`. Writes that do not fit fail and change nothing,
/// except [`push_str_truncating`](Self::push_str_truncating), which keeps
/// what fits.
///
/// ```
/// use core::fmt::Write;
/// use vas::{Inline, String};
///
/// let mut score = String::<Inline<32>>::inline();
/// assert!(write!(score, "Score: {}", 1250).is_ok());
/// assert_eq!(score, "Score: 1250");
/// assert!(score.ends_with("1250"));
/// ```
///
/// With [`Dynamic`] storage the string grows.
pub struct String<S: Storage<u8>> {
    // Invariant: the bytes are valid UTF-8.
    bytes: Vector<u8, S>,
}

impl<const N: usize> String<Inline<N>> {
    /// Creates an empty string of up to `N` bytes, held inside itself.
    /// This is `const`, so it can initialise a `static`.
    #[inline]
    pub const fn inline() -> Self {
        Self {
            bytes: Vector::inline(),
        }
    }
}

impl<const N: usize> Default for String<Inline<N>> {
    #[inline]
    fn default() -> Self {
        Self::inline()
    }
}

/// Only strings with inline storage can be cloned.
impl<const N: usize> Clone for String<Inline<N>> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes.clone(),
        }
    }
}

impl<A: Allocator + Default> Default for String<Dynamic<A>> {
    /// Creates an empty string that grows through `A::default()`.
    #[inline]
    fn default() -> Self {
        Self::new(Dynamic(A::default()))
    }
}

/// Bytes in the UTF-8 encoding of a character that starts with `first`.
#[inline]
const fn width(first: u8) -> usize {
    match first {
        0..0x80 => 1,
        0xE0..0xF0 => 3,
        0xF0.. => 4,
        _ => 2,
    }
}

impl<S: Storage<u8>> String<S> {
    /// Creates an empty string on `buffer`: a reference to memory borrows
    /// it, a `Memory` passed by value is stored inside the string.
    #[inline]
    pub fn new<B>(buffer: B) -> Self
    where
        B: IntoBuffer<u8, Storage = S>,
    {
        Self {
            bytes: Vector::new(buffer),
        }
    }

    /// Creates a string on `buffer` holding a copy of `text`, or fails with
    /// [`Full`] when it does not fit.
    #[inline]
    pub fn from_str_in<B>(buffer: B, text: &str) -> Result<Self, Full<()>>
    where
        B: IntoBuffer<u8, Storage = S>,
    {
        let mut string = Self::new(buffer);
        string.push_str(text)?;
        Ok(string)
    }

    /// Length in bytes.
    #[inline]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// `true` when the string is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Number of bytes the string has room for. With dynamic storage it
    /// grows.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Number of bytes that still fit without growing.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.bytes.remaining()
    }

    /// `true` when no more bytes fit without growing.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.bytes.is_full()
    }

    /// The text.
    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: the bytes are valid UTF-8 by the invariant.
        unsafe { str::from_utf8_unchecked(self.bytes.as_slice()) }
    }

    /// The text, mutably. A `&mut str` can only be changed in ways that
    /// keep it valid UTF-8, such as `make_ascii_uppercase`.
    #[inline]
    pub fn as_mut_str(&mut self) -> &mut str {
        // SAFETY: as in `as_str`; safe code cannot make a `&mut str`
        // invalid UTF-8.
        unsafe { str::from_utf8_unchecked_mut(self.bytes.as_mut_slice()) }
    }

    /// The UTF-8 bytes.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// Appends `ch`, or hands it back in [`Full`] when it does not fit.
    #[inline]
    pub fn push(&mut self, ch: char) -> Result<(), Full<char>> {
        let mut encoded = [0; 4];
        let encoded = ch.encode_utf8(&mut encoded);
        self.bytes
            .extend_from_slice(encoded.as_bytes())
            .map_err(|_| Full(ch))
    }

    /// Appends all of `text`, or nothing when it does not fit.
    #[inline]
    pub fn push_str(&mut self, text: &str) -> Result<(), Full<()>> {
        self.bytes.extend_from_slice(text.as_bytes())
    }

    /// Appends as much of `text` as fits, whole characters only, and
    /// returns the number of bytes written. For labels and logs that may be
    /// cut short.
    pub fn push_str_truncating(&mut self, text: &str) -> usize {
        if self.bytes.reserve(text.len()).is_err() {
            // Fits only in part: cut at the last character boundary.
            let mut end = self.remaining().min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let part = text.get(..end).unwrap_or_default();
            return self.push_str(part).map_or(0, |()| part.len());
        }
        self.push_str(text).map_or(0, |()| text.len())
    }

    /// Removes the last character and returns it.
    #[inline]
    pub fn pop(&mut self) -> Option<char> {
        let ch = self.as_str().chars().next_back()?;
        self.bytes.truncate(self.len() - ch.len_utf8());
        Some(ch)
    }

    /// Shortens the string to at most `len` bytes. When `len` falls inside
    /// a character, that character goes too.
    pub fn truncate(&mut self, len: usize) {
        if len >= self.len() {
            return;
        }
        let mut end = len;
        while !self.as_str().is_char_boundary(end) {
            end -= 1;
        }
        self.bytes.truncate(end);
    }

    /// Removes all text.
    #[inline]
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// Inserts `bytes`, valid UTF-8, at the character boundary `index`.
    /// Returns `false`, with nothing changed, when they do not fit.
    fn insert_bytes(&mut self, index: usize, bytes: &[u8]) -> bool {
        // Append, then rotate the appended bytes into place: the string is
        // valid UTF-8 after each step.
        if self.bytes.extend_from_slice(bytes).is_err() {
            return false;
        }
        if let Some(tail) = self.bytes.get_mut(index..) {
            tail.rotate_right(bytes.len());
        }
        true
    }

    /// Inserts `ch` at byte `index`. O(len).
    ///
    /// Fails with [`InsertError::OutOfBounds`] when `index` is past the end
    /// or inside a character, and [`InsertError::Full`] when there is no
    /// room; both hand `ch` back and change nothing.
    pub fn insert(&mut self, index: usize, ch: char) -> Result<(), InsertError<char>> {
        if !self.as_str().is_char_boundary(index) {
            return Err(InsertError::OutOfBounds(ch));
        }
        let mut encoded = [0; 4];
        let encoded = ch.encode_utf8(&mut encoded);
        if self.insert_bytes(index, encoded.as_bytes()) {
            Ok(())
        } else {
            Err(InsertError::Full(ch))
        }
    }

    /// Inserts `text` at byte `index`, failing as [`insert`](Self::insert)
    /// does. O(len).
    pub fn insert_str(&mut self, index: usize, text: &str) -> Result<(), InsertError<()>> {
        if !self.as_str().is_char_boundary(index) {
            return Err(InsertError::OutOfBounds(()));
        }
        if self.insert_bytes(index, text.as_bytes()) {
            Ok(())
        } else {
            Err(InsertError::Full(()))
        }
    }

    /// Removes the character starting at byte `index` and returns it.
    /// `None`, with nothing changed, when `index` is at or past the end or
    /// inside a character. O(len).
    pub fn remove(&mut self, index: usize) -> Option<char> {
        let ch = self.as_str().get(index..)?.chars().next()?;
        let width = ch.len_utf8();
        // Rotate the character to the end, then cut it off: valid UTF-8
        // after each step.
        if let Some(tail) = self.bytes.get_mut(index..) {
            tail.rotate_left(width);
        }
        self.bytes.truncate(self.len() - width);
        Some(ch)
    }

    /// Keeps only the characters for which `keep` is `true`. O(len).
    pub fn retain(&mut self, mut keep: impl FnMut(char) -> bool) {
        let len = self.len();
        let mut gap = Gap {
            bytes: &mut self.bytes,
            read: 0,
            write: 0,
            len,
        };
        while gap.read < len {
            let bytes = gap.bytes.as_slice();
            let Some(&first) = bytes.get(gap.read) else {
                break;
            };
            let end = gap.read + width(first);
            // Decoding at most four bytes: the text before `read` is being
            // rewritten, so the whole string is not read as a `str` here.
            let ch = bytes
                .get(gap.read..end)
                .and_then(|encoded| str::from_utf8(encoded).ok())
                .and_then(|encoded| encoded.chars().next());
            let Some(ch) = ch else {
                break;
            };
            if keep(ch) {
                gap.bytes.copy_within(gap.read..end, gap.write);
                gap.write += end - gap.read;
            }
            gap.read = end;
        }
    }
}

/// The state of [`String::retain`]: bytes `..write` are kept, `write..read`
/// are dropped, `read..len` are not looked at yet. Dropping it closes the
/// gap, so the string is valid UTF-8 even if `keep` panics.
struct Gap<'v, S: Storage<u8>> {
    bytes: &'v mut Vector<u8, S>,
    read: usize,
    write: usize,
    len: usize,
}

impl<S: Storage<u8>> Drop for Gap<'_, S> {
    fn drop(&mut self) {
        if self.read < self.len {
            self.bytes.copy_within(self.read..self.len, self.write);
        }
        self.bytes.truncate(self.write + (self.len - self.read));
    }
}

impl<S: Storage<u8>> Deref for String<S> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl<S: Storage<u8>> DerefMut for String<S> {
    #[inline]
    fn deref_mut(&mut self) -> &mut str {
        self.as_mut_str()
    }
}

impl<S: Storage<u8>> AsRef<str> for String<S> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<S: Storage<u8>> AsRef<[u8]> for String<S> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

/// Hashes and compares as its `str`, so a map keyed by `String` can be
/// looked up with a `&str`.
impl<S: Storage<u8>> Borrow<str> for String<S> {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// `write!` and `writeln!` append the formatted text. A write that does not
/// fit returns `fmt::Error`; the pieces formatted before it stay.
impl<S: Storage<u8>> fmt::Write for String<S> {
    #[inline]
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.push_str(text).map_err(|_| fmt::Error)
    }

    #[inline]
    fn write_char(&mut self, ch: char) -> fmt::Result {
        self.push(ch).map_err(|_| fmt::Error)
    }
}

impl<S: Storage<u8>> fmt::Display for String<S> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl<S: Storage<u8>> fmt::Debug for String<S> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl<S: Storage<u8>> Hash for String<S> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl<S: Storage<u8>, O: Storage<u8>> PartialEq<String<O>> for String<S> {
    #[inline]
    fn eq(&self, other: &String<O>) -> bool {
        self.as_str() == other.as_str()
    }
}

impl<S: Storage<u8>> Eq for String<S> {}

impl<S: Storage<u8>> PartialEq<str> for String<S> {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl<S: Storage<u8>> PartialEq<&str> for String<S> {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl<S: Storage<u8>> PartialEq<String<S>> for str {
    #[inline]
    fn eq(&self, other: &String<S>) -> bool {
        self == other.as_str()
    }
}

impl<S: Storage<u8>> PartialEq<String<S>> for &str {
    #[inline]
    fn eq(&self, other: &String<S>) -> bool {
        *self == other.as_str()
    }
}

impl<S: Storage<u8>, O: Storage<u8>> PartialOrd<String<O>> for String<S> {
    #[inline]
    fn partial_cmp(&self, other: &String<O>) -> Option<cmp::Ordering> {
        Some(self.as_str().cmp(other.as_str()))
    }
}

impl<S: Storage<u8>> Ord for String<S> {
    #[inline]
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}
