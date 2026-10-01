//! Units and dimensioned quantities.
//!
//! Canonical internal units (ADR-0002): length = millimetre, angle = radian,
//! time = second. Quantities carry a [`Dim`] so that seconds, metres and degrees
//! can never be added silently.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::{GeoResult, GeometryError};

/// Length units with exact conversion factors to millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LengthUnit {
    /// Millimetre (canonical).
    #[default]
    Millimetre,
    /// Centimetre.
    Centimetre,
    /// Metre.
    Metre,
    /// International inch (25.4 mm).
    Inch,
    /// International foot (304.8 mm).
    Foot,
}

impl LengthUnit {
    /// All units.
    pub const ALL: [Self; 5] = [Self::Millimetre, Self::Centimetre, Self::Metre, Self::Inch, Self::Foot];

    /// Millimetres per unit.
    #[must_use]
    pub const fn mm_per_unit(self) -> f64 {
        match self {
            Self::Millimetre => 1.0,
            Self::Centimetre => 10.0,
            Self::Metre => 1000.0,
            Self::Inch => 25.4,
            Self::Foot => 304.8,
        }
    }

    /// Convert a value in this unit to millimetres.
    #[must_use]
    pub fn to_mm(self, v: f64) -> f64 {
        v * self.mm_per_unit()
    }

    /// Convert millimetres to this unit.
    #[must_use]
    pub fn from_mm(self, mm: f64) -> f64 {
        mm / self.mm_per_unit()
    }

    /// Unit symbol.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Millimetre => "mm",
            Self::Centimetre => "cm",
            Self::Metre => "m",
            Self::Inch => "in",
            Self::Foot => "ft",
        }
    }

    /// Parse a unit symbol.
    pub fn parse(s: &str) -> GeoResult<Self> {
        match s.trim() {
            "mm" => Ok(Self::Millimetre),
            "cm" => Ok(Self::Centimetre),
            "m" => Ok(Self::Metre),
            "in" | "\"" => Ok(Self::Inch),
            "ft" | "'" => Ok(Self::Foot),
            other => Err(GeometryError::UnknownUnit(other.to_owned())),
        }
    }
}

/// Angle units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AngleUnit {
    /// Radian (canonical).
    #[default]
    Radian,
    /// Degree.
    Degree,
}

impl AngleUnit {
    /// Radians per unit.
    #[must_use]
    pub const fn rad_per_unit(self) -> f64 {
        match self {
            Self::Radian => 1.0,
            Self::Degree => core::f64::consts::PI / 180.0,
        }
    }

    /// Unit symbol.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Radian => "rad",
            Self::Degree => "deg",
        }
    }
}

/// Time units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimeUnit {
    /// Millisecond.
    Millisecond,
    /// Second (canonical).
    #[default]
    Second,
    /// Minute.
    Minute,
    /// Hour.
    Hour,
    /// Day (86 400 s).
    Day,
}

impl TimeUnit {
    /// Seconds per unit.
    #[must_use]
    pub const fn s_per_unit(self) -> f64 {
        match self {
            Self::Millisecond => 0.001,
            Self::Second => 1.0,
            Self::Minute => 60.0,
            Self::Hour => 3600.0,
            Self::Day => 86_400.0,
        }
    }

    /// Unit symbol.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Millisecond => "ms",
            Self::Second => "s",
            Self::Minute => "min",
            Self::Hour => "h",
            Self::Day => "d",
        }
    }
}

/// Physical dimension as exponents of length, angle and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Dim {
    /// Length exponent.
    #[serde(default, rename = "L", skip_serializing_if = "is_zero")]
    pub length: i8,
    /// Angle exponent.
    #[serde(default, rename = "A", skip_serializing_if = "is_zero")]
    pub angle: i8,
    /// Time exponent.
    #[serde(default, rename = "T", skip_serializing_if = "is_zero")]
    pub time: i8,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &i8) -> bool {
    *v == 0
}

impl Dim {
    /// Dimensionless.
    pub const SCALAR: Self = Self { length: 0, angle: 0, time: 0 };
    /// Length.
    pub const LENGTH: Self = Self { length: 1, angle: 0, time: 0 };
    /// Area.
    pub const AREA: Self = Self { length: 2, angle: 0, time: 0 };
    /// Angle.
    pub const ANGLE: Self = Self { length: 0, angle: 1, time: 0 };
    /// Time.
    pub const TIME: Self = Self { length: 0, angle: 0, time: 1 };

    /// Dimension of a product.
    #[must_use]
    pub const fn mul(self, o: Self) -> Self {
        Self {
            length: self.length.saturating_add(o.length),
            angle: self.angle.saturating_add(o.angle),
            time: self.time.saturating_add(o.time),
        }
    }

    /// Dimension of a quotient.
    #[must_use]
    pub const fn div(self, o: Self) -> Self {
        Self {
            length: self.length.saturating_sub(o.length),
            angle: self.angle.saturating_sub(o.angle),
            time: self.time.saturating_sub(o.time),
        }
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::SCALAR {
            return f.write_str("scalar");
        }
        let mut first = true;
        for (name, e) in [("length", self.length), ("angle", self.angle), ("time", self.time)] {
            if e != 0 {
                if !first {
                    f.write_str("·")?;
                }
                first = false;
                if e == 1 {
                    write!(f, "{name}")?;
                } else {
                    write!(f, "{name}^{e}")?;
                }
            }
        }
        Ok(())
    }
}

/// A value with a physical dimension, stored in canonical units (mm, rad, s).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    /// Value in canonical units.
    pub value: f64,
    /// Dimension.
    pub dim: Dim,
}

impl Quantity {
    /// Dimensionless number.
    #[must_use]
    pub const fn scalar(v: f64) -> Self {
        Self { value: v, dim: Dim::SCALAR }
    }

    /// Length from a value in `unit`.
    #[must_use]
    pub fn length(v: f64, unit: LengthUnit) -> Self {
        Self { value: unit.to_mm(v), dim: Dim::LENGTH }
    }

    /// Angle from a value in `unit`.
    #[must_use]
    pub fn angle(v: f64, unit: AngleUnit) -> Self {
        Self { value: v * unit.rad_per_unit(), dim: Dim::ANGLE }
    }

    /// Time from a value in `unit`.
    #[must_use]
    pub fn time(v: f64, unit: TimeUnit) -> Self {
        Self { value: v * unit.s_per_unit(), dim: Dim::TIME }
    }

    fn same_dim(self, o: Self) -> GeoResult<()> {
        if self.dim == o.dim {
            Ok(())
        } else {
            Err(GeometryError::DimensionMismatch { left: self.dim.to_string(), right: o.dim.to_string() })
        }
    }

    /// Checked addition (dimensions must match).
    pub fn checked_add(self, o: Self) -> GeoResult<Self> {
        self.same_dim(o)?;
        Ok(Self { value: self.value + o.value, dim: self.dim })
    }

    /// Checked subtraction.
    pub fn checked_sub(self, o: Self) -> GeoResult<Self> {
        self.same_dim(o)?;
        Ok(Self { value: self.value - o.value, dim: self.dim })
    }

    /// Product (dimensions multiply).
    #[must_use]
    pub fn times(self, o: Self) -> Self {
        Self { value: self.value * o.value, dim: self.dim.mul(o.dim) }
    }

    /// Quotient (dimensions divide).
    #[must_use]
    pub fn per(self, o: Self) -> Self {
        Self { value: self.value / o.value, dim: self.dim.div(o.dim) }
    }

    /// Parse strings like `"60 cm"`, `"2.5m"`, `"90 deg"`, `"15 min"`, `"3"`.
    pub fn parse(s: &str) -> GeoResult<Self> {
        let s = s.trim();
        let split = s
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .map_or(s.len(), |(i, _)| i);
        // Guard against "e" being taken as a unit start for inputs like "1e3".
        let (num, unit) = s.split_at(split);
        let v: f64 = num.trim().parse().map_err(|_| GeometryError::InvalidArgument("not a number"))?;
        if !v.is_finite() {
            return Err(GeometryError::NonFinite("quantity"));
        }
        let unit = unit.trim();
        if unit.is_empty() {
            return Ok(Self::scalar(v));
        }
        if let Ok(u) = LengthUnit::parse(unit) {
            return Ok(Self::length(v, u));
        }
        match unit {
            "deg" | "°" => Ok(Self::angle(v, AngleUnit::Degree)),
            "rad" => Ok(Self::angle(v, AngleUnit::Radian)),
            "ms" => Ok(Self::time(v, TimeUnit::Millisecond)),
            "s" => Ok(Self::time(v, TimeUnit::Second)),
            "min" => Ok(Self::time(v, TimeUnit::Minute)),
            "h" => Ok(Self::time(v, TimeUnit::Hour)),
            "d" => Ok(Self::time(v, TimeUnit::Day)),
            other => Err(GeometryError::UnknownUnit(other.to_owned())),
        }
    }
}

/// Explicit mapping from a domain time axis to model X coordinates (timelines).
///
/// `x_mm = (t_seconds - origin_s) * mm_per_second`. The mapping is the only place
/// where time becomes length; constraints work on time-dimensioned variables.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TimeAxis {
    /// Domain time shown at `x = 0`.
    pub origin_s: f64,
    /// Model millimetres per second.
    pub mm_per_second: f64,
}

impl TimeAxis {
    /// Create a validated axis.
    pub fn new(origin_s: f64, mm_per_second: f64) -> GeoResult<Self> {
        if !(origin_s.is_finite() && mm_per_second.is_finite()) {
            return Err(GeometryError::NonFinite("time axis"));
        }
        if mm_per_second <= 0.0 {
            return Err(GeometryError::InvalidArgument("time axis scale must be > 0"));
        }
        Ok(Self { origin_s, mm_per_second })
    }

    /// Model X for a time quantity.
    pub fn to_x(self, t: Quantity) -> GeoResult<f64> {
        if t.dim != Dim::TIME {
            return Err(GeometryError::DimensionMismatch { left: t.dim.to_string(), right: Dim::TIME.to_string() });
        }
        Ok((t.value - self.origin_s) * self.mm_per_second)
    }

    /// Time at model X.
    #[must_use]
    pub fn time_at(self, x_mm: f64) -> Quantity {
        Quantity { value: self.origin_s + x_mm / self.mm_per_second, dim: Dim::TIME }
    }

    /// Model width of a duration.
    pub fn width_of(self, duration: Quantity) -> GeoResult<f64> {
        if duration.dim != Dim::TIME {
            return Err(GeometryError::DimensionMismatch {
                left: duration.dim.to_string(),
                right: Dim::TIME.to_string(),
            });
        }
        Ok(duration.value * self.mm_per_second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_conversions_roundtrip() {
        for u in LengthUnit::ALL {
            let v = 123.456;
            assert!((u.from_mm(u.to_mm(v)) - v).abs() < 1e-12);
        }
        assert_eq!(LengthUnit::Centimetre.to_mm(60.0), 600.0);
        assert_eq!(LengthUnit::Metre.to_mm(1.8), 1800.0);
        assert!((LengthUnit::Inch.to_mm(1.0) - 25.4).abs() < 1e-15);
        assert!((LengthUnit::Foot.from_mm(304.8) - 1.0).abs() < 1e-15);
    }

    #[test]
    fn mixed_dimensions_do_not_add() {
        let a = Quantity::parse("2 m").unwrap();
        let b = Quantity::parse("30 s").unwrap();
        let c = Quantity::parse("90 deg").unwrap();
        assert!(matches!(a.checked_add(b), Err(GeometryError::DimensionMismatch { .. })));
        assert!(a.checked_add(c).is_err());
        assert!(b.checked_sub(c).is_err());
        let ok = a.checked_add(Quantity::parse("50 cm").unwrap()).unwrap();
        assert_eq!(ok.value, 2500.0);
        let area = a.times(a);
        assert_eq!(area.dim, Dim::AREA);
        let speed = a.per(b);
        assert_eq!(speed.dim.to_string(), "length·time^-1");
    }

    #[test]
    fn parse_variants() {
        assert_eq!(Quantity::parse("60cm").unwrap().value, 600.0);
        assert_eq!(Quantity::parse(" 3 ").unwrap(), Quantity::scalar(3.0));
        assert!((Quantity::parse("180 deg").unwrap().value - core::f64::consts::PI).abs() < 1e-15);
        assert_eq!(Quantity::parse("15 min").unwrap().value, 900.0);
        assert!(Quantity::parse("5 parsec").is_err());
        assert!(Quantity::parse("abc").is_err());
        assert!(Quantity::parse("1e400 mm").is_err());
    }

    #[test]
    fn time_axis_is_explicit() {
        let axis = TimeAxis::new(3600.0, 2.0).unwrap();
        let x = axis.to_x(Quantity::time(2.0, TimeUnit::Hour)).unwrap();
        assert_eq!(x, 7200.0);
        assert!(axis.to_x(Quantity::length(5.0, LengthUnit::Metre)).is_err());
        assert_eq!(axis.time_at(7200.0).value, 7200.0);
        assert!(TimeAxis::new(0.0, 0.0).is_err());
    }
}
