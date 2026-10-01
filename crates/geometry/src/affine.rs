use serde::{Deserialize, Serialize};

use crate::{GeoResult, GeometryError, Point, Vector};

/// A 2D affine transform `[a b c d e f]` with the SVG convention:
///
/// ```text
/// x' = a·x + c·y + e
/// y' = b·x + d·y + f
/// ```
///
/// Serialized as a 6-element array.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(from = "[f64; 6]", into = "[f64; 6]")]
pub struct Affine {
    /// Coefficients `[a, b, c, d, e, f]`.
    pub m: [f64; 6],
}

impl From<[f64; 6]> for Affine {
    fn from(m: [f64; 6]) -> Self {
        Self { m }
    }
}
impl From<Affine> for [f64; 6] {
    fn from(a: Affine) -> Self {
        a.m
    }
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Classification of the linear part of a transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearKind {
    /// Rotation + uniform scale `s` (`reflected` when the determinant is negative).
    Similarity {
        /// Uniform scale factor (> 0).
        scale: f64,
        /// Whether orientation is reversed.
        reflected: bool,
    },
    /// Anything else (non-uniform scale or shear).
    General,
    /// Determinant is (near) zero.
    Singular,
}

impl Affine {
    /// Determinants with magnitude below this are treated as singular.
    pub const SINGULAR_EPS: f64 = 1e-14;

    /// The identity transform.
    pub const IDENTITY: Self = Self { m: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] };

    /// Translation by `v`.
    #[must_use]
    pub const fn translate(v: Vector) -> Self {
        Self { m: [1.0, 0.0, 0.0, 1.0, v.x, v.y] }
    }

    /// Counter-clockwise rotation by `angle` radians about the origin.
    #[must_use]
    pub fn rotate(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Self { m: [c, s, -s, c, 0.0, 0.0] }
    }

    /// Rotation about `center`.
    #[must_use]
    pub fn rotate_about(angle: f64, center: Point) -> Self {
        Self::translate(-center.to_vector()).then(Self::rotate(angle)).then(Self::translate(center.to_vector()))
    }

    /// Scale about the origin.
    #[must_use]
    pub const fn scale(sx: f64, sy: f64) -> Self {
        Self { m: [sx, 0.0, 0.0, sy, 0.0, 0.0] }
    }

    /// Scale about `center`.
    #[must_use]
    pub fn scale_about(sx: f64, sy: f64, center: Point) -> Self {
        Self::translate(-center.to_vector()).then(Self::scale(sx, sy)).then(Self::translate(center.to_vector()))
    }

    /// Reflection across the line through `p` with direction `dir`.
    pub fn mirror(p: Point, dir: Vector) -> GeoResult<Self> {
        let d = dir.normalize().ok_or(GeometryError::Degenerate("mirror axis direction"))?;
        let (x, y) = (d.x, d.y);
        let lin = Self { m: [x * x - y * y, 2.0 * x * y, 2.0 * x * y, y * y - x * x, 0.0, 0.0] };
        Ok(Self::translate(-p.to_vector()).then(lin).then(Self::translate(p.to_vector())))
    }

    /// Composition: apply `self` first, then `next`.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        let [a1, b1, c1, d1, e1, f1] = self.m;
        let [a2, b2, c2, d2, e2, f2] = next.m;
        Self {
            m: [
                a2 * a1 + c2 * b1,
                b2 * a1 + d2 * b1,
                a2 * c1 + c2 * d1,
                b2 * c1 + d2 * d1,
                a2 * e1 + c2 * f1 + e2,
                b2 * e1 + d2 * f1 + f2,
            ],
        }
    }

    /// Determinant of the linear part.
    #[must_use]
    pub fn determinant(self) -> f64 {
        self.m[0] * self.m[3] - self.m[1] * self.m[2]
    }

    /// All coefficients are finite.
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.m.iter().all(|v| v.is_finite())
    }

    /// Whether this is exactly the identity.
    #[must_use]
    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// Inverse transform. Fails for singular or non-finite transforms.
    pub fn inverse(self) -> GeoResult<Self> {
        if !self.is_finite() {
            return Err(GeometryError::NonFinite("transform"));
        }
        let det = self.determinant();
        let [a, b, c, d, e, f] = self.m;
        // Relative test: |det| compared with the squared column norms, so tiny but
        // well-conditioned scales stay invertible.
        let scale = (a * a + b * b).max(c * c + d * d);
        if det == 0.0 || !det.is_finite() || det.abs() <= Self::SINGULAR_EPS * scale {
            return Err(GeometryError::SingularTransform { determinant: det });
        }
        let inv = 1.0 / det;
        let na = d * inv;
        let nb = -b * inv;
        let nc = -c * inv;
        let nd = a * inv;
        Ok(Self { m: [na, nb, nc, nd, -(na * e + nc * f), -(nb * e + nd * f)] })
    }

    /// Transform a point.
    #[must_use]
    pub fn apply(self, p: Point) -> Point {
        let [a, b, c, d, e, f] = self.m;
        Point::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// Transform a vector (ignores translation).
    #[must_use]
    pub fn apply_vector(self, v: Vector) -> Vector {
        let [a, b, c, d, _, _] = self.m;
        Vector::new(a * v.x + c * v.y, b * v.x + d * v.y)
    }

    /// Translation part.
    #[must_use]
    pub fn translation(self) -> Vector {
        Vector::new(self.m[4], self.m[5])
    }

    /// Classify the linear part with relative tolerance `rel`.
    #[must_use]
    pub fn linear_kind(self, rel: f64) -> LinearKind {
        let [a, b, c, d, _, _] = self.m;
        let det = a * d - b * c;
        let n1 = a * a + b * b;
        let n2 = c * c + d * d;
        let scale = n1.max(n2);
        if !det.is_finite() || det.abs() <= Self::SINGULAR_EPS * scale.max(f64::MIN_POSITIVE) {
            return LinearKind::Singular;
        }
        let ortho = (a * c + b * d).abs();
        if (n1 - n2).abs() <= rel * scale && ortho <= rel * scale {
            LinearKind::Similarity { scale: ((n1 + n2) * 0.5).sqrt(), reflected: det < 0.0 }
        } else {
            LinearKind::General
        }
    }

    /// Rotation angle of the linear part (meaningful for similarities).
    #[must_use]
    pub fn rotation_angle(self) -> f64 {
        self.m[1].atan2(self.m[0])
    }

    /// Whether the linear part is a diagonal (axis aligned) scale with positive or
    /// negative factors: no rotation or shear.
    #[must_use]
    pub fn is_axis_aligned(self, rel: f64) -> bool {
        let [a, b, c, d, _, _] = self.m;
        let scale = a.abs().max(d.abs()).max(f64::MIN_POSITIVE);
        b.abs() <= rel * scale && c.abs() <= rel * scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::FRAC_PI_2;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn compose_order() {
        let t = Affine::translate(Vector::new(10.0, 0.0)).then(Affine::rotate(FRAC_PI_2));
        // translate first, then rotate: (1,0) -> (11,0) -> (0,11)
        assert!(close(t.apply(Point::new(1.0, 0.0)), Point::new(0.0, 11.0)));
    }

    #[test]
    fn inverse_roundtrip() {
        let t = Affine::rotate_about(0.3, Point::new(5.0, -2.0))
            .then(Affine::scale(2.0, 0.5))
            .then(Affine::translate(Vector::new(3.0, 4.0)));
        let inv = t.inverse().unwrap();
        let p = Point::new(7.25, -1.5);
        assert!(close(inv.apply(t.apply(p)), p));
    }

    #[test]
    fn singular_is_rejected() {
        let t = Affine::scale(1.0, 0.0);
        assert!(matches!(t.inverse(), Err(GeometryError::SingularTransform { .. })));
        let nan = Affine { m: [f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0] };
        assert!(matches!(nan.inverse(), Err(GeometryError::NonFinite(_))));
    }

    #[test]
    fn classify() {
        assert!(matches!(
            Affine::rotate(0.7).then(Affine::scale(3.0, 3.0)).linear_kind(1e-12),
            LinearKind::Similarity { scale, reflected: false } if (scale - 3.0).abs() < 1e-12
        ));
        assert_eq!(Affine::scale(2.0, 1.0).linear_kind(1e-12), LinearKind::General);
        assert!(matches!(
            Affine::mirror(Point::ORIGIN, Vector::new(1.0, 1.0)).unwrap().linear_kind(1e-12),
            LinearKind::Similarity { reflected: true, .. }
        ));
        assert_eq!(Affine::scale(0.0, 1.0).linear_kind(1e-12), LinearKind::Singular);
    }

    #[test]
    fn mirror_reflects() {
        let m = Affine::mirror(Point::new(0.0, 1.0), Vector::new(1.0, 0.0)).unwrap();
        assert!(close(m.apply(Point::new(3.0, 3.0)), Point::new(3.0, -1.0)));
    }
}
