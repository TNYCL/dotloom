use serde::{Deserialize, Serialize};

use crate::{Affine, Point, Vector};

/// Axis-aligned bounding box. An empty box has `min > max`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    /// Minimum corner.
    pub min: Point,
    /// Maximum corner.
    pub max: Point,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    /// The empty box (identity for [`Aabb::union`]).
    pub const EMPTY: Self =
        Self { min: Point::new(f64::INFINITY, f64::INFINITY), max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY) };

    /// Box from two corners in any order.
    #[must_use]
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self { min: Point::new(a.x.min(b.x), a.y.min(b.y)), max: Point::new(a.x.max(b.x), a.y.max(b.y)) }
    }

    /// Smallest box containing all points (empty for no points).
    #[must_use]
    pub fn from_points<I: IntoIterator<Item = Point>>(points: I) -> Self {
        points.into_iter().fold(Self::EMPTY, Self::include)
    }

    /// Whether the box contains nothing.
    #[must_use]
    pub fn is_empty(self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }

    /// Grow to include `p` (non-finite points are ignored).
    #[must_use]
    pub fn include(self, p: Point) -> Self {
        if !p.is_finite() {
            return self;
        }
        Self {
            min: Point::new(self.min.x.min(p.x), self.min.y.min(p.y)),
            max: Point::new(self.max.x.max(p.x), self.max.y.max(p.y)),
        }
    }

    /// Union of two boxes.
    #[must_use]
    pub fn union(self, o: Self) -> Self {
        if o.is_empty() {
            return self;
        }
        if self.is_empty() {
            return o;
        }
        Self {
            min: Point::new(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: Point::new(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }

    /// Grow by `d` in every direction.
    #[must_use]
    pub fn inflate(self, d: f64) -> Self {
        if self.is_empty() {
            return self;
        }
        Self { min: Point::new(self.min.x - d, self.min.y - d), max: Point::new(self.max.x + d, self.max.y + d) }
    }

    /// Width (0 for empty).
    #[must_use]
    pub fn width(self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.x - self.min.x }
    }

    /// Height (0 for empty).
    #[must_use]
    pub fn height(self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.y - self.min.y }
    }

    /// Center point.
    #[must_use]
    pub fn center(self) -> Point {
        self.min.midpoint(self.max)
    }

    /// Size as a vector.
    #[must_use]
    pub fn size(self) -> Vector {
        Vector::new(self.width(), self.height())
    }

    /// Point inside or on the boundary.
    #[must_use]
    pub fn contains_point(self, p: Point) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// `o` lies completely inside `self`.
    #[must_use]
    pub fn contains(self, o: Self) -> bool {
        !o.is_empty()
            && !self.is_empty()
            && o.min.x >= self.min.x
            && o.max.x <= self.max.x
            && o.min.y >= self.min.y
            && o.max.y <= self.max.y
    }

    /// Boxes overlap (touching counts).
    #[must_use]
    pub fn intersects(self, o: Self) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
    }

    /// Distance from `p` to the box (0 inside).
    #[must_use]
    pub fn distance_to_point(self, p: Point) -> f64 {
        if self.is_empty() {
            return f64::INFINITY;
        }
        let dx = (self.min.x - p.x).max(0.0).max(p.x - self.max.x);
        let dy = (self.min.y - p.y).max(0.0).max(p.y - self.max.y);
        crate::math::hypot(dx, dy)
    }

    /// The four corners counter-clockwise from `min`.
    #[must_use]
    pub fn corners(self) -> [Point; 4] {
        [self.min, Point::new(self.max.x, self.min.y), self.max, Point::new(self.min.x, self.max.y)]
    }

    /// Bounding box of the transformed box.
    #[must_use]
    pub fn transform(self, t: Affine) -> Self {
        if self.is_empty() {
            return self;
        }
        Self::from_points(self.corners().map(|c| t.apply(c)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_behaviour() {
        assert!(Aabb::EMPTY.is_empty());
        let b = Aabb::from_corners(Point::new(1.0, 2.0), Point::new(-1.0, 0.0));
        assert_eq!(Aabb::EMPTY.union(b), b);
        assert_eq!(b.union(Aabb::EMPTY), b);
        assert!(!Aabb::EMPTY.intersects(b));
        assert_eq!(Aabb::from_points([]), Aabb::EMPTY);
    }

    #[test]
    fn distance() {
        let b = Aabb::from_corners(Point::ORIGIN, Point::new(2.0, 2.0));
        assert_eq!(b.distance_to_point(Point::new(1.0, 1.0)), 0.0);
        assert!((b.distance_to_point(Point::new(5.0, 6.0)) - 5.0).abs() < 1e-12);
    }
}
