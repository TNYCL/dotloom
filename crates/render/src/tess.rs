//! CPU tessellation of scene items into GPU-ready meshes.
//!
//! Coordinates are `f32` relative to a per-mesh origin (`f64`), so large world
//! coordinates keep sub-micrometre precision on the GPU.

use bytemuck::{Pod, Zeroable};
use dotloom_geometry::{Aabb, FlattenTolerance, Point, Shape, Text};
use dotloom_scene::{Primitive, SceneItem, Stroke, flags};
use lyon_tessellation::math::point as lpoint;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex as LyonVertex, VertexBuffers,
};

use crate::color::{Rgba, Theme, premul_bytes, with_alpha};
use crate::text::TextSystem;

/// Screen-constant line segment (instanced; expanded in the vertex shader).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct LineInstance {
    /// Start (local units).
    pub p0: [f32; 2],
    /// End (local units).
    pub p1: [f32; 2],
    /// Premultiplied RGBA8.
    pub color: u32,
    /// Width in CSS pixels.
    pub width: f32,
    /// Distance along the path at `p0` (local units), for dash phase.
    pub dist0: f32,
    /// Dash pattern in CSS pixels (`[on, off, on, off]`; `x <= 0` = solid).
    pub dash: [f32; 4],
}

/// Filled triangle vertex.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FillVertex {
    /// Position (local units).
    pub pos: [f32; 2],
    /// Premultiplied RGBA8.
    pub color: u32,
}

/// One glyph quad (instanced).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GlyphInstance {
    /// Bottom-left corner (local units).
    pub origin: [f32; 2],
    /// Quad x edge (local units).
    pub axis_x: [f32; 2],
    /// Quad y edge (local units).
    pub axis_y: [f32; 2],
    /// Atlas rectangle `[u0, v0, u1, v1]`.
    pub uv: [f32; 4],
    /// Premultiplied RGBA8.
    pub color: u32,
    /// Text height (local units); glyphs smaller than a pixel are skipped.
    pub height: f32,
}

/// Tessellated geometry of one item or overlay.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// World origin of the local coordinates.
    pub origin: [f64; 2],
    /// Line instances.
    pub lines: Vec<LineInstance>,
    /// Fill vertices.
    pub fill_vertices: Vec<FillVertex>,
    /// Fill triangle indices.
    pub fill_indices: Vec<u32>,
    /// Glyph instances.
    pub glyphs: Vec<GlyphInstance>,
}

impl Mesh {
    /// Empty mesh with an origin.
    #[must_use]
    pub fn at(origin: [f64; 2]) -> Self {
        Self { origin, ..Self::default() }
    }

    /// Whether nothing would be drawn.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.fill_indices.is_empty() && self.glyphs.is_empty()
    }

    fn local(&self, p: Point) -> [f32; 2] {
        [(p.x - self.origin[0]) as f32, (p.y - self.origin[1]) as f32]
    }

    /// Append a stroked polyline (`closed` adds the closing segment).
    pub fn polyline(&mut self, points: &[Point], closed: bool, color: u32, width: f32, dash: [f32; 4]) {
        let pts: Vec<Point> = points.iter().copied().filter(|p| p.is_finite()).collect();
        match pts.len() {
            0 => {}
            1 => {
                let p = self.local(pts[0]);
                self.lines.push(LineInstance {
                    p0: p,
                    p1: p,
                    color,
                    width: width.max(3.0),
                    dist0: 0.0,
                    dash: [0.0; 4],
                });
            }
            n => {
                let mut dist = 0.0f64;
                let segs = if closed { n } else { n - 1 };
                for i in 0..segs {
                    let a = pts[i];
                    let b = pts[(i + 1) % n];
                    self.lines.push(LineInstance {
                        p0: self.local(a),
                        p1: self.local(b),
                        color,
                        width,
                        dist0: dist as f32,
                        dash,
                    });
                    dist += a.distance(b);
                }
            }
        }
    }

    /// Append filled rings (even-odd rule; holes are rings inside rings).
    pub fn fill_rings(&mut self, rings: &[Vec<Point>], color: u32) {
        let mut b = Path::builder();
        let mut any = false;
        for ring in rings {
            let pts: Vec<[f32; 2]> = ring.iter().filter(|p| p.is_finite()).map(|&p| self.local(p)).collect();
            if pts.len() < 3 {
                continue;
            }
            b.begin(lpoint(pts[0][0], pts[0][1]));
            for p in &pts[1..] {
                b.line_to(lpoint(p[0], p[1]));
            }
            b.end(true);
            any = true;
        }
        if !any {
            return;
        }
        let path = b.build();
        let mut buffers: VertexBuffers<FillVertex, u32> = VertexBuffers::new();
        let options = FillOptions::tolerance(0.01).with_fill_rule(FillRule::EvenOdd);
        let ok = FillTessellator::new()
            .tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: LyonVertex| FillVertex {
                    pos: v.position().to_array(),
                    color,
                }),
            )
            .is_ok();
        if !ok {
            return;
        }
        let base = self.fill_vertices.len() as u32;
        self.fill_vertices.extend_from_slice(&buffers.vertices);
        self.fill_indices.extend(buffers.indices.iter().map(|i| i + base));
    }

    /// Append a filled triangle.
    pub fn triangle(&mut self, a: Point, b: Point, c: Point, color: u32) {
        if !(a.is_finite() && b.is_finite() && c.is_finite()) {
            return;
        }
        let base = self.fill_vertices.len() as u32;
        for p in [a, b, c] {
            self.fill_vertices.push(FillVertex { pos: self.local(p), color });
        }
        self.fill_indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// Append laid-out text. Returns `false` if the glyph atlas was reset during
    /// layout (the caller re-tessellates everything with text).
    pub fn text(&mut self, ts: &mut TextSystem, t: &Text, color: u32) -> bool {
        if !t.position.is_finite() {
            return true;
        }
        let Some(glyphs) = ts.layout(t) else { return false };
        let (sin, cos) = if t.rotation.is_finite() { t.rotation.sin_cos() } else { (0.0, 1.0) };
        let rot = |x: f64, y: f64| Point::new(t.position.x + x * cos - y * sin, t.position.y + x * sin + y * cos);
        for g in glyphs {
            let o = rot(g.x0, g.y0);
            let ox = rot(g.x1, g.y0);
            let oy = rot(g.x0, g.y1);
            let origin = self.local(o);
            self.glyphs.push(GlyphInstance {
                origin,
                axis_x: [(ox.x - o.x) as f32, (ox.y - o.y) as f32],
                axis_y: [(oy.x - o.x) as f32, (oy.y - o.y) as f32],
                uv: g.uv,
                color,
                height: t.height as f32,
            });
        }
        true
    }
}

/// Visual state applied on top of the scene styles.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Look {
    pub flags: u8,
}

/// Normalize a dash list (CSS px) to the shader's 4 entries (SVG repeats odd lists).
#[must_use]
pub fn dash4(dash: &[f32]) -> [f32; 4] {
    let vals: Vec<f32> = dash.iter().copied().filter(|v| v.is_finite() && *v >= 0.0).collect();
    if vals.is_empty() || vals.iter().all(|v| *v == 0.0) {
        return [0.0; 4];
    }
    let mut out = [0.0f32; 4];
    let src: Vec<f32> = if vals.len() % 2 == 1 { vals.iter().chain(vals.iter()).copied().collect() } else { vals };
    for (o, v) in out.iter_mut().zip(src.iter()) {
        *o = *v;
    }
    if out[0] <= 0.0 {
        // A leading zero "on" length would read as solid; use a dot.
        out[0] = 0.01;
    }
    out
}

/// Flatten tolerance actually used for a shape: never finer than 1e-5 of its size
/// (bounds the vertex count of huge curves at extreme zoom).
pub(crate) fn effective_tolerance(lod_tol: f64, bbox: Aabb) -> f64 {
    let size = if bbox.is_empty() { 0.0 } else { bbox.width().max(bbox.height()) };
    let floor = if size.is_finite() { size * 1e-5 } else { 0.0 };
    lod_tol.max(floor).max(FlattenTolerance::MIN)
}

/// Whether a shape's tessellation depends on the zoom level.
pub(crate) fn shape_has_curves(s: &Shape) -> bool {
    match s {
        Shape::Circle(_) | Shape::Arc(_) | Shape::Path(_) => true,
        Shape::Polyline(p) => p.bulges.iter().any(|b| *b != 0.0),
        _ => false,
    }
}

/// Whether an item's tessellation depends on the zoom level.
pub(crate) fn item_has_curves(item: &SceneItem) -> bool {
    item.prims.iter().any(|p| matches!(p, Primitive::Shape { shape, .. } if shape_has_curves(shape)))
}

/// Whether an item draws text.
pub(crate) fn item_has_text(item: &SceneItem) -> bool {
    item.prims.iter().any(|p| matches!(p, Primitive::Text { .. } | Primitive::Shape { shape: Shape::Text(_), .. }))
}

fn origin_of(item: &SceneItem) -> [f64; 2] {
    let b = item.bbox;
    if !b.is_empty() && b.min.is_finite() && b.max.is_finite() {
        let c = b.center();
        return [c.x, c.y];
    }
    for p in &item.prims {
        let at = match p {
            Primitive::Shape { shape, .. } => {
                shape.flatten(FlattenTolerance(1.0)).first().and_then(|f| f.points.first().copied())
            }
            Primitive::Text { text, .. } => Some(text.position),
            Primitive::Arrow { tip, .. } => Some(*tip),
        };
        if let Some(at) = at.filter(|p| p.is_finite()) {
            return [at.x, at.y];
        }
    }
    [0.0, 0.0]
}

/// Tessellate a scene item. Returns `None` if the glyph atlas was reset (retry).
pub(crate) fn tessellate_item(
    item: &SceneItem,
    look: Look,
    lod_tol: f64,
    theme: &Theme,
    ts: &mut TextSystem,
) -> Option<Mesh> {
    let mut m = Mesh::at(origin_of(item));
    let f = look.flags;
    let mut alpha = 1.0f32;
    if f & flags::PREVIEW != 0 {
        alpha *= 0.8;
    }
    if f & flags::LOCKED != 0 {
        alpha *= 0.6;
    }
    if f & flags::READONLY != 0 {
        alpha *= 0.6;
    }
    let emphasis = |c: Rgba| -> Rgba {
        if f & flags::SELECTED != 0 {
            theme.selection
        } else if f & flags::PROBLEM != 0 {
            theme.problem
        } else if f & flags::HOVER != 0 {
            theme.hover
        } else {
            theme.resolve(c)
        }
    };
    let extra_width = if f & (flags::SELECTED | flags::HOVER) != 0 { 1.0 } else { 0.0 };
    let readonly_dash = f & flags::READONLY != 0;
    let tol = effective_tolerance(lod_tol, item.bbox);
    for prim in &item.prims {
        match prim {
            Primitive::Shape { shape, stroke, fill } => {
                if let Shape::Text(t) = shape {
                    let c = stroke.as_ref().map(|s| s.color).or(*fill).unwrap_or(0);
                    if !m.text(ts, t, premul_bytes(with_alpha(emphasis(c), alpha))) {
                        return None;
                    }
                    continue;
                }
                let flat = shape.flatten(FlattenTolerance(tol));
                if let Some(fc) = fill
                    && shape.is_region()
                {
                    let rings: Vec<Vec<Point>> = flat.iter().filter(|p| p.closed).map(|p| p.points.clone()).collect();
                    let mut fc = theme.resolve(*fc);
                    if f & flags::SELECTED != 0 {
                        fc = mix_toward(fc, theme.selection, 0.25);
                    }
                    m.fill_rings(&rings, premul_bytes(with_alpha(fc, alpha)));
                }
                let stroke = match stroke {
                    Some(s) => Some(s.clone()),
                    // Selected fill-only regions get an outline so selection is visible.
                    None if f & (flags::SELECTED | flags::HOVER | flags::PROBLEM) != 0 => {
                        Some(Stroke { color: 0, width: 1.0, dash: Vec::new() })
                    }
                    None => None,
                };
                if let Some(s) = stroke {
                    let color = premul_bytes(with_alpha(emphasis(s.color), alpha));
                    let width = if s.width.is_finite() { s.width.max(0.0) } else { 1.0 } + extra_width;
                    let dash = if readonly_dash && s.dash.is_empty() { dash4(&[6.0, 4.0]) } else { dash4(&s.dash) };
                    if let Shape::Point(p) = shape {
                        m.polyline(&[p.at], false, color, (width + 2.0).max(4.0), [0.0; 4]);
                        continue;
                    }
                    for fp in &flat {
                        m.polyline(&fp.points, fp.closed, color, width, dash);
                    }
                }
            }
            Primitive::Text { text, color } => {
                if !m.text(ts, text, premul_bytes(with_alpha(emphasis(*color), alpha))) {
                    return None;
                }
            }
            Primitive::Arrow { tip, direction, size, color } => {
                let Some(d) = direction.normalize() else { continue };
                if !(size.is_finite() && *size > 0.0) {
                    continue;
                }
                let base = *tip - d * *size;
                let n = dotloom_geometry::Vector::new(-d.y, d.x) * (*size * 0.22);
                m.triangle(*tip, base + n, base - n, premul_bytes(with_alpha(emphasis(*color), alpha)));
            }
        }
    }
    Some(m)
}

fn mix_toward(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let (x, y) = (crate::color::unpack(a), crate::color::unpack(b));
    crate::color::pack([
        x[0] + (y[0] - x[0]) * t,
        x[1] + (y[1] - x[1]) * t,
        x[2] + (y[2] - x[2]) * t,
        x[3].max(y[3] * 0.25),
    ])
}

#[cfg(test)]
mod tests {
    use dotloom_geometry::{Circle, Rect, Segment};

    use super::*;

    fn item(prims: Vec<Primitive>) -> SceneItem {
        let mut bbox = Aabb::EMPTY;
        for p in &prims {
            if let Primitive::Shape { shape, .. } = p {
                bbox = bbox.union(shape.bbox());
            }
        }
        SceneItem { id: 1, layer: 0, bbox, flags: 0, prims }
    }

    fn stroke() -> Option<Stroke> {
        Some(Stroke { color: 0, width: 1.0, dash: Vec::new() })
    }

    #[test]
    fn line_becomes_one_instance_relative_to_origin() {
        let it = item(vec![Primitive::Shape {
            shape: Shape::Line(Segment::new(Point::new(1.0e7, 5.0), Point::new(1.0e7 + 10.0, 5.0))),
            stroke: stroke(),
            fill: None,
        }]);
        let mut ts = TextSystem::with_default_font().unwrap();
        let m = tessellate_item(&it, Look { flags: 0 }, 0.01, &Theme::light(), &mut ts).unwrap();
        assert_eq!(m.lines.len(), 1);
        assert_eq!(m.lines[0].p0, [-5.0, 0.0]);
        assert_eq!(m.lines[0].p1, [5.0, 0.0]);
    }

    #[test]
    fn circle_density_follows_lod() {
        let it = item(vec![Primitive::Shape {
            shape: Shape::Circle(Circle::new(Point::new(0.0, 0.0), 100.0).unwrap()),
            stroke: stroke(),
            fill: Some(0xff00_00ff),
        }]);
        let mut ts = TextSystem::with_default_font().unwrap();
        let coarse = tessellate_item(&it, Look { flags: 0 }, 1.0, &Theme::light(), &mut ts).unwrap();
        let fine = tessellate_item(&it, Look { flags: 0 }, 0.01, &Theme::light(), &mut ts).unwrap();
        assert!(fine.lines.len() > coarse.lines.len() * 5);
        assert!(!fine.fill_indices.is_empty());
        // Huge circles are capped by the relative floor.
        let huge = item(vec![Primitive::Shape {
            shape: Shape::Circle(Circle::new(Point::new(0.0, 0.0), 1.0e6).unwrap()),
            stroke: stroke(),
            fill: None,
        }]);
        let h = tessellate_item(&huge, Look { flags: 0 }, 1e-6, &Theme::light(), &mut ts).unwrap();
        assert!(h.lines.len() < 2000, "{}", h.lines.len());
    }

    #[test]
    fn polygon_with_hole_and_selection_outline() {
        let outer = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)];
        let hole = vec![Point::new(3.0, 3.0), Point::new(7.0, 3.0), Point::new(7.0, 7.0), Point::new(3.0, 7.0)];
        let mut it = item(vec![Primitive::Shape {
            shape: Shape::Polygon(dotloom_geometry::Polygon { outer, holes: vec![hole] }),
            stroke: None,
            fill: Some(0x00ff_00ff),
        }]);
        let mut ts = TextSystem::with_default_font().unwrap();
        let m = tessellate_item(&it, Look { flags: 0 }, 0.1, &Theme::light(), &mut ts).unwrap();
        // A square ring: 8 triangles.
        assert_eq!(m.fill_indices.len(), 8 * 3);
        assert!(m.lines.is_empty());
        it.flags = flags::SELECTED;
        let s = tessellate_item(&it, Look { flags: it.flags }, 0.1, &Theme::light(), &mut ts).unwrap();
        assert_eq!(s.lines.len(), 8);
        assert_eq!(s.lines[0].color, premul_bytes(Theme::light().selection));
    }

    #[test]
    fn text_arrow_point_and_bad_input() {
        let t = Text {
            position: Point::new(0.0, 0.0),
            content: "Ölçü 1200".into(),
            height: 5.0,
            rotation: core::f64::consts::FRAC_PI_2,
            halign: Default::default(),
            valign: Default::default(),
        };
        let it = item(vec![
            Primitive::Text { text: t, color: 0 },
            Primitive::Arrow {
                tip: Point::new(0.0, 0.0),
                direction: dotloom_geometry::Vector::new(1.0, 0.0),
                size: 3.0,
                color: 0,
            },
            Primitive::Shape {
                shape: Shape::Point(dotloom_geometry::PointShape { at: Point::new(1.0, 1.0) }),
                stroke: stroke(),
                fill: None,
            },
            Primitive::Shape {
                shape: Shape::Rect(Rect { origin: Point::new(f64::NAN, 0.0), width: 1.0, height: 1.0 }),
                stroke: stroke(),
                fill: Some(1),
            },
        ]);
        let mut ts = TextSystem::with_default_font().unwrap();
        let m = tessellate_item(&it, Look { flags: 0 }, 0.1, &Theme::light(), &mut ts).unwrap();
        assert_eq!(m.glyphs.len(), 8); // spaces have no quad
        // Rotated 90°: glyph x axis points up.
        assert!(m.glyphs[0].axis_x[0].abs() < 1e-5 && m.glyphs[0].axis_x[1] > 0.0);
        assert_eq!(m.fill_indices.len(), 3);
        assert_eq!(m.lines.len(), 1);
        assert!(m.lines[0].width >= 4.0);
    }

    #[test]
    fn dash_normalization() {
        assert_eq!(dash4(&[]), [0.0; 4]);
        assert_eq!(dash4(&[4.0, 2.0]), [4.0, 2.0, 0.0, 0.0]);
        assert_eq!(dash4(&[3.0]), [3.0, 3.0, 0.0, 0.0]);
        assert_eq!(dash4(&[5.0, 1.0, 1.0]), [5.0, 1.0, 1.0, 5.0]);
        assert_eq!(dash4(&[0.0, 0.0]), [0.0; 4]);
    }
}
