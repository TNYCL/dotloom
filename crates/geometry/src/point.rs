use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

/// A position in the plane (model units, `f64`). Serialized as `[x, y]`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "[f64; 2]", into = "[f64; 2]")]
pub struct Point {
    /// X coordinate.
    pub x: f64,
    /// Y coordinate (Y grows "up" in model space; views flip it).
    pub y: f64,
}

/// A displacement in the plane. Serialized as `[x, y]`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "[f64; 2]", into = "[f64; 2]")]
pub struct Vector {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
}

impl From<[f64; 2]> for Point {
    fn from(v: [f64; 2]) -> Self {
        Self { x: v[0], y: v[1] }
    }
}
impl From<Point> for [f64; 2] {
    fn from(p: Point) -> Self {
        [p.x, p.y]
    }
}
impl From<[f64; 2]> for Vector {
    fn from(v: [f64; 2]) -> Self {
        Self { x: v[0], y: v[1] }
    }
}
impl From<Vector> for [f64; 2] {
    fn from(p: Vector) -> Self {
        [p.x, p.y]
    }
}

impl Point {
    /// The origin.
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    /// Create a point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Both coordinates are finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Euclidean distance.
    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        (other - self).length()
    }

    /// Squared Euclidean distance.
    #[must_use]
    pub fn distance_sq(self, other: Self) -> f64 {
        (other - self).length_sq()
    }

    /// Linear interpolation, `t = 0` gives `self`.
    #[must_use]
    pub fn lerp(self, other: Self, t: f64) -> Self {
        Self::new(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t)
    }

    /// Midpoint between two points.
    #[must_use]
    pub fn midpoint(self, other: Self) -> Self {
        Self::new((self.x + other.x) * 0.5, (self.y + other.y) * 0.5)
    }

    /// Vector from the origin to this point.
    #[must_use]
    pub const fn to_vector(self) -> Vector {
        Vector::new(self.x, self.y)
    }

    pub(crate) fn coord(self) -> robust::Coord<f64> {
        robust::Coord { x: self.x, y: self.y }
    }
}

impl Vector {
    /// Zero vector.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// Create a vector.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Unit vector at `angle` radians from +X, counter-clockwise.
    #[must_use]
    pub fn from_angle(angle: f64) -> Self {
        let (s, c) = crate::math::sin_cos(angle);
        Self::new(c, s)
    }

    /// Both components are finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Euclidean length (overflow-safe).
    #[must_use]
    pub fn length(self) -> f64 {
        crate::math::hypot(self.x, self.y)
    }

    /// Squared length.
    #[must_use]
    pub fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    /// Dot product.
    #[must_use]
    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// 2D cross product (z component of the 3D cross product).
    #[must_use]
    pub fn cross(self, o: Self) -> f64 {
        self.x * o.y - self.y * o.x
    }

    /// Unit vector in the same direction, or `None` for a zero/non-finite vector.
    #[must_use]
    pub fn normalize(self) -> Option<Self> {
        let len = self.length();
        if len > 0.0 && len.is_finite() { Some(Self::new(self.x / len, self.y / len)) } else { None }
    }

    /// Rotate 90° counter-clockwise.
    #[must_use]
    pub const fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Angle from +X in radians, in `(-π, π]`.
    #[must_use]
    pub fn angle(self) -> f64 {
        crate::math::atan2(self.y, self.x)
    }

    /// Rotate by `angle` radians counter-clockwise.
    #[must_use]
    pub fn rotate(self, angle: f64) -> Self {
        let (s, c) = crate::math::sin_cos(angle);
        Self::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    /// Point at the tip of this vector from the origin.
    #[must_use]
    pub const fn to_point(self) -> Point {
        Point::new(self.x, self.y)
    }
}

impl Sub for Point {
    type Output = Vector;
    fn sub(self, o: Self) -> Vector {
        Vector::new(self.x - o.x, self.y - o.y)
    }
}
impl Add<Vector> for Point {
    type Output = Self;
    fn add(self, v: Vector) -> Self {
        Self::new(self.x + v.x, self.y + v.y)
    }
}
impl AddAssign<Vector> for Point {
    fn add_assign(&mut self, v: Vector) {
        self.x += v.x;
        self.y += v.y;
    }
}
impl Sub<Vector> for Point {
    type Output = Self;
    fn sub(self, v: Vector) -> Self {
        Self::new(self.x - v.x, self.y - v.y)
    }
}
impl SubAssign<Vector> for Point {
    fn sub_assign(&mut self, v: Vector) {
        self.x -= v.x;
        self.y -= v.y;
    }
}
impl Add for Vector {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vector {
    fn add_assign(&mut self, o: Self) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl Sub for Vector {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
}
impl Neg for Vector {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}
impl Mul<f64> for Vector {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s)
    }
}
impl Mul<Vector> for f64 {
    type Output = Vector;
    fn mul(self, v: Vector) -> Vector {
        Vector::new(self * v.x, self * v.y)
    }
}
impl Div<f64> for Vector {
    type Output = Self;
    fn div(self, s: f64) -> Self {
        Self::new(self.x / s, self.y / s)
    }
}

/// Orientation of three points using an exact (adaptive-precision) predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// `c` lies to the left of `a → b`.
    CounterClockwise,
    /// `c` lies to the right of `a → b`.
    Clockwise,
    /// The three points are exactly collinear.
    Collinear,
}

/// Exact orientation test (Shewchuk's adaptive predicate via the `robust` crate).
#[must_use]
pub fn orientation(a: Point, b: Point, c: Point) -> Orientation {
    let d = robust::orient2d(a.coord(), b.coord(), c.coord());
    if d > 0.0 {
        Orientation::CounterClockwise
    } else if d < 0.0 {
        Orientation::Clockwise
    } else {
        Orientation::Collinear
    }
}

/// Normalize an angle into `[0, 2π)`.
#[must_use]
pub fn normalize_angle(a: f64) -> f64 {
    let tau = core::f64::consts::TAU;
    let r = a.rem_euclid(tau);
    if r >= tau { 0.0 } else { r }
}

/// Normalize an angle into `(-π, π]`.
#[must_use]
pub fn normalize_angle_signed(a: f64) -> f64 {
    let pi = core::f64::consts::PI;
    let r = normalize_angle(a);
    if r > pi { r - core::f64::consts::TAU } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_as_array() {
        let p = Point::new(1.5, -2.0);
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(s, "[1.5,-2.0]");
        let back: Point = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn orientation_is_exact_for_nearly_collinear_points() {
        // Classic failure case of naive floating point orientation.
        let a = Point::new(0.5, 0.5);
        let b = Point::new(12.0, 12.0);
        let c = Point::new(24.0, 24.0);
        assert_eq!(orientation(a, b, c), Orientation::Collinear);
        let c2 = Point::new(24.0, 24.000_000_000_000_004);
        assert_eq!(orientation(a, b, c2), Orientation::CounterClockwise);
    }

    #[test]
    fn angle_normalization() {
        use core::f64::consts::{PI, TAU};
        assert!((normalize_angle(-PI / 2.0) - 1.5 * PI).abs() < 1e-15);
        assert!(normalize_angle(TAU) < 1e-15);
        assert!((normalize_angle_signed(1.5 * PI) + PI / 2.0).abs() < 1e-15);
    }

    #[test]
    fn normalize_zero_is_none() {
        assert!(Vector::ZERO.normalize().is_none());
        assert!(Vector::new(f64::NAN, 1.0).normalize().is_none());
        assert!(Vector::new(f64::INFINITY, 1.0).normalize().is_none());
    }
}
