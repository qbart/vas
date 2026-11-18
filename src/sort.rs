//! Sorting slices in place, with no memory from the library: vas's own
//! sorts, not the ones from `core`.
//!
//! - [`unstable`], [`unstable_by`], [`unstable_by_key`]: a quicksort.
//!   Fastest; equal elements may change order.
//! - [`stable`], [`stable_by`], [`stable_by_key`]: a merge sort that keeps
//!   equal elements in their order, needing no memory.
//! - [`stable_in`]: the same merge sort, faster with scratch memory from
//!   the caller.
//!
//! The same sorts are methods on [`Vector`](crate::Vector); a
//! [`Deque`](crate::Deque) is sorted through `make_contiguous`.
//!
//! What every sort here promises:
//!
//! - **No panics.** A comparison that is not a total order (`partial_cmp`
//!   on floats meeting a NaN, a comparison that changes its answers) leaves
//!   the elements in an unspecified order, but every element is still there
//!   exactly once and the sort returns.
//! - **The same result everywhere.** The order of equal elements after an
//!   unstable sort depends only on the input and the vas version, not on
//!   the compiler, the target or the standard library.
//! - **Panic safety.** If the comparison panics, every element is still in
//!   the slice exactly once.
//! - **O(n log n)** comparisons in the worst case, and O(n) on input that is
//!   already sorted or reversed (unstable) or sorted (stable).
//!
//! # Unstable: quicksort
//!
//! Slices are partitioned around a pivot, the median of three (or, from 64
//! elements, a median of medians of samples spread over the slice), with a
//! branch-free Lomuto partition that moves each element twice. The larger
//! elements are sorted first, by recursion, while they are still in the
//! cache; the smaller ones by the loop. When the pivot equals the one of
//! the partition just before, the elements equal to it are put first and
//! skipped, so many repeated keys cost O(n log k) for k distinct keys. After
//! 2 log₂ n partitions, a slice still left falls back to heapsort, which
//! keeps the worst case O(n log n) on inputs built to defeat the pivot.
//!
//! Slices of up to 24 elements of up to 16 bytes are sorted without
//! branching on the comparisons, which are close to random there: runs of
//! four by a network of five comparisons, then merges back and forth with a
//! 32-element buffer on the stack (at most 512 bytes). Equal runs merge from
//! both ends at once, a loop of known length. Larger elements, up to 20 at a
//! time, are sorted by insertion.
//!
//! # Stable: merge sort
//!
//! Runs of 32 small elements (or 16 larger ones) are sorted as the quicksort
//! sorts short slices, then merged pairwise. A merge whose shorter run fits
//! in the scratch memory moves that run out and merges back, O(n).
//! Otherwise it is done in place with rotations: split the longer run in the
//! middle, find where that element belongs in the other run by binary
//! search, rotate the two middle pieces past each other and merge both
//! sides the same way. Without scratch memory that costs O(n log n)
//! comparisons and O(n log² n) moves in total; with scratch for half the
//! elements, O(n log n) of both. The stack depth is O(log n).
//!
//! ```
//! let mut scores = [(3, "c"), (1, "a"), (2, "b"), (1, "d")];
//! vas::sort::unstable_by_key(&mut scores, |s| s.0);
//! assert_eq!(scores[0].0, 1);
//!
//! // Keeps "a" before "d".
//! let mut scores = [(3, "c"), (1, "a"), (2, "b"), (1, "d")];
//! vas::sort::stable_by_key(&mut scores, |s| s.0);
//! assert_eq!(scores, [(1, "a"), (1, "d"), (2, "b"), (3, "c")]);
//! ```

#![allow(unsafe_code)]

use core::cmp::Ordering;
use core::mem::{ManuallyDrop, MaybeUninit};
use core::ptr;

/// Sorts `items`; equal elements may change order. See the
/// [module](self) docs.
#[inline]
pub fn unstable<T: Ord>(items: &mut [T]) {
    quick(items, &mut T::lt);
}

/// Sorts `items` by `compare`; equal elements may change order.
#[inline]
pub fn unstable_by<T>(items: &mut [T], compare: impl FnMut(&T, &T) -> Ordering) {
    quick(items, &mut by(compare));
}

/// Sorts `items` by the key `key` gives; elements with equal keys may
/// change order. The key is computed on every comparison.
#[inline]
pub fn unstable_by_key<T, K: Ord>(items: &mut [T], mut key: impl FnMut(&T) -> K) {
    quick(items, &mut |a: &T, b: &T| key(a) < key(b));
}

/// Sorts `items`, keeping equal elements in their order. Needs no memory;
/// O(n log n) comparisons and O(n log² n) moves.
#[inline]
pub fn stable<T: Ord>(items: &mut [T]) {
    merge_sort(items, &mut [], &mut T::lt);
}

/// Sorts `items` by `compare`, keeping equal elements in their order.
#[inline]
pub fn stable_by<T>(items: &mut [T], compare: impl FnMut(&T, &T) -> Ordering) {
    merge_sort(items, &mut [], &mut by(compare));
}

/// Sorts `items` by the key `key` gives, keeping elements with equal keys
/// in their order. The key is computed on every comparison.
#[inline]
pub fn stable_by_key<T, K: Ord>(items: &mut [T], mut key: impl FnMut(&T) -> K) {
    merge_sort(items, &mut [], &mut |a: &T, b: &T| key(a) < key(b));
}

/// Sorts `items` by `compare`, keeping equal elements in their order,
/// using `scratch` to go faster. Any amount of scratch memory works; with
/// room for half the elements every merge uses it, and the sort is
/// O(n log n) moves instead of O(n log² n).
///
/// `scratch` is only borrowed for the call: pass a `Memory` block, an arena
/// piece or any `&mut [MaybeUninit<T>]`. Its contents are not read and are
/// left uninitialised.
#[inline]
pub fn stable_in<T>(
    items: &mut [T],
    scratch: &mut [MaybeUninit<T>],
    compare: impl FnMut(&T, &T) -> Ordering,
) {
    merge_sort(items, scratch, &mut by(compare));
}

/// `less` from a comparison.
#[inline]
fn by<T>(mut compare: impl FnMut(&T, &T) -> Ordering) -> impl FnMut(&T, &T) -> bool {
    move |a, b| compare(a, b) == Ordering::Less
}

// Unstable -------------------------------------------------------------

/// The quicksort sorts slices up to this long by insertion, when the
/// elements are larger than [`NET_SIZE`].
const SMALL: usize = 20;

/// Elements up to this size are sorted in short slices by [`small_sort`],
/// with a stack buffer of [`NET`] elements (at most 512 bytes).
const NET_SIZE: usize = 16;

/// Slices up to this long are sorted by [`small_sort`].
const NET: usize = 32;

/// Up to which length the quicksort hands a slice to [`small_sort`]: below
/// [`NET`], since partitions are cheaper per element than merges of
/// uneven runs (measured: 24 beats 16, 20 and 32).
const fn small_len<T>() -> usize {
    if size_of::<T>() <= NET_SIZE {
        24
    } else {
        SMALL
    }
}

/// From this length the pivot is a median of medians of three.
const NINTHER: usize = 64;

/// Sorts `items` so that `less(b, a)` holds for no `a` before `b`.
fn quick<T>(items: &mut [T], less: &mut impl FnMut(&T, &T) -> bool) {
    let len = items.len();
    if size_of::<T>() == 0 {
        // All elements are the same value.
        return;
    }
    if len <= small_len::<T>() {
        small_sort(items, less);
        return;
    }
    let (run, descending) = existing_run(items, less);
    if run == len {
        if descending {
            items.reverse();
        }
        return;
    }
    // Partitions allowed on any one path before falling back to heapsort;
    // it also bounds the recursion depth.
    let limit = 2 * len.ilog2();
    quicksort(items, None, limit, less);
}

/// The length of the run at the start of `items`: ascending, or strictly
/// descending (then `true`).
fn existing_run<T>(items: &[T], less: &mut impl FnMut(&T, &T) -> bool) -> (usize, bool) {
    let (Some(first), Some(second)) = (items.first(), items.get(1)) else {
        return (items.len(), false);
    };
    let descending = less(second, first);
    let mut run = 2;
    for pair in items.windows(2).skip(1) {
        let [a, b] = pair else {
            break;
        };
        if less(b, a) != descending {
            break;
        }
        run += 1;
    }
    (run, descending)
}

/// `ancestor` is the pivot just before `items` in the whole slice, if any:
/// no element of `items` is less than it.
fn quicksort<'a, T>(
    mut items: &'a mut [T],
    mut ancestor: Option<&'a T>,
    mut limit: u32,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    loop {
        if items.len() <= small_len::<T>() {
            small_sort(items, less);
            return;
        }
        if limit == 0 {
            heapsort(items, less);
            return;
        }
        limit -= 1;
        let pivot = choose_pivot(items, less);
        if let (Some(before), Some(chosen)) = (ancestor, items.get(pivot))
            && !less(before, chosen)
        {
            // No element here is below the ancestor and the pivot is not
            // above it, so the pivot equals it. Put the elements equal to
            // it first; they are in place, sort only the rest.
            let equal = partition(items, pivot, &mut |x: &T, p: &T| !less(p, x));
            let rest = items;
            items = rest.get_mut(equal + 1..).unwrap_or_default();
            ancestor = None;
            continue;
        }
        let mid = partition(items, pivot, less);
        let rest = items;
        let Some((left, right)) = rest.split_at_mut_checked(mid) else {
            return;
        };
        let Some((pivot, right)) = right.split_first_mut() else {
            return;
        };
        // The right part was walked last and is still in the cache.
        quicksort(right, Some(pivot), limit, less);
        items = left;
    }
}

/// The index of a pivot close to the median.
fn choose_pivot<T>(items: &[T], less: &mut impl FnMut(&T, &T) -> bool) -> usize {
    let len = items.len();
    let eighth = len / 8;
    let (a, b, c) = (0, eighth * 4, eighth * 7);
    if len < NINTHER {
        median3(items, a, b, c, less)
    } else {
        median3_rec(items, a, b, c, eighth, less)
    }
}

/// The median of three samples, each itself the median of three samples
/// `step` apart while those are far enough apart.
fn median3_rec<T>(
    items: &[T],
    mut a: usize,
    mut b: usize,
    mut c: usize,
    step: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) -> usize {
    if step * 8 >= NINTHER {
        let eighth = step / 8;
        a = median3_rec(items, a, a + eighth * 4, a + eighth * 7, eighth, less);
        b = median3_rec(items, b, b + eighth * 4, b + eighth * 7, eighth, less);
        c = median3_rec(items, c, c + eighth * 4, c + eighth * 7, eighth, less);
    }
    median3(items, a, b, c, less)
}

/// The index of the median of `items[a]`, `items[b]` and `items[c]`.
fn median3<T>(
    items: &[T],
    a: usize,
    b: usize,
    c: usize,
    less: &mut impl FnMut(&T, &T) -> bool,
) -> usize {
    let (Some(x), Some(y), Some(z)) = (items.get(a), items.get(b), items.get(c)) else {
        return a;
    };
    let below_y = less(x, y);
    if below_y != less(x, z) {
        // `x` is between the two.
        return a;
    }
    // `x` is below both (then the median is the smaller) or above both.
    if less(y, z) == below_y { b } else { c }
}

/// Puts the element at `pivot` in its place: the elements `less` than it
/// before, the others after. Returns that place.
fn partition<T>(items: &mut [T], pivot: usize, less: &mut impl FnMut(&T, &T) -> bool) -> usize {
    if pivot >= items.len() {
        return 0;
    }
    items.swap(0, pivot);
    let Some((pivot, rest)) = items.split_first_mut() else {
        return 0;
    };
    let below = lomuto(rest, pivot, less);
    // `below <= rest.len()`, so the last element below the pivot (or the
    // pivot itself) is in bounds.
    items.swap(0, below);
    below
}

/// An element taken out of the slice, owed to the gap at `pos`. On drop
/// (also when `less` panics) it is written there, so every element is in
/// the slice exactly once.
struct Gap<T> {
    value: *const T,
    pos: *mut T,
}

impl<T> Drop for Gap<T> {
    fn drop(&mut self) {
        // SAFETY: `value` is the element taken out and `pos` the one slot
        // without an element; they do not overlap.
        unsafe { ptr::copy_nonoverlapping(self.value, self.pos, 1) };
    }
}

/// Moves the elements `less` than `pivot` to the front, in no particular
/// order, and returns how many there are. Each step is free of branches on
/// the comparison: the element is written at the front either way and the
/// front only advances when it belongs there.
fn lomuto<T>(items: &mut [T], pivot: &T, less: &mut impl FnMut(&T, &T) -> bool) -> usize {
    let len = items.len();
    if len == 0 {
        return 0;
    }
    let base = items.as_mut_ptr();
    // SAFETY: every pointer stays in `base..base + len`: `right` walks it
    // once, `below <= right - base - 1` since it grows at most once per
    // step, and the gap is the slot just walked over. The first element is
    // taken out, leaving a gap. Each step moves the first element not below
    // the pivot (at `base + below`, or the gap itself) into the gap, and
    // the new element into its slot; the gap is then the new element's old
    // slot. `gap` puts the taken element back if `less` panics.
    unsafe {
        let value = ManuallyDrop::new(ptr::read(base));
        let mut gap = Gap {
            value: &raw const *value,
            pos: base,
        };
        let mut below = 0;
        let mut right = base.add(1);
        let end = base.add(len);
        while right < end {
            let is_below = less(&*right, pivot);
            let front = base.add(below);
            ptr::copy(front, gap.pos, 1);
            ptr::copy_nonoverlapping(right, front, 1);
            gap.pos = right;
            below += usize::from(is_below);
            right = right.add(1);
        }
        // Last, the element taken out goes to the front the same way.
        let is_below = less(&*value, pivot);
        let front = base.add(below);
        ptr::copy(front, gap.pos, 1);
        gap.pos = front;
        drop(gap);
        below + usize::from(is_below)
    }
}

/// Sorts by heapsort: O(n log n) on any input, used when the pivots keep
/// coming out bad.
fn heapsort<T>(items: &mut [T], less: &mut impl FnMut(&T, &T) -> bool) {
    let len = items.len();
    for node in (0..len / 2).rev() {
        sift_down(items, node, less);
    }
    for end in (1..len).rev() {
        items.swap(0, end);
        if let Some(heap) = items.get_mut(..end) {
            sift_down(heap, 0, less);
        }
    }
}

/// Moves `heap[node]` down until it is not less than its children.
fn sift_down<T>(heap: &mut [T], mut node: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    loop {
        let mut child = 2 * node + 1;
        let Some(left) = heap.get(child) else {
            return;
        };
        if let Some(right) = heap.get(child + 1)
            && less(left, right)
        {
            child += 1;
        }
        let (Some(parent), Some(larger)) = (heap.get(node), heap.get(child)) else {
            return;
        };
        if !less(parent, larger) {
            return;
        }
        heap.swap(node, child);
        node = child;
    }
}

/// Sorts a short slice. Elements up to [`NET_SIZE`] bytes in slices up to
/// [`NET`] long go through [`net_sort`] with a buffer on the stack; others
/// by insertion. Stable.
fn small_sort<T>(items: &mut [T], less: &mut impl FnMut(&T, &T) -> bool) {
    if size_of::<T>() > NET_SIZE || items.len() > NET {
        insertion(items, less);
        return;
    }
    let mut buffer = [const { MaybeUninit::<T>::uninit() }; NET];
    net_sort(items, &mut buffer, less);
}

/// Copies `len` elements from `from` back over `to` on drop: when a merge
/// pass reads from the scratch memory and writes `items`, a panic in
/// `less` leaves `items` part written, and the scratch memory still holds
/// every element.
struct Restore<T> {
    from: *const T,
    to: *mut T,
    len: usize,
}

impl<T> Drop for Restore<T> {
    fn drop(&mut self) {
        // SAFETY: `from` holds `len` elements, `to` has room for them, and
        // they are different blocks.
        unsafe { ptr::copy_nonoverlapping(self.from, self.to, self.len) };
    }
}

/// Sorts `items` without branching on the comparisons, which are close to
/// random in the leaves of the quicksort: runs of four by a fixed network
/// of five comparisons into `scratch`, then merges back and forth between
/// the two. Stable. `scratch` must be at least as long as `items`.
fn net_sort<T>(
    items: &mut [T],
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let len = items.len();
    if len < 2 || scratch.len() < len {
        insertion(items, less);
        return;
    }
    let v = items.as_mut_ptr();
    let s = scratch.as_mut_ptr().cast::<T>();
    // SAFETY: all pointers stay within the first `len` elements of `items`
    // and `scratch`, two different blocks. Passes from `items` to the
    // scratch memory leave `items` whole, so a panic in `less` loses
    // nothing; passes the other way are covered by `Restore`. Every pass
    // writes each element exactly once (`sort4` and `merge_into` write a
    // permutation of their input whatever `less` answers).
    unsafe {
        let full = len - len % 4;
        let mut start = 0;
        while start < full {
            sort4(v.add(start), s.add(start), less);
            start += 4;
        }
        ptr::copy_nonoverlapping(v.add(full), s.add(full), len - full);
        insertion(
            core::slice::from_raw_parts_mut(s.add(full), len - full),
            less,
        );
        let (mut src, mut dst) = (s, v);
        let mut width = 4;
        while width < len {
            // Built only when needed: dropping it copies.
            let restore = if src == s {
                Some(Restore {
                    from: s,
                    to: v,
                    len,
                })
            } else {
                None
            };
            let mut start = 0;
            while start < len {
                let mid = len.min(start + width);
                let end = len.min(start + 2 * width);
                if end - mid == width {
                    merge_both_ends(src.add(start), width, dst.add(start), less);
                } else {
                    merge_into(
                        src.add(start),
                        src.add(mid),
                        src.add(end),
                        dst.add(start),
                        less,
                    );
                }
                start = end;
            }
            core::mem::forget(restore);
            core::mem::swap(&mut src, &mut dst);
            width *= 2;
        }
        if src == s {
            ptr::copy_nonoverlapping(s, v, len);
        }
    }
}

/// `if_true` or `if_false`, meant to compile to a conditional move.
#[inline(always)]
fn select<T>(condition: bool, if_true: *const T, if_false: *const T) -> *const T {
    if condition { if_true } else { if_false }
}

/// Writes the four elements at `v` to `dst` in order, stable, with five
/// comparisons and no branches.
///
/// # Safety
///
/// `v` and `dst` hold four elements each and do not overlap.
#[inline(always)]
unsafe fn sort4<T>(v: *const T, dst: *mut T, less: &mut impl FnMut(&T, &T) -> bool) {
    // SAFETY: every pointer is one of `v..v + 4`, and each of the four is
    // written to `dst` exactly once: `min` and `low` come from different
    // pairs or the same pair in order, likewise for the others.
    unsafe {
        let c1 = less(&*v.add(1), &*v);
        let c2 = less(&*v.add(3), &*v.add(2));
        // a <= b and c <= d.
        let a = v.add(usize::from(c1));
        let b = v.add(usize::from(!c1));
        let c = v.add(2 + usize::from(c2));
        let d = v.add(2 + usize::from(!c2));
        let c3 = less(&*c, &*a);
        let c4 = less(&*d, &*b);
        let min = select(c3, c, a);
        let max = select(c4, b, d);
        let left = select(c3, a, select(c4, c, b));
        let right = select(c4, d, select(c3, b, c));
        let c5 = less(&*right, &*left);
        let low = select(c5, right, left);
        let high = select(c5, left, right);
        ptr::copy_nonoverlapping(min, dst, 1);
        ptr::copy_nonoverlapping(low, dst.add(1), 1);
        ptr::copy_nonoverlapping(high, dst.add(2), 1);
        ptr::copy_nonoverlapping(max, dst.add(3), 1);
    }
}

/// Merges the sorted runs `a..a + half` and `a + half..a + 2 * half` into
/// `out`, stable, taking the smallest element to the front and the largest
/// to the back in each of `half` steps: no branches on the comparisons and
/// a loop of known length. A comparison that is not a total order can make
/// the two ends take the same element twice; that is caught at the end and
/// the runs are then copied as they are, since `a` is not written.
///
/// # Safety
///
/// `a` holds `2 * half` elements, `half >= 1`, and `out` has room for as
/// many and does not overlap it.
#[inline(always)]
unsafe fn merge_both_ends<T>(
    a: *const T,
    half: usize,
    out: *mut T,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    // SAFETY: each end moves at most `half` times, so the front pointers
    // read at most `half - 1` past their run's start and the back pointers
    // at most `half - 1` before their run's end: always inside `a`. The back
    // pointers may step to just before `a`, hence the wrapping arithmetic;
    // they are only compared then, not read.
    unsafe {
        let mid = a.add(half);
        let (mut left, mut right) = (a, mid);
        let (mut left_back, mut right_back) = (mid.sub(1), a.add(2 * half - 1));
        let (mut front, mut back) = (out, out.add(2 * half - 1));
        for _ in 0..half {
            let take_right = less(&*right, &*left);
            ptr::copy_nonoverlapping(select(take_right, right, left), front, 1);
            front = front.add(1);
            right = right.wrapping_add(usize::from(take_right));
            left = left.wrapping_add(usize::from(!take_right));

            let take_left = less(&*right_back, &*left_back);
            ptr::copy_nonoverlapping(select(take_left, left_back, right_back), back, 1);
            back = back.wrapping_sub(1);
            left_back = left_back.wrapping_sub(usize::from(take_left));
            right_back = right_back.wrapping_sub(usize::from(!take_left));
        }
        if left != left_back.wrapping_add(1) || right != right_back.wrapping_add(1) {
            ptr::copy_nonoverlapping(a, out, 2 * half);
        }
    }
}

/// Merges the sorted runs `a..mid` and `mid..end` into `out`, stable,
/// without branching on the comparisons.
///
/// # Safety
///
/// `a <= mid <= end` lie in one block, `out` has room for `end - a`
/// elements and does not overlap it.
#[inline(always)]
unsafe fn merge_into<T>(
    mut a: *const T,
    mid: *const T,
    end: *const T,
    mut out: *mut T,
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let mut b = mid;
    // SAFETY: `a` stays in `..=mid` and `b` in `..=end`, whatever `less`
    // answers, and `out` advances once per element taken.
    unsafe {
        while a < mid && b < end {
            let take_b = less(&*b, &*a);
            ptr::copy_nonoverlapping(select(take_b, b, a), out, 1);
            out = out.add(1);
            b = b.add(usize::from(take_b));
            a = a.add(usize::from(!take_b));
        }
        let rest = mid.offset_from_unsigned(a);
        ptr::copy_nonoverlapping(a, out, rest);
        ptr::copy_nonoverlapping(b, out.add(rest), end.offset_from_unsigned(b));
    }
}

/// Sorts by insertion: for short slices. Stable.
fn insertion<T>(items: &mut [T], less: &mut impl FnMut(&T, &T) -> bool) {
    let len = items.len();
    let base = items.as_mut_ptr();
    for tail in 1..len {
        // SAFETY: `tail < len` and `items[..tail]` is sorted.
        unsafe { insert_tail(base, tail, less) };
    }
}

/// Moves the element at `tail` back into the sorted `..tail` before it,
/// after the elements not greater than it.
///
/// # Safety
///
/// `base..=base + tail` are elements of one slice and `tail >= 1`.
unsafe fn insert_tail<T>(base: *mut T, tail: usize, less: &mut impl FnMut(&T, &T) -> bool) {
    // SAFETY: the pointers stay in `base..=base + tail` (the loop stops at
    // `base`). The element is taken out and `gap` writes it back to the one
    // free slot, also if `less` panics.
    unsafe {
        let tail = base.add(tail);
        let mut before = tail.sub(1);
        if !less(&*tail, &*before) {
            return;
        }
        let value = ManuallyDrop::new(ptr::read(tail));
        let mut gap = Gap {
            value: &raw const *value,
            pos: tail,
        };
        loop {
            ptr::copy_nonoverlapping(before, gap.pos, 1);
            gap.pos = before;
            if before == base {
                break;
            }
            before = before.sub(1);
            if !less(&*value, &*before) {
                break;
            }
        }
    }
}

// Stable ---------------------------------------------------------------

/// The merge sort starts from runs this long, sorted by [`small_sort`].
const fn run_len<T>() -> usize {
    if size_of::<T>() <= NET_SIZE { NET } else { 16 }
}

/// Sorts `items` so that `less(b, a)` holds for no `a` before `b`; equal
/// elements keep their order. Uses as much of `scratch` as helps.
fn merge_sort<T>(
    items: &mut [T],
    scratch: &mut [MaybeUninit<T>],
    less: &mut impl FnMut(&T, &T) -> bool,
) {
    let len = items.len();
    if size_of::<T>() == 0 {
        return;
    }
    let run = run_len::<T>();
    for chunk in items.chunks_mut(run) {
        small_sort(chunk, less);
    }
    let mut width = run;
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
