//! Grid and interaction overlays (built every frame; cheap and view-dependent).

use dotloom_geometry::{FlattenTolerance, Point};
use dotloom_scene::{MarkerKind, Overlay};
use serde::{Deserialize, Serialize};

use crate::color::{Theme, premul_bytes, with_alpha};
use crate::tess::{Mesh, dash4, effective_tolerance};
use crate::view::View;

/// Grid settings (model units).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Grid {
    /// Draw the grid.
    pub visible: bool,
    /// Minor spacing in model units.
    pub spacing: f64,
    /// Major line every N minor lines.
    pub major_every: u32,
    /// Smallest on-screen spacing in CSS pixels; finer grids step up by
    /// `major_every`.
    pub min_px: f64,
}

impl Default for Grid {
    fn default() -> Self {
        Self { visible: true, spacing: 10.0, major_every: 10, min_px: 8.0 }
    }
}

/// Marker size in CSS pixels.
pub const MARKER_PX: f64 = 10.0;

/// Effective grid spacing (model units) for a view: steps up until lines are at
/// least `min_px` apart. Returns `None` if the grid cannot be drawn.
#[must_use]
pub fn effective_spacing(grid: &Grid, view: &View) -> Option<f64> {
    if !(grid.spacing.is_finite() && grid.spacing > 0.0) {
        return None;
    }
    let step = f64::from(grid.major_every.max(2));
    let mut s = grid.spacing;
    for _ in 0..64 {
        if s * view.scale >= grid.min_px.max(2.0) {
            return Some(s);
        }
        s *= step;
    }
    None
}

/// Grid lines in device pixels (origin `[0, 0]`, identity transform).
pub(crate) fn grid_mesh(grid: &Grid, view: &View, size: (u32, u32), theme: &Theme) -> Mesh {
    let mut m = Mesh::at([0.0, 0.0]);
    if !grid.visible {
        return m;
    }
    let Some(sp) = effective_spacing(grid, view) else { return m };
    let major = i64::from(grid.major_every.max(2));
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let s = view.device_scale();
    let to_x = |x: f64| ((x - view.center[0]) * s + w * 0.5).floor() + 0.5;
    let to_y = |y: f64| (h * 0.5 - (y - view.center[1]) * s).floor() + 0.5;
    let half_w = w * 0.5 / s;
    let half_h = h * 0.5 / s;
    let (x0, x1) = ((view.center[0] - half_w) / sp, (view.center[0] + half_w) / sp);
    let (y0, y1) = ((view.center[1] - half_h) / sp, (view.center[1] + half_h) / sp);
    // Defensive bound: at most a few thousand lines.
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite())
        || (x1 - x0) > 4096.0
        || (y1 - y0) > 4096.0
    {
        return m;
    }
    // Whether the coarser level is the true major grid (k * sp is a multiple of
    // major spacing) — use exact integer math on the level index.
    let base_ratio = (sp / grid.spacing).round();
    let level_major = |k: i64| -> bool {
        let n = (k as f64 * base_ratio).round() as i64;
        n % major == 0
    };
    let mut lines: Vec<(f64, bool, u32)> = Vec::new();
    for k in (x0.ceil() as i64)..=(x1.floor() as i64) {
        let color = if k == 0 {
            theme.grid_axis
        } else if level_major(k) {
            theme.grid_major
        } else {
            theme.grid_minor
        };
        lines.push((to_x(k as f64 * sp), true, color));
    }
    for k in (y0.ceil() as i64)..=(y1.floor() as i64) {
        let color = if k == 0 {
            theme.grid_axis
        } else if level_major(k) {
            theme.grid_major
        } else {
            theme.grid_minor
        };
        lines.push((to_y(k as f64 * sp), false, color));
    }
    // Minor lines first so majors and axes draw on top.
    lines.sort_by_key(|(_, _, c)| u8::from(*c == theme.grid_major) + 2 * u8::from(*c == theme.grid_axis));
    for (v, vertical, color) in lines {
        let (a, b) =
            if vertical { (Point::new(v, 0.0), Point::new(v, h)) } else { (Point::new(0.0, v), Point::new(w, v)) };
        m.polyline(&[a, b], false, premul_bytes(color), 1.0, [0.0; 4]);
    }
    m
}

fn marker(m: &mut Mesh, kind: MarkerKind, c: [f64; 2], dpr: f64, theme: &Theme) {
    let r = MARKER_PX * 0.5 * dpr;
    let p = |dx: f64, dy: f64| Point::new(c[0] + dx * r, c[1] + dy * r);
    let color = premul_bytes(theme.marker);
    let w = 1.5;
    match kind {
        MarkerKind::Handle => {
            let k = 0.7;
            let pts = [p(-k, -k), p(k, -k), p(k, k), p(-k, k)];
            m.fill_rings(&[pts.to_vec()], premul_bytes(theme.background));
            m.polyline(&pts, true, premul_bytes(theme.selection), 1.5, [0.0; 4]);
        }
        MarkerKind::Endpoint => {
            m.polyline(&[p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)], true, color, w, [0.0; 4])
        }
        MarkerKind::Midpoint => m.polyline(&[p(0.0, -1.1), p(1.0, 0.8), p(-1.0, 0.8)], true, color, w, [0.0; 4]),
        MarkerKind::Center => {
            let pts: Vec<Point> = (0..24)
                .map(|i| {
                    let a = f64::from(i) / 24.0 * core::f64::consts::TAU;
                    p(dotloom_geometry::math::cos(a), dotloom_geometry::math::sin(a))
                })
                .collect();
            m.polyline(&pts, true, color, w, [0.0; 4]);
        }
        MarkerKind::Intersection => {
            m.polyline(&[p(-1.0, -1.0), p(1.0, 1.0)], false, color, w + 0.5, [0.0; 4]);
            m.polyline(&[p(-1.0, 1.0), p(1.0, -1.0)], false, color, w + 0.5, [0.0; 4]);
        }
        MarkerKind::Nearest => {
            m.polyline(&[p(-1.0, -1.0), p(1.0, -1.0), p(-1.0, 1.0), p(1.0, 1.0)], true, color, w, [0.0; 4]);
        }
        MarkerKind::Grid => {
            m.polyline(&[p(-1.0, 0.0), p(1.0, 0.0)], false, color, w, [0.0; 4]);
            m.polyline(&[p(0.0, -1.0), p(0.0, 1.0)], false, color, w, [0.0; 4]);
        }
        MarkerKind::Anchor => {
            m.polyline(&[p(0.0, -1.2), p(1.2, 0.0), p(0.0, 1.2), p(-1.2, 0.0)], true, color, w, [0.0; 4])
        }
    }
}

/// Screen-space overlay (device pixels): markers and the selection rectangle.
pub(crate) fn screen_overlay(o: &Overlay, view: &View, size: (u32, u32), theme: &Theme) -> Mesh {
    let mut m = Mesh::at([0.0, 0.0]);
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let s = view.device_scale();
    let to_dev = |p: Point| [(p.x - view.center[0]) * s + w * 0.5, h * 0.5 - (p.y - view.center[1]) * s];
    if let Some(r) = o.marquee
        && !r.is_empty()
        && r.min.is_finite()
        && r.max.is_finite()
    {
        let a = to_dev(r.min);
        let b = to_dev(r.max);
        let pts = [Point::new(a[0], a[1]), Point::new(b[0], a[1]), Point::new(b[0], b[1]), Point::new(a[0], b[1])];
        let base = if o.crossing { theme.marquee_crossing } else { theme.marquee_window };
        m.fill_rings(&[pts.to_vec()], premul_bytes(with_alpha(base, 0.1)));
        let dash = if o.crossing { dash4(&[5.0, 4.0]) } else { [0.0; 4] };
        m.polyline(&pts, true, premul_bytes(base), 1.0, dash);
    }
    for mk in &o.markers {
        if mk.at.is_finite() {
            marker(&mut m, mk.kind, to_dev(mk.at), view.dpr, theme);
        }
    }
    m
}

/// World-space overlay (construction guides and in-progress sketch shapes).
pub(crate) fn world_overlay(o: &Overlay, view: &View, lod_tol: f64, theme: &Theme) -> Mesh {
    let mut m = Mesh::at(view.center);
    let guide = premul_bytes(theme.guide);
    for g in &o.guides {
        m.polyline(&[g.a, g.b], false, guide, 1.0, dash4(&[6.0, 4.0]));
    }
    let sketch = premul_bytes(theme.sketch);
    for s in &o.sketch {
        let tol = effective_tolerance(lod_tol, s.bbox());
        for fp in s.flatten(FlattenTolerance(tol)) {
            m.polyline(&fp.points, fp.closed, sketch, 1.5, dash4(&[4.0, 3.0]));
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use dotloom_geometry::{Aabb, Segment, Shape};
    use dotloom_scene::Marker;

    use super::*;

    fn view(scale: f64) -> View {
        View { center: [0.0, 0.0], scale, width: 400.0, height: 300.0, dpr: 2.0 }
    }

    #[test]
    fn grid_steps_up_when_dense() {
        let g = Grid::default();
        assert_eq!(effective_spacing(&g, &view(1.0)), Some(10.0));
        assert_eq!(effective_spacing(&g, &view(0.5)), Some(100.0));
        assert_eq!(effective_spacing(&g, &view(0.01)), Some(1000.0));
        let m = grid_mesh(&g, &view(1.0), (800, 600), &Theme::light());
        // 400 css px / 10 = 40 vertical lines (+1), 30 horizontal (+1).
        assert!((70..=74).contains(&m.lines.len()), "{}", m.lines.len());
        // Lines are pixel-centered.
        assert!(m.lines.iter().all(|l| (l.p0[0].fract() - 0.5).abs() < 1e-6 || (l.p0[1].fract() - 0.5).abs() < 1e-6));
        let hidden = Grid { visible: false, ..g };
        assert!(grid_mesh(&hidden, &view(1.0), (800, 600), &Theme::light()).lines.is_empty());
        let bad = Grid { spacing: f64::NAN, ..g };
        assert!(grid_mesh(&bad, &view(1.0), (800, 600), &Theme::light()).lines.is_empty());
    }

    #[test]
    fn markers_have_constant_screen_size() {
        let o = Overlay {
            markers: vec![Marker { at: Point::new(10.0, 10.0), kind: MarkerKind::Endpoint }],
            ..Overlay::default()
        };
        let a = screen_overlay(&o, &view(1.0), (800, 600), &Theme::light());
        let b = screen_overlay(&o, &view(100.0), (800, 600), &Theme::light());
        let span = |m: &Mesh| m.lines[0].p1[0] - m.lines[0].p0[0];
        assert_eq!(span(&a), span(&b));
        assert!((f64::from(span(&a)) - MARKER_PX * 2.0).abs() < 1e-3, "dpr 2 → 20 device px");
    }

    #[test]
    fn marquee_and_world_overlay() {
        let o = Overlay {
            marquee: Some(Aabb::from_corners(Point::new(-10.0, -10.0), Point::new(10.0, 10.0))),
            crossing: true,
            guides: vec![Segment::new(Point::new(0.0, 0.0), Point::new(100.0, 0.0))],
            sketch: vec![Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(0.0, 50.0)))],
            ..Overlay::default()
        };
        let s = screen_overlay(&o, &view(1.0), (800, 600), &Theme::light());
        assert_eq!(s.lines.len(), 4);
        assert!(s.lines[0].dash[0] > 0.0);
        assert_eq!(s.fill_indices.len(), 6);
        let w = world_overlay(&o, &view(1.0), 0.1, &Theme::light());
        assert_eq!(w.lines.len(), 2);
    }
}
