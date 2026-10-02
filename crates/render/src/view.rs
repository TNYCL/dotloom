//! Camera and coordinate conversions.
//!
//! World coordinates are model units (mm, Y up). The view maps them to *device*
//! pixels (Y down) of a target whose CSS size times `dpr` is its pixel size. All
//! conversions use `f64`; GPU buffers receive `f32` offsets relative to local
//! origins, so precision does not depend on the absolute world position.

use dotloom_geometry::{Aabb, Point};
use serde::{Deserialize, Serialize};

use crate::RenderError;

/// Largest supported zoom range (CSS px per model unit).
pub const MIN_SCALE: f64 = 1e-9;
/// See [`MIN_SCALE`].
pub const MAX_SCALE: f64 = 1e9;

/// Camera state. Not part of the document; hosts may persist it as view state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// World point at the center of the target.
    pub center: [f64; 2],
    /// CSS pixels per model unit.
    pub scale: f64,
    /// Target width in CSS pixels.
    pub width: f64,
    /// Target height in CSS pixels.
    pub height: f64,
    /// Device pixels per CSS pixel.
    pub dpr: f64,
}

impl Default for View {
    fn default() -> Self {
        Self { center: [0.0, 0.0], scale: 1.0, width: 800.0, height: 600.0, dpr: 1.0 }
    }
}

impl View {
    /// Validate finite, positive values.
    ///
    /// # Errors
    /// [`RenderError::InvalidView`] for NaN/∞, non-positive sizes or out-of-range zoom.
    pub fn validate(&self) -> Result<(), RenderError> {
        let ok = self.center.iter().all(|v| v.is_finite())
            && self.scale.is_finite()
            && (MIN_SCALE..=MAX_SCALE).contains(&self.scale)
            && self.width.is_finite()
            && self.width > 0.0
            && self.height.is_finite()
            && self.height > 0.0
            && self.dpr.is_finite()
            && self.dpr > 0.0
            && self.dpr <= 16.0;
        if ok { Ok(()) } else { Err(RenderError::InvalidView(format!("{self:?}"))) }
    }

    /// Target size in device pixels (rounded, at least 1×1).
    #[must_use]
    pub fn device_size(&self) -> (u32, u32) {
        let px = |v: f64| (v * self.dpr).round().clamp(1.0, 16384.0) as u32;
        (px(self.width), px(self.height))
    }

    /// Device pixels per model unit.
    #[must_use]
    pub fn device_scale(&self) -> f64 {
        self.scale * self.dpr
    }

    /// World → device pixels.
    #[must_use]
    pub fn world_to_device(&self, p: Point) -> [f64; 2] {
        let (w, h) = self.device_size();
        let s = self.device_scale();
        [(p.x - self.center[0]) * s + f64::from(w) * 0.5, f64::from(h) * 0.5 - (p.y - self.center[1]) * s]
    }

    /// World → CSS pixels.
    #[must_use]
    pub fn world_to_css(&self, p: Point) -> [f64; 2] {
        [
            (p.x - self.center[0]) * self.scale + self.width * 0.5,
            self.height * 0.5 - (p.y - self.center[1]) * self.scale,
        ]
    }

    /// CSS pixels (relative to the target's top-left corner) → world.
    #[must_use]
    pub fn css_to_world(&self, x: f64, y: f64) -> Point {
        Point::new(
            self.center[0] + (x - self.width * 0.5) / self.scale,
            self.center[1] - (y - self.height * 0.5) / self.scale,
        )
    }

    /// Visible world rectangle.
    #[must_use]
    pub fn visible(&self) -> Aabb {
        let hw = self.width * 0.5 / self.scale;
        let hh = self.height * 0.5 / self.scale;
        Aabb::from_corners(
            Point::new(self.center[0] - hw, self.center[1] - hh),
            Point::new(self.center[0] + hw, self.center[1] + hh),
        )
    }

    /// Zoom by `factor` keeping the world point under CSS position `(x, y)` fixed.
    #[must_use]
    pub fn zoomed_at(&self, x: f64, y: f64, factor: f64) -> Self {
        let anchor = self.css_to_world(x, y);
        let scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let mut v = Self { scale, ..*self };
        let moved = v.css_to_world(x, y);
        v.center = [self.center[0] + anchor.x - moved.x, self.center[1] + anchor.y - moved.y];
        v
    }

    /// Fit a world box with `margin` CSS pixels on each side.
    #[must_use]
    pub fn fit(&self, b: Aabb, margin: f64) -> Self {
        if b.is_empty() || !b.min.is_finite() || !b.max.is_finite() {
            return *self;
        }
        let w = (self.width - 2.0 * margin).max(1.0);
        let h = (self.height - 2.0 * margin).max(1.0);
        let bw = b.width().max(1e-9);
        let bh = b.height().max(1e-9);
        let scale = (w / bw).min(h / bh).clamp(MIN_SCALE, MAX_SCALE);
        let c = b.center();
        Self { center: [c.x, c.y], scale, ..*self }
    }

    /// Flatten tolerance for the current zoom, bucketed by powers of two so cached
    /// tessellations stay valid while zooming within a bucket.
    #[must_use]
    pub fn lod(&self) -> i32 {
        dotloom_geometry::math::log2(self.device_scale()).floor().clamp(-120.0, 120.0) as i32
    }
}

/// Flatten tolerance (model units) for a LOD bucket: ¼ device pixel at the
/// bucket's smallest scale.
#[must_use]
pub fn lod_tolerance(lod: i32) -> f64 {
    0.25 / 2f64.powi(lod)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_dpr() {
        let v = View { center: [100.0, 50.0], scale: 2.0, width: 400.0, height: 300.0, dpr: 2.0 };
        assert_eq!(v.device_size(), (800, 600));
        let p = Point::new(110.0, 40.0);
        let css = v.world_to_css(p);
        assert_eq!(css, [220.0, 170.0]);
        // Device pixels are exactly dpr × CSS pixels (no double scaling).
        let dev = v.world_to_device(p);
        assert_eq!(dev, [440.0, 340.0]);
        let back = v.css_to_world(css[0], css[1]);
        assert!((back.x - p.x).abs() < 1e-12 && (back.y - p.y).abs() < 1e-12);
    }

    #[test]
    fn zoom_keeps_anchor() {
        let v = View { center: [0.0, 0.0], scale: 1.0, width: 200.0, height: 100.0, dpr: 1.5 };
        let before = v.css_to_world(30.0, 70.0);
        let z = v.zoomed_at(30.0, 70.0, 3.0);
        let after = z.css_to_world(30.0, 70.0);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
        assert_eq!(z.scale, 3.0);
    }

    #[test]
    fn rejects_invalid() {
        assert!(View { scale: f64::NAN, ..View::default() }.validate().is_err());
        assert!(View { dpr: 0.0, ..View::default() }.validate().is_err());
        assert!(View::default().validate().is_ok());
    }

    #[test]
    fn large_coordinates_stay_precise() {
        let v = View { center: [1.0e9, -1.0e9], scale: 100.0, width: 100.0, height: 100.0, dpr: 1.0 };
        let d = v.world_to_device(Point::new(1.0e9 + 0.01, -1.0e9));
        assert!((d[0] - 51.0).abs() < 1e-3);
    }
}
