//! Tolerance kinds.
//!
//! Dotloom keeps four tolerance concepts strictly apart (ADR-0002):
//!
//! * [`ModelTolerance`] — when two model values are "the same" (mm, absolute + relative).
//! * solver tolerance — lives in `dotloom-constraints` (residual acceptance).
//! * [`FlattenTolerance`] — maximum chord deviation when curves are tessellated.
//! * [`ScreenTolerance`] — pick/snap radius in CSS pixels, converted to model units
//!   through the current view scale, never used directly as a model distance.

use serde::{Deserialize, Serialize};

/// Model comparison tolerance: `|a - b| <= max(abs, rel * max(|a|, |b|))`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelTolerance {
    /// Absolute tolerance in model length units (millimetres).
    pub abs: f64,
    /// Relative tolerance, applied to the magnitude of the compared values.
    pub rel: f64,
}

impl ModelTolerance {
    /// Default model tolerance: 1 nm absolute, 1e-12 relative.
    pub const DEFAULT: Self = Self { abs: 1e-6, rel: 1e-12 };

    /// Whether two scalars are equal within this tolerance.
    #[must_use]
    pub fn eq(self, a: f64, b: f64) -> bool {
        let scale = a.abs().max(b.abs());
        (a - b).abs() <= self.abs.max(self.rel * scale)
    }

    /// Whether `v` is zero within the absolute tolerance.
    #[must_use]
    pub fn is_zero(self, v: f64) -> bool {
        v.abs() <= self.abs
    }

    /// Effective absolute tolerance near magnitude `scale`.
    #[must_use]
    pub fn at_scale(self, scale: f64) -> f64 {
        self.abs.max(self.rel * scale.abs())
    }
}

impl Default for ModelTolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Maximum distance (model units) between a curve and its flattened polyline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FlattenTolerance(pub f64);

impl FlattenTolerance {
    /// Smallest accepted flatten tolerance; prevents runaway subdivision.
    pub const MIN: f64 = 1e-9;

    /// Tolerance clamped to a sane positive value.
    #[must_use]
    pub fn value(self) -> f64 {
        if self.0.is_finite() && self.0 > Self::MIN { self.0 } else { Self::MIN.max(1e-3) }
    }

    /// Tolerance for drawing at `pixels_per_unit` screen scale with `max_px` deviation.
    #[must_use]
    pub fn for_screen(pixels_per_unit: f64, max_px: f64) -> Self {
        if pixels_per_unit.is_finite() && pixels_per_unit > 0.0 { Self(max_px / pixels_per_unit) } else { Self(1e-3) }
    }
}

impl Default for FlattenTolerance {
    fn default() -> Self {
        Self(0.01)
    }
}

/// A screen-space tolerance in CSS pixels (pick radius, snap radius).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScreenTolerance {
    /// Radius in CSS pixels (device pixel ratio is already applied by the view).
    pub css_px: f64,
}

impl ScreenTolerance {
    /// Convert to model units given the view's model units per CSS pixel.
    #[must_use]
    pub fn to_model(self, model_units_per_css_px: f64) -> f64 {
        if self.css_px.is_finite() && model_units_per_css_px.is_finite() {
            (self.css_px * model_units_per_css_px).abs()
        } else {
            0.0
        }
    }
}
