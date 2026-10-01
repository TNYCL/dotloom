//! Dimension (measurement annotation) geometry.
//!
//! These functions compute the drawable pieces of a dimension from the measured
//! points. Values come from the exact `f64` model, never from flattened render data.

use core::f64::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use crate::{Arc, GeoResult, GeometryError, Point, Segment, Vector, normalize_angle};

/// Orientation of a linear dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinearKind {
    /// Parallel to the measured points.
    #[default]
    Aligned,
    /// Horizontal projection.
    Horizontal,
    /// Vertical projection.
    Vertical,
}

/// Visual parameters in model units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DimensionStyle {
    /// Gap between the measured point and the extension line start.
    pub extension_gap: f64,
    /// Extension line overshoot beyond the dimension line.
    pub extension_overshoot: f64,
    /// Arrow length.
    pub arrow_size: f64,
    /// Text height.
    pub text_height: f64,
}

impl Default for DimensionStyle {
    fn default() -> Self {
        Self { extension_gap: 1.0, extension_overshoot: 2.0, arrow_size: 3.0, text_height: 3.5 }
    }
}

/// An arrow head: tip and unit direction pointing at the tip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Arrow {
    /// Tip position.
    pub tip: Point,
    /// Unit direction of travel towards the tip.
    pub direction: Vector,
}

/// Drawable dimension pieces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionGeometry {
    /// Extension and dimension lines.
    pub lines: Vec<Segment>,
    /// Dimension arc (angular dimensions).
    pub arc: Option<Arc>,
    /// Arrow heads.
    pub arrows: Vec<Arrow>,
    /// Text anchor (center of the label).
    pub text_position: Point,
    /// Text rotation (kept readable: within `(-π/2, π/2]`).
    pub text_rotation: f64,
    /// Measured value: length in model units, or angle in radians.
    pub value: f64,
}

fn readable(angle: f64) -> f64 {
    let a = crate::normalize_angle_signed(angle);
    if a > PI / 2.0 + 1e-12 {
        a - PI
    } else if a <= -PI / 2.0 + 1e-12 {
        a + PI
    } else {
        a
    }
}

/// Linear dimension between `p1` and `p2`; the dimension line passes through `through`.
pub fn linear(
    p1: Point,
    p2: Point,
    through: Point,
    kind: LinearKind,
    style: DimensionStyle,
) -> GeoResult<DimensionGeometry> {
    if !(p1.is_finite() && p2.is_finite() && through.is_finite()) {
        return Err(GeometryError::NonFinite("dimension points"));
    }
    let u = match kind {
        LinearKind::Aligned => (p2 - p1).normalize().ok_or(GeometryError::Degenerate("dimension points coincide"))?,
        LinearKind::Horizontal => Vector::new(1.0, 0.0),
        LinearKind::Vertical => Vector::new(0.0, 1.0),
    };
    let n = u.perp();
    let d1 = (through - p1).dot(n);
    let d2 = (through - p2).dot(n);
    let q1 = p1 + n * d1;
    let q2 = p2 + n * d2;
    let value = (p2 - p1).dot(u).abs();
    if value == 0.0 {
        return Err(GeometryError::Degenerate("projected dimension length is zero"));
    }
    let ext = |p: Point, q: Point| -> Option<Segment> {
        let v = q - p;
        let len = v.length();
        let dir = v.normalize()?;
        (len > style.extension_gap)
            .then(|| Segment::new(p + dir * style.extension_gap, q + dir * style.extension_overshoot))
    };
    let mut lines: Vec<Segment> = [ext(p1, q1), ext(p2, q2)].into_iter().flatten().collect();
    lines.push(Segment::new(q1, q2));
    let along = (q2 - q1).normalize().unwrap_or(u);
    Ok(DimensionGeometry {
        lines,
        arc: None,
        arrows: vec![Arrow { tip: q1, direction: -along }, Arrow { tip: q2, direction: along }],
        // Label sits on the side of the dimension line away from the measured points.
        text_position: q1.midpoint(q2) + n * (style.text_height * 0.75 * if d1 < 0.0 { -1.0 } else { 1.0 }),
        text_rotation: readable(u.angle()),
        value,
    })
}

/// Angular dimension at `vertex` between rays through `a` and `b` (CCW from `a`),
/// with the dimension arc at distance `radius`.
pub fn angular(vertex: Point, a: Point, b: Point, radius: f64, style: DimensionStyle) -> GeoResult<DimensionGeometry> {
    let va = (a - vertex).normalize().ok_or(GeometryError::Degenerate("angular ray a"))?;
    let vb = (b - vertex).normalize().ok_or(GeometryError::Degenerate("angular ray b"))?;
    if !(radius.is_finite() && radius > 0.0) {
        return Err(GeometryError::InvalidArgument("angular dimension radius must be > 0"));
    }
    let start = va.angle();
    let sweep = normalize_angle(vb.angle() - start);
    if sweep == 0.0 {
        return Err(GeometryError::Degenerate("angular rays coincide"));
    }
    let arc = Arc::new(vertex, radius, start, sweep)?;
    let mid_dir = Vector::from_angle(start + sweep * 0.5);
    Ok(DimensionGeometry {
        lines: vec![
            Segment::new(
                vertex + va * style.extension_gap.min(radius * 0.5),
                vertex + va * (radius + style.extension_overshoot),
            ),
            Segment::new(
                vertex + vb * style.extension_gap.min(radius * 0.5),
                vertex + vb * (radius + style.extension_overshoot),
            ),
        ],
        arc: Some(arc),
        arrows: vec![
            Arrow { tip: arc.start_point(), direction: -arc.tangent_at(0.0) },
            Arrow { tip: arc.end_point(), direction: arc.tangent_at(1.0) },
        ],
        text_position: vertex + mid_dir * (radius + style.text_height),
        text_rotation: readable(mid_dir.angle() - PI / 2.0),
        value: sweep.min(TAU),
    })
}

/// Radial dimension from `center` to the circle at angle `angle`.
pub fn radial(center: Point, radius: f64, angle: f64, style: DimensionStyle) -> GeoResult<DimensionGeometry> {
    if !(radius.is_finite() && radius > 0.0 && center.is_finite() && angle.is_finite()) {
        return Err(GeometryError::InvalidArgument("radial dimension input"));
    }
    let dir = Vector::from_angle(angle);
    let tip = center + dir * radius;
    Ok(DimensionGeometry {
        lines: vec![Segment::new(center, tip)],
        arc: None,
        arrows: vec![Arrow { tip, direction: dir }],
        text_position: center + dir * (radius * 0.5) + dir.perp() * (style.text_height * 0.75),
        text_rotation: readable(angle),
        value: radius,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_dimension_value_and_offset() {
        let d = linear(
            Point::new(0.0, 0.0),
            Point::new(30.0, 40.0),
            Point::new(-8.0, 6.0),
            LinearKind::Aligned,
            DimensionStyle::default(),
        )
        .unwrap();
        assert!((d.value - 50.0).abs() < 1e-12);
        let dim_line = d.lines.last().unwrap();
        // Dimension line is parallel to the measured points and offset by 10.
        assert!((dim_line.length() - 50.0).abs() < 1e-12);
        assert!((dim_line.a.distance(Point::ORIGIN) - 10.0).abs() < 1e-12);
    }

    #[test]
    fn horizontal_projection() {
        let d = linear(
            Point::new(0.0, 0.0),
            Point::new(30.0, 40.0),
            Point::new(0.0, 60.0),
            LinearKind::Horizontal,
            DimensionStyle::default(),
        )
        .unwrap();
        assert!((d.value - 30.0).abs() < 1e-12);
        assert!((d.text_rotation).abs() < 1e-12);
    }

    #[test]
    fn text_stays_readable() {
        let d = linear(
            Point::new(10.0, 0.0),
            Point::new(0.0, 0.0),
            Point::new(5.0, 5.0),
            LinearKind::Aligned,
            DimensionStyle::default(),
        )
        .unwrap();
        assert!(d.text_rotation.abs() < 1e-12, "{}", d.text_rotation);
    }

    #[test]
    fn angular_value() {
        let d =
            angular(Point::ORIGIN, Point::new(1.0, 0.0), Point::new(0.0, 2.0), 5.0, DimensionStyle::default()).unwrap();
        assert!((d.value - PI / 2.0).abs() < 1e-12);
        assert!(d.arc.is_some());
    }
}
