//! Axis-aligned boxes in any number of dimensions.

/// An axis-aligned bounding box in `D` dimensions: a rectangle for `D = 2`,
/// a box for `D = 3`.
///
/// Points are plain `[f32; D]` arrays, so any math library's vectors convert
/// with no cost. Edges are part of the box: a point on the boundary is
/// inside, and boxes that only touch do intersect.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Aabb<const D: usize> {
    /// The corner with the smallest coordinate on every axis.
    pub min: [f32; D],
    /// The corner with the greatest coordinate on every axis.
    pub max: [f32; D],
}

impl<const D: usize> Aabb<D> {
    /// The box spanned by two opposite corners, given in any order.
    #[inline]
    pub fn new(a: [f32; D], b: [f32; D]) -> Self {
        let mut min = a;
        let mut max = b;
        for (low, high) in min.iter_mut().zip(max.iter_mut()) {
            if *high < *low {
                core::mem::swap(low, high);
            }
        }
        Self { min, max }
    }

    /// The box containing only `point`.
    #[inline]
    pub const fn point(point: [f32; D]) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// The box reaching `radius` from `center` in every direction: the
    /// bounding box of a circle or sphere.
    #[inline]
    pub fn around(center: [f32; D], radius: f32) -> Self {
        Self::point(center).expanded(radius)
    }

    /// The middle of the box.
    #[inline]
    pub fn center(&self) -> [f32; D] {
        let mut center = self.min;
        for (value, high) in center.iter_mut().zip(self.max.iter()) {
            *value = (*value + *high) * 0.5;
        }
        center
    }

    /// The extent of the box along each axis.
    #[inline]
    pub fn size(&self) -> [f32; D] {
        let mut size = self.max;
        for (value, low) in size.iter_mut().zip(self.min.iter()) {
            *value -= *low;
        }
        size
    }

    /// `true` when `point` is inside the box or on its boundary.
    #[inline]
    pub fn contains_point(&self, point: &[f32; D]) -> bool {
        self.min
            .iter()
            .zip(self.max.iter())
            .zip(point.iter())
            .all(|((low, high), value)| low <= value && value <= high)
    }

    /// `true` when `other` lies entirely inside this box.
    #[inline]
    pub fn contains(&self, other: &Self) -> bool {
        self.contains_point(&other.min) && self.contains_point(&other.max)
    }

    /// `true` when the two boxes overlap or touch.
    #[inline]
    pub fn intersects(&self, other: &Self) -> bool {
        let below = self
            .min
            .iter()
            .zip(other.max.iter())
            .all(|(low, high)| low <= high);
        let above = other
            .min
            .iter()
            .zip(self.max.iter())
            .all(|(low, high)| low <= high);
        below && above
    }

    /// The smallest box containing both boxes.
    #[inline]
    pub fn union(&self, other: &Self) -> Self {
        self.grown(&other.min).grown(&other.max)
    }

    /// This box, enlarged just enough to contain `point`.
    #[inline]
    pub fn grown(&self, point: &[f32; D]) -> Self {
        let mut grown = *self;
        for ((low, high), value) in grown
            .min
            .iter_mut()
            .zip(grown.max.iter_mut())
            .zip(point.iter())
        {
            *low = low.min(*value);
            *high = high.max(*value);
        }
        grown
    }

    /// This box, pushed out by `margin` on every side.
    #[inline]
    pub fn expanded(&self, margin: f32) -> Self {
        let mut expanded = *self;
        for (low, high) in expanded.min.iter_mut().zip(expanded.max.iter_mut()) {
            *low -= margin;
            *high += margin;
        }
        expanded
    }

    /// Squared distance from `point` to the nearest point of the box; zero
    /// when the point is inside.
    #[inline]
    pub fn distance_squared(&self, point: &[f32; D]) -> f32 {
        self.min
            .iter()
            .zip(self.max.iter())
            .zip(point.iter())
            .map(|((low, high), value)| {
                let outside = (low - value).max(value - high).max(0.0);
                outside * outside
            })
            .sum()
    }

    /// Distance along a ray at which it enters the box, or `None` when the
    /// ray misses it within `max_distance`. Zero when the ray starts inside.
    ///
    /// The ray starts at `origin` and runs along `direction`, which need
    /// not be normalised: distances are in multiples of its length.
    pub fn ray(&self, origin: &[f32; D], direction: &[f32; D], max_distance: f32) -> Option<f32> {
        let mut enter = 0.0f32;
        let mut exit = max_distance;
        let axes = self
            .min
            .iter()
            .zip(self.max.iter())
            .zip(origin.iter().zip(direction.iter()));
        for ((low, high), (start, step)) in axes {
            if *step == 0.0 {
                // Parallel to this pair of faces: must already be between
                // them.
                if !(low <= start && start <= high) {
                    return None;
                }
                continue;
            }
            let (a, b) = ((low - start) / step, (high - start) / step);
            if a.is_nan() || b.is_nan() {
                return None;
            }
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
        }
        (enter <= exit).then_some(enter)
    }
}

/// Squared distance between two points.
#[inline]
pub(crate) fn distance_squared<const D: usize>(a: &[f32; D], b: &[f32; D]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum()
}
