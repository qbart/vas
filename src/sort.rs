//! Stable sorting for [`Vector`](crate::Vector), in place or with scratch
//! memory from the caller.
//!
//! Short runs are sorted by insertion, then merged pairwise. A merge whose
//! shorter run fits in the scratch memory moves that run out and merges
//! back, O(n). Otherwise it is done in place with rotations: split the
//! longer run in the middle, find where that element belongs in the other
//! run by binary search, rotate the two middle pieces past each other and
//! merge both sides the same way. Without scratch memory that costs
//! O(n log n) comparisons and O(n log² n) moves in total; with scratch for
//! half the elements, O(n log n) of both. The stack depth is O(log n).

#![allow(unsafe_code)]

use core::cmp::Ordering;
use core::mem::MaybeUninit;
use core::ptr;

/// Runs shorter than this are sorted by insertion.
const RUN: usize = 16;

/// Sorts `items` so that `less(b, a)` holds for no `a` before `b`; equal
/// elements keep their order. Uses as much of `scratch` as helps.
pub(crate) fn stable<T>(
    items: &mut [T],
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let len = items.len();
    for run in items.chunks_mut(RUN) {
        insertion(run, less);
    }
    let mut width = RUN;
    while width < len {
        let mut start = 0;
        while start + width < len {
            let end = len.min(start + 2 * width);
            if let Some(pair) = items.get_mut(start..end) {
                merge_trimmed(pair, width, scratch, less);
            }
            start = end;
        }
        width = width.saturating_mul(2);
    }
}

fn insertion<T>(items: &mut [T], less: &mut impl FnMut(&T, &T) -> bool) {
    for sorted in 1..items.len() {
        let mut index = sorted;
        while index > 0 {
            let in_order = match (items.get(index - 1), items.get(index)) {
                (Some(before), Some(after)) => !less(after, before),
                _ => true,
            };
            if in_order {
                break;
            }
            items.swap(index - 1, index);
            index -= 1;
        }
    }
}

/// Merges the sorted runs `items[..mid]` and `items[mid..]`, leaving out
/// first what is already in place: the left elements not above the first
/// right one, and the right elements not below the last left one.
fn merge_trimmed<T>(
    items: &mut [T],
    mid: usize,
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let (Some(last_left), Some(first_right)) = (items.get(mid.wrapping_sub(1)), items.get(mid))
    else {
        return;
    };
    if !less(first_right, last_left) {
        // Already in order: common for nearly sorted input.
        return;
    }
    let skip = items
        .get(..mid)
        .map_or(0, |left| left.partition_point(|x| !less(first_right, x)));
    let keep = mid
        + items
            .get(mid..)
            .map_or(0, |right| right.partition_point(|x| less(x, last_left)));
    if let Some(middle) = items.get_mut(skip..keep) {
        merge(middle, mid - skip, scratch, less);
    }
}

/// Merges the sorted runs `items[..mid]` and `items[mid..]`.
fn merge<T>(
    items: &mut [T],
    mid: usize,
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let len = items.len();
    if mid == 0 || mid >= len {
        return;
    }
    let (Some(last_left), Some(first_right)) = (items.get(mid - 1), items.get(mid)) else {
        return;
    };
    if !less(first_right, last_left) {
        return;
    }

    if mid <= scratch.len() {
        // SAFETY: the left run fits in the scratch memory.
        unsafe { merge_from_front(items, mid, scratch, less) };
        return;
    }
    if len - mid <= scratch.len() {
        // SAFETY: the right run fits in the scratch memory.
        unsafe { merge_from_back(items, mid, scratch, less) };
        return;
    }

    let (left_cut, right_cut);
    if mid >= len - mid {
        // Split the left run; equal elements on the right stay after it.
        left_cut = mid / 2;
        let Some(pivot) = items.get(left_cut) else {
            return;
        };
        right_cut = mid
            + items
                .get(mid..)
                .map_or(0, |right| right.partition_point(|x| less(x, pivot)));
    } else {
        // Split the right run; equal elements on the left stay before it.
        right_cut = mid + (len - mid) / 2;
        let Some(pivot) = items.get(right_cut) else {
            return;
        };
        left_cut = items
            .get(..mid)
            .map_or(0, |left| left.partition_point(|x| !less(pivot, x)));
    }
    if let Some(middle) = items.get_mut(left_cut..right_cut) {
        middle.rotate_left(mid - left_cut);
    }
    let new_mid = left_cut + (right_cut - mid);
    let (front, back) = items.split_at_mut(new_mid);
    merge(front, left_cut, scratch, less);
    merge(back, right_cut - new_mid, scratch, less);
}

/// Elements moved out to the scratch memory, still owed to `dest`. On drop
/// (also when `less` panics) they are moved back, so every element ends up
/// in `items` exactly once.
struct Hole<T> {
    start: *mut T,
    end: *mut T,
    dest: *mut T,
}

impl<T> Drop for Hole<T> {
    fn drop(&mut self) {
        // SAFETY: `start..end` are initialised elements in the scratch
        // memory and `dest` is the gap of exactly that length left in
        // `items`; the two blocks do not overlap.
        unsafe {
            let count = self.end.offset_from_unsigned(self.start);
            ptr::copy_nonoverlapping(self.start, self.dest, count);
        }
    }
}

/// Merges by moving the left run out and filling `items` from the front.
///
/// # Safety
///
/// `mid <= scratch.len()` and `mid <= items.len()`.
unsafe fn merge_from_front<T>(
    items: &mut [T],
    mid: usize,
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let base = items.as_mut_ptr();
    let buffer = scratch.as_mut_ptr().cast::<T>();
    // SAFETY: `mid` elements fit in both blocks, which do not overlap. From
    // here on the left run lives in the scratch memory and `hole` puts back
    // whatever has not been merged yet. `dest` always stays behind `right`
    // (`dest + left still in the buffer == right`), so writing at `dest`
    // never touches an element not merged yet.
    unsafe {
        ptr::copy_nonoverlapping(base, buffer, mid);
        let mut hole = Hole {
            start: buffer,
            end: buffer.add(mid),
            dest: base,
        };
        let mut right = base.add(mid);
        let right_end = base.add(items.len());
        while hole.start < hole.end && right < right_end {
            // On ties the left element goes first: that keeps it stable.
            let take_right = less(&*right, &*hole.start);
            // No branch on the outcome, which is close to random.
            let from = if take_right { right } else { hole.start };
            ptr::copy_nonoverlapping(from, hole.dest, 1);
            hole.dest = hole.dest.add(1);
            right = right.add(usize::from(take_right));
            hole.start = hole.start.add(usize::from(!take_right));
        }
    }
}

/// Merges by moving the right run out and filling `items` from the back.
///
/// # Safety
///
/// `items.len() - mid <= scratch.len()` and `mid <= items.len()`.
unsafe fn merge_from_back<T>(
    items: &mut [T],
    mid: usize,
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let base = items.as_mut_ptr();
    let buffer = scratch.as_mut_ptr().cast::<T>();
    let right_len = items.len() - mid;
    // SAFETY: as in `merge_from_front`, mirrored: the gap the buffer owes
    // is `left_end..out`, whose length is always what is left in the
    // buffer, so `hole.dest` is kept at `left_end`.
    unsafe {
        ptr::copy_nonoverlapping(base.add(mid), buffer, right_len);
        let mut hole = Hole {
            start: buffer,
            end: buffer.add(right_len),
            dest: base.add(mid),
        };
        let mut out = base.add(items.len());
        while hole.start < hole.end && hole.dest > base {
            let left = hole.dest.sub(1);
            let last = hole.end.sub(1);
            // On ties the right element goes last: that keeps it stable.
            let take_left = less(&*last, &*left);
            out = out.sub(1);
            let from = if take_left { left } else { last };
            ptr::copy(from, out, 1);
            hole.dest = hole.dest.sub(usize::from(take_left));
            hole.end = hole.end.sub(usize::from(!take_left));
        }
    }
}

/// `less` from a comparison.
#[inline]
pub(crate) fn by<T>(mut compare: impl FnMut(&T, &T) -> Ordering) -> impl FnMut(&T, &T) -> bool {
    move |a, b| compare(a, b) == Ordering::Less
}
