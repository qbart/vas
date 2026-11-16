//! k-d tree for nearest-neighbour search over user-provided memory.

use crate::Vector;
use crate::aabb::{Aabb, distance_squared};

/// A search result: the index of an item in [`KdTree::items`] and its
/// squared distance from the query point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Neighbor {
    /// Index of the item; pass it to [`KdTree::get`].
    pub index: usize,
    /// Squared distance from the query point to the item.
    pub distance_squared: f32,
}

/// A k-d tree: a set of points arranged so that the **nearest** points to
/// any position can be found quickly, in memory provided by the caller.
///
/// It needs no memory of its own. The tree is the caller's slice of
/// `(position, value)` pairs, reordered in place: the middle item of every
/// range splits the rest into a lower and an upper half along one axis, and
/// the axis cycles with depth.
///
/// The tree is built in one go, not edited. Build again when the points
/// change.
///
/// ```
/// use vas::KdTree;
///
/// let mut items = [
///     ([0.0, 0.0], "well"),
///     ([8.0, 1.0], "forge"),
///     ([3.0, 7.0], "inn"),
/// ];
/// let tree = KdTree::build(&mut items);
///
/// let nearest = tree.nearest(&[7.0, 2.0]).and_then(|found| tree.get(found.index));
/// assert_eq!(nearest.map(|(_, name)| *name), Some("forge"));
/// ```
pub struct KdTree<'a, T, const D: usize> {
    // Invariant: for every range the build visited (the whole slice, then
    // recursively the parts before and after each middle item), no item
    // before the middle is greater, and no item after it is smaller, than
    // the middle item along the range's axis (`depth % D`, by `total_cmp`).
    items: &'a mut [([f32; D], T)],
}

impl<'a, T, const D: usize> KdTree<'a, T, D> {
    /// Arranges `items` into a tree. O(n log n), no extra memory.
    pub fn build(items: &'a mut [([f32; D], T)]) -> Self {
        arrange(items, 0);
        Self { items }
    }

    /// Number of points in the tree.
    #[inline]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when the tree holds no points.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The points, in the order the tree arranged them.
    #[inline]
    pub const fn items(&self) -> &[([f32; D], T)] {
        self.items
    }

    /// The position and value of the item at `index`.
    #[inline]
    pub fn get(&self, index: usize) -> Option<(&[f32; D], &T)> {
        self.items
            .get(index)
            .map(|(position, value)| (position, value))
    }

    /// The value of the item at `index`, mutably. Positions cannot be
    /// changed; build again for that.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.items.get_mut(index).map(|(_, value)| value)
    }

    /// The point nearest to `target`, or `None` when the tree is empty.
    /// When several are equally near, any one of them is returned.
    pub fn nearest(&self, target: &[f32; D]) -> Option<Neighbor> {
        let mut best = Neighbor {
            index: usize::MAX,
            distance_squared: f32::INFINITY,
        };
        self.search(0, self.items.len(), 0, target, &mut |index, distance| {
            if distance < best.distance_squared {
                best = Neighbor {
                    index,
                    distance_squared: distance,
                };
            }
            best.distance_squared
        });
        (best.index != usize::MAX).then_some(best)
    }

    /// Finds the points nearest to `target` and writes them to `out`,
    /// nearest first. `out` is cleared first, and its capacity is how many
    /// are wanted.
    pub fn nearest_k(&self, target: &[f32; D], out: &mut Vector<'_, Neighbor>) {
        out.clear();
        if out.capacity() == 0 {
            return;
        }
        self.search(0, self.items.len(), 0, target, &mut |index, distance| {
            let worst = match out.last() {
                Some(last) if out.is_full() => last.distance_squared,
                _ => f32::INFINITY,
            };
            if distance < worst {
                // Keep `out` sorted: drop the farthest when full, then put
                // the new one in its place.
                if out.is_full() {
                    out.pop();
                }
                let position = out.partition_point(|kept| kept.distance_squared <= distance);
                let neighbor = Neighbor {
                    index,
                    distance_squared: distance,
                };
                if out.insert(position, neighbor).is_err() {
                    return worst;
                }
            }
            match out.last() {
                Some(last) if out.is_full() => last.distance_squared,
                _ => f32::INFINITY,
            }
        });
    }

    /// Calls `visit(position, value)` for every point inside `area`
    /// (boundary included). The order is unspecified.
    pub fn query(&self, area: &Aabb<D>, mut visit: impl FnMut(&[f32; D], &T)) {
        self.range(0, self.items.len(), 0, area, &mut |position, value| {
            if area.contains_point(position) {
                visit(position, value);
            }
        });
    }

    /// Calls `visit(position, value)` for every point within `radius` of
    /// `center` (boundary included). The order is unspecified.
    pub fn query_radius(
        &self,
        center: &[f32; D],
        radius: f32,
        mut visit: impl FnMut(&[f32; D], &T),
    ) {
        if radius.is_nan() || radius < 0.0 {
            return;
        }
        let limit = radius * radius;
        let area = Aabb::around(*center, radius);
        self.range(0, self.items.len(), 0, &area, &mut |position, value| {
            if distance_squared(position, center) <= limit {
                visit(position, value);
            }
        });
    }

    /// Nearest-neighbour search over `items[low..high]`. `consider(index,
    /// distance_squared)` is told about each candidate and returns the
    /// distance beyond which nothing is of interest any more.
    ///
    /// Recursion is as deep as the tree: about log2(n).
    fn search<F>(&self, low: usize, high: usize, depth: usize, target: &[f32; D], consider: &mut F)
    where
        F: FnMut(usize, f32) -> f32,
    {
        if low >= high {
            return;
        }
        let middle = low + (high - low) / 2;
        let Some((position, _)) = self.items.get(middle) else {
            return;
        };
        let mut limit = consider(middle, distance_squared(position, target));

        // How far the target is from the splitting plane, and on which side.
        let offset = axis_of(depth, D)
            .and_then(|axis| Some(target.get(axis)? - position.get(axis)?))
            .unwrap_or(f32::NAN);
        let (near, far) = if offset < 0.0 {
            ((low, middle), (middle + 1, high))
        } else {
            ((middle + 1, high), (low, middle))
        };
        self.search(near.0, near.1, depth + 1, target, consider);
        // The far side can only help if the plane is closer than the
        // current limit. A NaN offset proves nothing, so then it is searched.
        limit = limit.min(consider(usize::MAX, f32::INFINITY));
        let too_far = offset * offset >= limit;
        if !too_far {
            self.search(far.0, far.1, depth + 1, target, consider);
        }
    }

    /// Visits every point of `items[low..high]` that could lie in `area`.
    fn range<F>(&self, low: usize, high: usize, depth: usize, area: &Aabb<D>, visit: &mut F)
    where
        F: FnMut(&[f32; D], &T),
    {
        if low >= high {
            return;
        }
        let middle = low + (high - low) / 2;
        let Some((position, value)) = self.items.get(middle) else {
            return;
        };
        visit(position, value);
        let axis = axis_of(depth, D);
        let split = axis.and_then(|axis| position.get(axis)).copied();
        let area_low = axis.and_then(|axis| area.min.get(axis)).copied();
        let area_high = axis.and_then(|axis| area.max.get(axis)).copied();
        // Skip a side only when it is certainly outside the area; anything
        // involving NaN is not certain.
        let skip_lower = matches!((area_low, split), (Some(low), Some(split)) if low > split);
        let skip_upper = matches!((area_high, split), (Some(high), Some(split)) if high < split);
        if !skip_lower {
            self.range(low, middle, depth + 1, area, visit);
        }
        if !skip_upper {
            self.range(middle + 1, high, depth + 1, area, visit);
        }
    }
}

/// The splitting axis at `depth`, or `None` in zero dimensions.
#[inline]
fn axis_of(depth: usize, dimensions: usize) -> Option<usize> {
    depth.checked_rem(dimensions)
}

/// Puts the median of `items` along the axis for `depth` in the middle, with
/// smaller items before it and greater ones after, then does the same for
/// both halves. Recursion is about log2(n) deep.
fn arrange<T, const D: usize>(items: &mut [([f32; D], T)], depth: usize) {
    let Some(axis) = axis_of(depth, D) else {
        return;
    };
    if items.len() < 2 {
        return;
    }
    let middle = items.len() / 2;
    let coordinate = |item: &([f32; D], T)| item.0.get(axis).copied().unwrap_or(0.0);
    let (lower, _, upper) =
        items.select_nth_unstable_by(middle, |a, b| coordinate(a).total_cmp(&coordinate(b)));
    arrange(lower, depth + 1);
    arrange(upper, depth + 1);
}
