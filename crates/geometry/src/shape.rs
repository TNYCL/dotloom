//! Canonical shape model.

use core::f64::consts::{FRAC_PI_2, TAU};
use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::font_metrics as fm;
use crate::{
    Aabb, Affine, Arc, Circle, CubicBez, Curve, FlattenTolerance, GeoResult, GeometryError, LinearKind, ModelTolerance,
    Orientation, Point, QuadBez, Segment, Vector, curve::SIMILARITY_REL, intersect, orientation,
};

/// A point marker.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointShape {
    /// Location.
    pub at: Point,
}

/// Open or closed polyline whose segments may be circular arcs (DXF-style bulge).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polyline {
    /// Vertices.
    pub points: Vec<Point>,
    /// Bulge per segment (`tan(sweep/4)`); empty means all straight.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bulges: Vec<f64>,
    /// Whether the last vertex connects back to the first.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub closed: bool,
}

impl Polyline {
    /// Straight open polyline.
    #[must_use]
    pub fn open(points: Vec<Point>) -> Self {
        Self { points, bulges: Vec::new(), closed: false }
    }

    /// Straight closed polyline.
    #[must_use]
    pub fn closed(points: Vec<Point>) -> Self {
        Self { points, bulges: Vec::new(), closed: true }
    }

    /// Number of segments.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        let n = self.points.len();
        if n < 2 {
            0
        } else if self.closed {
            n
        } else {
            n - 1
        }
    }

    /// Bulge of segment `i` (0 when absent).
    #[must_use]
    pub fn bulge(&self, i: usize) -> f64 {
        self.bulges.get(i).copied().unwrap_or(0.0)
    }

    /// Whether any segment is an arc.
    #[must_use]
    pub fn has_arcs(&self) -> bool {
        self.bulges.iter().any(|b| *b != 0.0)
    }

    /// Segment `i` as a curve.
    #[must_use]
    pub fn segment(&self, i: usize) -> Option<Curve> {
        let n = self.points.len();
        if i >= self.segment_count() {
            return None;
        }
        let a = *self.points.get(i)?;
        let b = *self.points.get((i + 1) % n)?;
        let bulge = self.bulge(i);
        if bulge != 0.0
            && let Ok(arc) = Arc::from_bulge(a, b, bulge)
        {
            return Some(Curve::Arc(arc));
        }
        Some(Curve::Line(Segment::new(a, b)))
    }
}

/// Axis-aligned rectangle in local coordinates (rotation lives in the entity transform).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    /// Minimum corner.
    pub origin: Point,
    /// Width (≥ 0).
    pub width: f64,
    /// Height (≥ 0).
    pub height: f64,
}

impl Rect {
    /// Rectangle from two opposite corners.
    #[must_use]
    pub fn from_corners(a: Point, b: Point) -> Self {
        Self { origin: Point::new(a.x.min(b.x), a.y.min(b.y)), width: (a.x - b.x).abs(), height: (a.y - b.y).abs() }
    }

    /// Corners counter-clockwise from the origin.
    #[must_use]
    pub fn corners(&self) -> [Point; 4] {
        let o = self.origin;
        [
            o,
            Point::new(o.x + self.width, o.y),
            Point::new(o.x + self.width, o.y + self.height),
            Point::new(o.x, o.y + self.height),
        ]
    }

    /// Center.
    #[must_use]
    pub fn center(&self) -> Point {
        Point::new(self.origin.x + self.width * 0.5, self.origin.y + self.height * 0.5)
    }
}

/// Polygon with straight edges and optional holes (even-odd fill).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    /// Outer ring (implicitly closed).
    pub outer: Vec<Point>,
    /// Hole rings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Point>>,
}

/// Path element.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathEl {
    /// Start a new subpath.
    #[serde(rename = "M")]
    MoveTo(Point),
    /// Straight line.
    #[serde(rename = "L")]
    LineTo(Point),
    /// Quadratic Bézier (control, end).
    #[serde(rename = "Q")]
    QuadTo(Point, Point),
    /// Cubic Bézier (control 1, control 2, end).
    #[serde(rename = "C")]
    CubicTo(Point, Point, Point),
    /// Close the current subpath.
    #[serde(rename = "Z")]
    Close,
}

/// A Bézier path made of subpaths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    /// Elements; must start with `MoveTo`.
    pub elements: Vec<PathEl>,
}

/// One subpath of a [`Path`] decomposed into curves.
#[derive(Debug, Clone, PartialEq)]
pub struct SubPath {
    /// Curve pieces in order.
    pub curves: Vec<Curve>,
    /// Whether the subpath is closed.
    pub closed: bool,
}

impl Path {
    /// Decompose into subpaths of curves.
    #[must_use]
    pub fn subpaths(&self) -> Vec<SubPath> {
        let mut out = Vec::new();
        let mut cur: Vec<Curve> = Vec::new();
        let mut start = Point::ORIGIN;
        let mut pen = Point::ORIGIN;
        let mut open = false;
        for el in &self.elements {
            match *el {
                PathEl::MoveTo(p) => {
                    if open && !cur.is_empty() {
                        out.push(SubPath { curves: core::mem::take(&mut cur), closed: false });
                    }
                    cur.clear();
                    start = p;
                    pen = p;
                    open = true;
                }
                PathEl::LineTo(p) => {
                    cur.push(Curve::Line(Segment::new(pen, p)));
                    pen = p;
                }
                PathEl::QuadTo(c, p) => {
                    cur.push(Curve::Cubic(QuadBez { p0: pen, p1: c, p2: p }.to_cubic()));
                    pen = p;
                }
                PathEl::CubicTo(c1, c2, p) => {
                    cur.push(Curve::Cubic(CubicBez { p0: pen, p1: c1, p2: c2, p3: p }));
                    pen = p;
                }
                PathEl::Close => {
                    if pen != start {
                        cur.push(Curve::Line(Segment::new(pen, start)));
                    }
                    if !cur.is_empty() {
                        out.push(SubPath { curves: core::mem::take(&mut cur), closed: true });
                    }
                    pen = start;
                    open = false;
                }
            }
        }
        if open && !cur.is_empty() {
            out.push(SubPath { curves: cur, closed: false });
        }
        out
    }

    fn points_mut(&mut self) -> impl Iterator<Item = &mut Point> {
        self.elements.iter_mut().flat_map(|el| -> Vec<&mut Point> {
            match el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![p],
                PathEl::QuadTo(a, b) => vec![a, b],
                PathEl::CubicTo(a, b, c) => vec![a, b, c],
                PathEl::Close => vec![],
            }
        })
    }

    /// On-curve points (subpath starts and segment ends).
    #[must_use]
    pub fn on_curve_points(&self) -> Vec<Point> {
        self.elements
            .iter()
            .filter_map(|el| match *el {
                PathEl::MoveTo(p) | PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CubicTo(_, _, p) => Some(p),
                PathEl::Close => None,
            })
            .collect()
    }
}

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HAlign {
    /// Anchor at the left edge.
    #[default]
    Left,
    /// Anchor at the center.
    Center,
    /// Anchor at the right edge.
    Right,
}

/// Vertical text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    /// Anchor on the first line's baseline.
    #[default]
    Baseline,
    /// Anchor at the vertical middle of the block.
    Middle,
    /// Anchor at the top of the block.
    Top,
    /// Anchor at the bottom of the block.
    Bottom,
}

/// Text annotation. Height is the cap-to-descender line height in model units.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Text {
    /// Insertion point.
    pub position: Point,
    /// UTF-8 content; `\n` separates lines.
    pub content: String,
    /// Text height in model units.
    pub height: f64,
    /// Rotation in radians.
    #[serde(default)]
    pub rotation: f64,
    /// Horizontal alignment.
    #[serde(default)]
    pub halign: HAlign,
    /// Vertical alignment.
    #[serde(default)]
    pub valign: VAlign,
}

impl Text {
    /// Line spacing relative to height.
    pub const LINE_SPACING: f64 = 1.2;
    /// Top of the text block above the first baseline (relative to height) for
    /// `Top`, `Middle` and `Bottom` alignment — the renderer's layout contract.
    pub const TOP_ABOVE_BASELINE: f64 = 0.8;

    /// Model units per em for text of `height` (height = cap height + descender).
    fn em(height: f64) -> f64 {
        height / ((fm::CAP_HEIGHT + fm::DESCENT) / fm::UNITS_PER_EM)
    }

    /// Font units per em of the renderer's default font (the unit of
    /// [`Text::line_pens`]).
    pub const FONT_UNITS_PER_EM: f64 = fm::UNITS_PER_EM;

    /// Model units per em for text of `height` (for renderers that place glyph
    /// outlines with [`Text::line_pens`]).
    #[must_use]
    pub fn em_size(height: f64) -> f64 {
        Self::em(height)
    }

    /// Pen positions of one line laid out with the renderer's default font: each
    /// drawn character (after the renderer's substitutions: tabs and no-break
    /// spaces become spaces, control characters are dropped) with its pen position
    /// in font units from the line start — advance widths plus pair kerning — and
    /// the line's total advance in font units. The renderer places glyphs with
    /// exactly these positions, so drawn text and [`Text::layout_box`] agree.
    #[must_use]
    pub fn line_pens(line: &str) -> (Vec<(char, f64)>, f64) {
        let mut pens = Vec::with_capacity(line.len());
        let mut pen = 0.0;
        let mut prev: Option<usize> = None;
        for c in line.trim_end_matches('\r').chars() {
            let Some(c) = substitute(c) else { continue };
            let idx = fm::ADVANCES.binary_search_by_key(&u32::from(c), |e| e.0).ok();
            if let (Some(a), Some(b)) = (prev, idx) {
                pen += kerning(a, b);
            }
            pens.push((c, pen));
            pen += f64::from(idx.and_then(|i| fm::ADVANCES.get(i)).map_or(fm::NOTDEF_ADVANCE, |e| e.1));
            prev = idx;
        }
        (pens, pen)
    }

    /// Advance width of one line in model units, measured with the renderer's
    /// default font (advance widths and pair kerning — exactly how it is drawn).
    /// Tabs count as spaces; control characters are skipped.
    #[must_use]
    pub fn line_width(line: &str, height: f64) -> f64 {
        Self::line_pens(line).1 / fm::UNITS_PER_EM * Self::em(height)
    }

    /// Layout box (before rotation) relative to `position`: each line's advance
    /// width with its alignment, from the ascender of the first line to the
    /// descender of the last. Matches the renderer's layout of the default font.
    #[must_use]
    pub fn layout_box(&self) -> Aabb {
        let h = self.height;
        let em = Self::em(h);
        let lines: Vec<&str> = self.content.split('\n').collect();
        let n = lines.len().max(1) as f64;
        let block_h = h * (1.0 + (n - 1.0) * Self::LINE_SPACING);
        let first_baseline = match self.valign {
            VAlign::Baseline => 0.0,
            VAlign::Top => -Self::TOP_ABOVE_BASELINE * h,
            VAlign::Middle => block_h * 0.5 - Self::TOP_ABOVE_BASELINE * h,
            VAlign::Bottom => block_h - Self::TOP_ABOVE_BASELINE * h,
        };
        let (mut x0, mut x1) = (0.0_f64, 0.0_f64);
        for line in &lines {
            let w = Self::line_width(line, h);
            let start = match self.halign {
                HAlign::Left => 0.0,
                HAlign::Center => -w * 0.5,
                HAlign::Right => -w,
            };
            x0 = x0.min(start);
            x1 = x1.max(start + w);
        }
        let top = first_baseline + fm::ASCENT / fm::UNITS_PER_EM * em;
        let bottom = first_baseline - (n - 1.0) * Self::LINE_SPACING * h - fm::DESCENT / fm::UNITS_PER_EM * em;
        Aabb::from_corners(Point::new(x0, top), Point::new(x1, bottom))
    }

    /// The four corners of the layout box in model space.
    #[must_use]
    pub fn layout_corners(&self) -> [Point; 4] {
        let t = Affine::rotate(self.rotation).then(Affine::translate(self.position.to_vector()));
        self.layout_box().corners().map(|c| t.apply(c))
    }
}

/// The renderer's character substitutions: tabs become spaces and other controls
/// are dropped; characters the font lacks fall back (no-break spaces to a space,
/// the diameter sign to the empty-set sign).
fn substitute(c: char) -> Option<char> {
    match c {
        '\t' => return Some(' '),
        c if c.is_control() => return None,
        _ => {}
    }
    if fm::ADVANCES.binary_search_by_key(&u32::from(c), |e| e.0).is_ok() {
        return Some(c);
    }
    Some(match c {
        '\u{00a0}' | '\u{2007}' | '\u{202f}' => ' ',
        '\u{2300}' => '\u{2205}',
        c => c,
    })
}

/// Pair kerning between two characters of the default font (indices into the
/// advance table) in font units. Per `kern` lookup an explicit pair wins over the
/// class matrix; the lookups add up (OpenType GPOS pair positioning).
fn kerning(a: usize, b: usize) -> f64 {
    let key = (u16::try_from(a).unwrap_or(u16::MAX), u16::try_from(b).unwrap_or(u16::MAX));
    let mut units = 0i32;
    for k in &fm::KERN {
        if let Ok(i) = k.pairs.binary_search_by_key(&key, |p| (p.0, p.1)) {
            units += k.pairs.get(i).map_or(0, |p| i32::from(p.2));
            continue;
        }
        let (Some(&left), Some(&right)) = (k.left.get(a), k.right.get(b)) else { continue };
        if left == u8::MAX {
            continue;
        }
        units += k.matrix.get(usize::from(left) * k.columns + usize::from(right)).map_or(0, |v| i32::from(*v));
    }
    f64::from(units)
}

/// How to handle transforms that a shape cannot represent exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransformPolicy {
    /// Return [`GeometryError::UnsupportedTransform`].
    #[default]
    Strict,
    /// Convert to a [`Path`]/[`Polygon`] that represents the transformed shape
    /// (circles/arcs become cubic approximations with < 0.03 % radial error).
    Convert,
}

/// Semantic anchor kinds, used by snapping and constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorKind {
    /// Start/end of an open curve.
    Endpoint,
    /// Midpoint of a segment or arc.
    Midpoint,
    /// Center of a circle/arc/rectangle.
    Center,
    /// Polyline/polygon vertex.
    Vertex,
    /// Circle quadrant point.
    Quadrant,
    /// Rectangle corner.
    Corner,
    /// Text insertion point.
    Insert,
    /// Area centroid.
    Centroid,
    /// Point shape location.
    Node,
}

/// A named semantic anchor of a shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// Stable name within the shape (`start`, `end`, `mid`, `center`, `v3`, ...).
    /// Built-in names are static (no allocation per anchor: engines cache the
    /// anchors of every entity).
    pub name: Cow<'static, str>,
    /// Kind.
    pub kind: AnchorKind,
    /// Position.
    pub point: Point,
}

impl Anchor {
    fn new(name: impl Into<Cow<'static, str>>, kind: AnchorKind, point: Point) -> Self {
        Self { name: name.into(), kind, point }
    }
}

/// `prefix{i}` names (`v0`, `m3`, `c1`, …); the first 32 of each are static.
fn indexed(prefix: char, i: usize) -> Cow<'static, str> {
    const N: usize = 32;
    macro_rules! table {
        ($p:literal) => {
            [
                concat!($p, "0"),
                concat!($p, "1"),
                concat!($p, "2"),
                concat!($p, "3"),
                concat!($p, "4"),
                concat!($p, "5"),
                concat!($p, "6"),
                concat!($p, "7"),
                concat!($p, "8"),
                concat!($p, "9"),
                concat!($p, "10"),
                concat!($p, "11"),
                concat!($p, "12"),
                concat!($p, "13"),
                concat!($p, "14"),
                concat!($p, "15"),
                concat!($p, "16"),
                concat!($p, "17"),
                concat!($p, "18"),
                concat!($p, "19"),
                concat!($p, "20"),
                concat!($p, "21"),
                concat!($p, "22"),
                concat!($p, "23"),
                concat!($p, "24"),
                concat!($p, "25"),
                concat!($p, "26"),
                concat!($p, "27"),
                concat!($p, "28"),
                concat!($p, "29"),
                concat!($p, "30"),
                concat!($p, "31"),
            ]
        };
    }
    static V: [&str; N] = table!("v");
    static M: [&str; N] = table!("m");
    static C: [&str; N] = table!("c");
    static E: [&str; N] = table!("e");
    static Q: [&str; N] = table!("q");
    let table: Option<&[&'static str; N]> = match prefix {
        'v' => Some(&V),
        'm' => Some(&M),
        'c' => Some(&C),
        'e' => Some(&E),
        'q' => Some(&Q),
        _ => None,
    };
    match table.and_then(|t| t.get(i)) {
        Some(name) => Cow::Borrowed(name),
        None => Cow::Owned(format!("{prefix}{i}")),
    }
}

/// Shape kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    /// Point marker.
    Point,
    /// Line segment.
    Line,
    /// Polyline.
    Polyline,
    /// Rectangle.
    Rect,
    /// Circle.
    Circle,
    /// Arc.
    Arc,
    /// Bézier path.
    Path,
    /// Polygon.
    Polygon,
    /// Text.
    Text,
}

impl ShapeKind {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::Polyline => "polyline",
            Self::Rect => "rect",
            Self::Circle => "circle",
            Self::Arc => "arc",
            Self::Path => "path",
            Self::Polygon => "polygon",
            Self::Text => "text",
        }
    }
}

/// A canonical 2D shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Shape {
    /// Point marker.
    Point(PointShape),
    /// Line segment.
    Line(Segment),
    /// Polyline (optionally closed, optional arc segments).
    Polyline(Polyline),
    /// Axis-aligned rectangle.
    Rect(Rect),
    /// Circle.
    Circle(Circle),
    /// Circular arc.
    Arc(Arc),
    /// Bézier path.
    Path(Path),
    /// Polygon with holes.
    Polygon(Polygon),
    /// Text annotation.
    Text(Text),
}

/// Flattened ring/curve.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPath {
    /// Points in order.
    pub points: Vec<Point>,
    /// Closed ring.
    pub closed: bool,
}

/// Limits enforced by [`Shape::validate`] to bound memory/CPU on untrusted input.
pub const MAX_SHAPE_POINTS: usize = 1_000_000;
/// Maximum text length in characters.
pub const MAX_TEXT_CHARS: usize = 100_000;

impl Shape {
    /// Kind of the shape.
    #[must_use]
    pub const fn kind(&self) -> ShapeKind {
        match self {
            Self::Point(_) => ShapeKind::Point,
            Self::Line(_) => ShapeKind::Line,
            Self::Polyline(_) => ShapeKind::Polyline,
            Self::Rect(_) => ShapeKind::Rect,
            Self::Circle(_) => ShapeKind::Circle,
            Self::Arc(_) => ShapeKind::Arc,
            Self::Path(_) => ShapeKind::Path,
            Self::Polygon(_) => ShapeKind::Polygon,
            Self::Text(_) => ShapeKind::Text,
        }
    }

    /// Validate numeric sanity and structural rules.
    pub fn validate(&self) -> GeoResult<()> {
        let all_finite = |pts: &[Point], what: &'static str| -> GeoResult<()> {
            if pts.len() > MAX_SHAPE_POINTS {
                return Err(GeometryError::InvalidArgument("too many points in shape"));
            }
            if pts.iter().all(|p| p.is_finite()) { Ok(()) } else { Err(GeometryError::NonFinite(what)) }
        };
        match self {
            Self::Point(p) => all_finite(&[p.at], "point"),
            Self::Line(s) => all_finite(&[s.a, s.b], "line"),
            Self::Polyline(p) => {
                all_finite(&p.points, "polyline")?;
                if p.points.len() < 2 {
                    return Err(GeometryError::Degenerate("polyline needs at least 2 points"));
                }
                if !p.bulges.is_empty() && p.bulges.len() != p.segment_count() {
                    return Err(GeometryError::InvalidArgument("bulge count must equal segment count"));
                }
                if p.bulges.iter().any(|b| !b.is_finite()) {
                    return Err(GeometryError::NonFinite("polyline bulge"));
                }
                Ok(())
            }
            Self::Rect(r) => {
                all_finite(&[r.origin], "rect")?;
                if !(r.width.is_finite() && r.height.is_finite()) {
                    return Err(GeometryError::NonFinite("rect size"));
                }
                if r.width < 0.0 || r.height < 0.0 {
                    return Err(GeometryError::InvalidArgument("rect size must be non-negative"));
                }
                Ok(())
            }
            Self::Circle(c) => Circle::new(c.center, c.radius).map(|_| ()),
            Self::Arc(a) => {
                Arc::new(a.center, a.radius, a.start, a.sweep)?;
                if a.sweep.abs() > TAU {
                    return Err(GeometryError::InvalidArgument("arc sweep exceeds 2π"));
                }
                Ok(())
            }
            Self::Path(p) => {
                if p.elements.len() > MAX_SHAPE_POINTS {
                    return Err(GeometryError::InvalidArgument("too many path elements"));
                }
                if !matches!(p.elements.first(), Some(PathEl::MoveTo(_))) {
                    return Err(GeometryError::InvalidArgument("path must start with MoveTo"));
                }
                let mut q = p.clone();
                if q.points_mut().all(|p| p.is_finite()) { Ok(()) } else { Err(GeometryError::NonFinite("path")) }
            }
            Self::Polygon(p) => {
                all_finite(&p.outer, "polygon")?;
                if p.outer.len() < 3 {
                    return Err(GeometryError::Degenerate("polygon needs at least 3 points"));
                }
                for h in &p.holes {
                    all_finite(h, "polygon hole")?;
                    if h.len() < 3 {
                        return Err(GeometryError::Degenerate("polygon hole needs at least 3 points"));
                    }
                }
                Ok(())
            }
            Self::Text(t) => {
                all_finite(&[t.position], "text")?;
                if !(t.height.is_finite() && t.rotation.is_finite()) {
                    return Err(GeometryError::NonFinite("text metrics"));
                }
                if t.height <= 0.0 {
                    return Err(GeometryError::InvalidArgument("text height must be > 0"));
                }
                if t.content.chars().count() > MAX_TEXT_CHARS {
                    return Err(GeometryError::InvalidArgument("text too long"));
                }
                Ok(())
            }
        }
    }

    /// Decompose into curve pieces. Points and text have none.
    #[must_use]
    pub fn curves(&self) -> Vec<Curve> {
        match self {
            Self::Point(_) | Self::Text(_) => Vec::new(),
            Self::Line(s) => vec![Curve::Line(*s)],
            Self::Polyline(p) => (0..p.segment_count()).filter_map(|i| p.segment(i)).collect(),
            Self::Rect(r) => ring_lines(&r.corners()),
            Self::Circle(c) => vec![Curve::Circle(*c)],
            Self::Arc(a) => vec![Curve::Arc(*a)],
            Self::Path(p) => p.subpaths().into_iter().flat_map(|s| s.curves).collect(),
            Self::Polygon(p) => {
                let mut v = ring_lines(&p.outer);
                for h in &p.holes {
                    v.extend(ring_lines(h));
                }
                v
            }
        }
    }

    /// Bounding box (text: its layout box with the default font).
    #[must_use]
    pub fn bbox(&self) -> Aabb {
        match self {
            Self::Point(p) => Aabb::from_corners(p.at, p.at),
            Self::Text(t) => Aabb::from_points(t.layout_corners()),
            Self::Rect(r) => Aabb::from_points(r.corners()),
            Self::Polygon(p) => Aabb::from_points(p.outer.iter().copied()),
            _ => self.curves().iter().fold(Aabb::EMPTY, |b, c| b.union(c.bbox())),
        }
    }

    /// Whether the shape bounds an area (fill and inside tests apply).
    #[must_use]
    pub fn is_region(&self) -> bool {
        match self {
            Self::Rect(_) | Self::Circle(_) | Self::Polygon(_) => true,
            Self::Polyline(p) => p.closed && p.points.len() >= 3,
            Self::Path(p) => {
                let subs = p.subpaths();
                !subs.is_empty() && subs.iter().all(|s| s.closed)
            }
            _ => false,
        }
    }

    /// Distance from `p` to the outline (text: to its layout box).
    #[must_use]
    pub fn distance_to(&self, p: Point) -> f64 {
        match self {
            Self::Point(s) => s.at.distance(p),
            Self::Text(t) => {
                if point_in_ring(p, &t.layout_corners()) {
                    0.0
                } else {
                    ring_lines(&t.layout_corners()).iter().map(|c| c.distance_to_point(p)).fold(f64::INFINITY, f64::min)
                }
            }
            _ => self.curves().iter().map(|c| c.distance_to_point(p)).fold(f64::INFINITY, f64::min),
        }
    }

    /// Even-odd inside test for regions (always false for non-regions).
    /// Straight edges use exact orientation predicates; curved edges are flattened
    /// with `tol`.
    #[must_use]
    pub fn contains_point(&self, p: Point, tol: FlattenTolerance) -> bool {
        if !self.is_region() || !self.bbox().contains_point(p) {
            return false;
        }
        match self {
            Self::Rect(r) => point_in_ring(p, &r.corners()),
            Self::Circle(c) => p.distance(c.center) <= c.radius,
            Self::Polygon(poly) => {
                let mut inside = point_in_ring(p, &poly.outer);
                for h in &poly.holes {
                    if point_in_ring(p, h) {
                        inside = !inside;
                    }
                }
                inside
            }
            _ => {
                let mut inside = false;
                for ring in self.flatten(tol) {
                    if ring.closed && point_in_ring(p, &ring.points) {
                        inside = !inside;
                    }
                }
                inside
            }
        }
    }

    /// Hit test: within `radius` of the outline, or inside when `fill` is set.
    #[must_use]
    pub fn hit(&self, p: Point, radius: f64, fill: bool) -> bool {
        if self.bbox().distance_to_point(p) > radius {
            return false;
        }
        if fill && self.contains_point(p, FlattenTolerance(radius.max(1e-9) * 0.25)) {
            return true;
        }
        self.distance_to(p) <= radius
    }

    /// Window selection: shape entirely inside `r`.
    #[must_use]
    pub fn inside_rect(&self, r: Aabb) -> bool {
        r.contains(self.bbox())
    }

    /// Crossing selection: any part of the shape touches `r` (regions: also when
    /// `r` lies inside the region).
    #[must_use]
    pub fn intersects_rect(&self, r: Aabb, tol: ModelTolerance) -> bool {
        let bb = self.bbox();
        if !bb.intersects(r) {
            return false;
        }
        if r.contains(bb) {
            return true;
        }
        match self {
            Self::Point(p) => r.contains_point(p.at),
            Self::Text(t) => {
                let corners = t.layout_corners();
                corners.iter().any(|c| r.contains_point(*c))
                    || rect_edges(r).iter().any(|e| {
                        ring_lines(&corners).iter().any(|c| !intersect::intersect(e, c, tol).points.is_empty())
                    })
                    || point_in_ring(r.center(), &corners)
            }
            _ => {
                let curves = self.curves();
                if curves.iter().any(|c| r.contains_point(c.start()) || r.contains_point(c.end())) {
                    return true;
                }
                let edges = rect_edges(r);
                if curves.iter().any(|c| edges.iter().any(|e| !intersect::intersect(e, c, tol).points.is_empty())) {
                    return true;
                }
                // Curve fully inside rect but endpoints outside is impossible; the
                // remaining case is the rect fully inside a region.
                self.is_region() && self.contains_point(r.center(), FlattenTolerance::default())
            }
        }
    }

    /// Flatten into polylines/rings with maximum deviation `tol`.
    #[must_use]
    pub fn flatten(&self, tol: FlattenTolerance) -> Vec<FlatPath> {
        let tol = tol.value();
        let curves_to_flat = |curves: &[Curve], closed: bool| -> FlatPath {
            let mut points = Vec::new();
            if let Some(first) = curves.first() {
                points.push(first.start());
            }
            for c in curves {
                c.flatten_into(tol, &mut points);
            }
            if closed && points.len() > 1 && points.first() == points.last() {
                points.pop();
            }
            FlatPath { points, closed }
        };
        match self {
            Self::Point(p) => vec![FlatPath { points: vec![p.at], closed: false }],
            Self::Text(t) => vec![FlatPath { points: t.layout_corners().to_vec(), closed: true }],
            Self::Line(s) => vec![FlatPath { points: vec![s.a, s.b], closed: false }],
            Self::Rect(r) => vec![FlatPath { points: r.corners().to_vec(), closed: true }],
            Self::Polygon(p) => core::iter::once(&p.outer)
                .chain(p.holes.iter())
                .map(|ring| FlatPath { points: ring.clone(), closed: true })
                .collect(),
            Self::Circle(c) => {
                let mut f = curves_to_flat(&[Curve::Circle(*c)], true);
                if f.points.len() > 1
                    && f.points.first().zip(f.points.last()).is_some_and(|(a, b)| a.distance(*b) < tol)
                {
                    f.points.pop();
                }
                vec![f]
            }
            Self::Arc(a) => vec![curves_to_flat(&[Curve::Arc(*a)], false)],
            Self::Polyline(p) => vec![curves_to_flat(&self.curves(), p.closed)],
            Self::Path(p) => p.subpaths().iter().map(|s| curves_to_flat(&s.curves, s.closed)).collect(),
        }
    }

    /// Semantic anchors.
    #[must_use]
    pub fn anchors(&self) -> Vec<Anchor> {
        use AnchorKind as K;
        match self {
            Self::Point(p) => vec![Anchor::new("point", K::Node, p.at)],
            Self::Line(s) => vec![
                Anchor::new("start", K::Endpoint, s.a),
                Anchor::new("end", K::Endpoint, s.b),
                Anchor::new("mid", K::Midpoint, s.midpoint()),
            ],
            Self::Polyline(p) => {
                let mut v: Vec<Anchor> =
                    p.points.iter().enumerate().map(|(i, q)| Anchor::new(indexed('v', i), K::Vertex, *q)).collect();
                for i in 0..p.segment_count() {
                    if let Some(c) = p.segment(i) {
                        v.push(Anchor::new(indexed('m', i), K::Midpoint, c.point_at(0.5)));
                    }
                }
                if !p.closed {
                    if let Some(f) = p.points.first() {
                        v.push(Anchor::new("start", K::Endpoint, *f));
                    }
                    if let Some(l) = p.points.last() {
                        v.push(Anchor::new("end", K::Endpoint, *l));
                    }
                }
                v
            }
            Self::Rect(r) => {
                let c = r.corners();
                let mut v: Vec<Anchor> =
                    c.iter().enumerate().map(|(i, q)| Anchor::new(indexed('c', i), K::Corner, *q)).collect();
                for i in 0..4 {
                    v.push(Anchor::new(indexed('e', i), K::Midpoint, c[i].midpoint(c[(i + 1) % 4])));
                }
                v.push(Anchor::new("center", K::Center, r.center()));
                v
            }
            Self::Circle(c) => {
                let mut v = vec![Anchor::new("center", K::Center, c.center)];
                for i in 0..4u8 {
                    v.push(Anchor::new(
                        indexed('q', usize::from(i)),
                        K::Quadrant,
                        c.point_at_angle(f64::from(i) * FRAC_PI_2),
                    ));
                }
                v
            }
            Self::Arc(a) => vec![
                Anchor::new("start", K::Endpoint, a.start_point()),
                Anchor::new("end", K::Endpoint, a.end_point()),
                Anchor::new("mid", K::Midpoint, a.mid_point()),
                Anchor::new("center", K::Center, a.center),
            ],
            Self::Path(p) => {
                let pts = p.on_curve_points();
                let mut v: Vec<Anchor> =
                    pts.iter().enumerate().map(|(i, q)| Anchor::new(indexed('v', i), K::Vertex, *q)).collect();
                if let Some(f) = pts.first() {
                    v.push(Anchor::new("start", K::Endpoint, *f));
                }
                if let Some(l) = pts.last() {
                    v.push(Anchor::new("end", K::Endpoint, *l));
                }
                v
            }
            Self::Polygon(p) => {
                let mut v: Vec<Anchor> =
                    p.outer.iter().enumerate().map(|(i, q)| Anchor::new(indexed('v', i), K::Vertex, *q)).collect();
                if let Some(c) = ring_centroid(&p.outer) {
                    v.push(Anchor::new("centroid", K::Centroid, c));
                }
                v
            }
            Self::Text(t) => vec![Anchor::new("insert", K::Insert, t.position)],
        }
    }

    /// Anchor by name.
    #[must_use]
    pub fn anchor(&self, name: &str) -> Option<Point> {
        self.anchors().into_iter().find(|a| a.name == name).map(|a| a.point)
    }

    /// Total outline length (text and points: 0).
    #[must_use]
    pub fn length(&self) -> f64 {
        self.curves().iter().map(Curve::length).sum()
    }

    /// Unsigned area of regions (0 for non-regions). Bézier areas use flattening.
    #[must_use]
    pub fn area(&self) -> f64 {
        match self {
            Self::Rect(r) => r.width * r.height,
            Self::Circle(c) => core::f64::consts::PI * c.radius * c.radius,
            Self::Polygon(p) => {
                let mut a = ring_signed_area(&p.outer).abs();
                for h in &p.holes {
                    a -= ring_signed_area(h).abs();
                }
                a.max(0.0)
            }
            Self::Polyline(p) if p.closed => polyline_signed_area(p).abs(),
            Self::Path(_) if self.is_region() => self
                .flatten(FlattenTolerance(self.bbox().size().length() * 1e-6))
                .iter()
                .map(|r| ring_signed_area(&r.points))
                .sum::<f64>()
                .abs(),
            _ => 0.0,
        }
    }

    /// Transform the shape. See [`TransformPolicy`].
    pub fn transform(&self, t: Affine, policy: TransformPolicy) -> GeoResult<Self> {
        if !t.is_finite() {
            return Err(GeometryError::NonFinite("transform"));
        }
        let kind = t.linear_kind(SIMILARITY_REL);
        if kind == LinearKind::Singular {
            return Err(GeometryError::SingularTransform { determinant: t.determinant() });
        }
        let unsupported =
            |shape: &'static str, reason: &'static str| GeometryError::UnsupportedTransform { shape, reason };
        Ok(match self {
            Self::Point(p) => Self::Point(PointShape { at: t.apply(p.at) }),
            Self::Line(s) => Self::Line(s.transform(t)),
            Self::Polyline(p) => {
                if p.has_arcs() && !matches!(kind, LinearKind::Similarity { .. }) {
                    if policy == TransformPolicy::Strict {
                        return Err(unsupported("polyline", "arc segments need a similarity transform"));
                    }
                    return Self::Path(curves_to_path(&self.curves(), p.closed)).transform(t, policy);
                }
                let reflected = matches!(kind, LinearKind::Similarity { reflected: true, .. });
                Self::Polyline(Polyline {
                    points: p.points.iter().map(|q| t.apply(*q)).collect(),
                    bulges: if reflected { p.bulges.iter().map(|b| -b).collect() } else { p.bulges.clone() },
                    closed: p.closed,
                })
            }
            Self::Rect(r) => {
                if t.is_axis_aligned(1e-12) {
                    let c = r.corners();
                    Self::Rect(Rect::from_corners(t.apply(c[0]), t.apply(c[2])))
                } else if policy == TransformPolicy::Convert {
                    Self::Polygon(Polygon {
                        outer: r.corners().iter().map(|q| t.apply(*q)).collect(),
                        holes: Vec::new(),
                    })
                } else {
                    return Err(unsupported("rect", "rotation/shear must be applied through the entity transform"));
                }
            }
            Self::Circle(c) => match c.transform(t) {
                Ok(c) => Self::Circle(c),
                Err(e) if policy == TransformPolicy::Strict => return Err(e),
                Err(_) => {
                    let arc = Arc::new(c.center, c.radius, 0.0, TAU)?;
                    Self::Path(curves_to_path(&arc_to_cubics(arc), true)).transform(t, policy)?
                }
            },
            Self::Arc(a) => match a.transform(t) {
                Ok(a) => Self::Arc(a),
                Err(e) if policy == TransformPolicy::Strict => return Err(e),
                Err(_) => Self::Path(curves_to_path(&arc_to_cubics(*a), false)).transform(t, policy)?,
            },
            Self::Path(p) => {
                let mut q = p.clone();
                for pt in q.points_mut() {
                    *pt = t.apply(*pt);
                }
                Self::Path(q)
            }
            Self::Polygon(p) => Self::Polygon(Polygon {
                outer: p.outer.iter().map(|q| t.apply(*q)).collect(),
                holes: p.holes.iter().map(|h| h.iter().map(|q| t.apply(*q)).collect()).collect(),
            }),
            Self::Text(tx) => match kind {
                LinearKind::Similarity { scale, .. } => {
                    let dir = t.apply_vector(Vector::from_angle(tx.rotation));
                    Self::Text(Text {
                        position: t.apply(tx.position),
                        height: tx.height * scale,
                        rotation: dir.angle(),
                        ..tx.clone()
                    })
                }
                _ => return Err(unsupported("text", "text only supports similarity transforms")),
            },
        })
    }
}

fn ring_lines(pts: &[Point]) -> Vec<Curve> {
    let n = pts.len();
    (0..n)
        .filter_map(|i| {
            let a = *pts.get(i)?;
            let b = *pts.get((i + 1) % n)?;
            Some(Curve::Line(Segment::new(a, b)))
        })
        .collect()
}

fn rect_edges(r: Aabb) -> Vec<Curve> {
    ring_lines(&r.corners())
}

/// Even-odd point-in-ring test using exact orientation predicates.
#[must_use]
pub fn point_in_ring(p: Point, ring: &[Point]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (Some(&a), Some(&b)) = (ring.get(i), ring.get(j)) else {
            break;
        };
        if (a.y > p.y) != (b.y > p.y) {
            // Edge crosses the horizontal ray; decide side exactly.
            let o = orientation(b, a, p);
            let upward = a.y > b.y;
            let left = if upward { o == Orientation::CounterClockwise } else { o == Orientation::Clockwise };
            if left {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Signed area (positive = counter-clockwise).
#[must_use]
pub fn ring_signed_area(ring: &[Point]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let o = ring.first().copied().unwrap_or(Point::ORIGIN);
    let mut s = 0.0;
    for i in 0..n {
        if let (Some(a), Some(b)) = (ring.get(i), ring.get((i + 1) % n)) {
            s += (*a - o).cross(*b - o);
        }
    }
    s * 0.5
}

fn ring_centroid(ring: &[Point]) -> Option<Point> {
    let n = ring.len();
    let o = *ring.first()?;
    let mut a2 = 0.0;
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..n {
        let p = *ring.get(i)? - o;
        let q = *ring.get((i + 1) % n)? - o;
        let c = p.cross(q);
        a2 += c;
        cx += (p.x + q.x) * c;
        cy += (p.y + q.y) * c;
    }
    if a2.abs() < f64::MIN_POSITIVE {
        return None;
    }
    Some(Point::new(o.x + cx / (3.0 * a2), o.y + cy / (3.0 * a2)))
}

fn polyline_signed_area(p: &Polyline) -> f64 {
    let mut a = ring_signed_area(&p.points);
    for i in 0..p.segment_count() {
        if let Some(Curve::Arc(arc)) = p.segment(i) {
            // Signed circular segment area between chord and arc.
            let th = arc.sweep;
            a += 0.5 * arc.radius * arc.radius * (th - crate::math::sin(th));
        }
    }
    a
}

/// Approximate an arc by cubic Béziers (≤ 90° each).
#[must_use]
pub fn arc_to_cubics(a: Arc) -> Vec<Curve> {
    let n = (a.sweep.abs() / FRAC_PI_2).ceil().max(1.0) as u32;
    let step = a.sweep / f64::from(n);
    let k = 4.0 / 3.0 * crate::math::tan(step / 4.0);
    (0..n)
        .map(|i| {
            let a0 = a.start + step * f64::from(i);
            let a1 = a0 + step;
            let p0 = a.center + Vector::from_angle(a0) * a.radius;
            let p3 = a.center + Vector::from_angle(a1) * a.radius;
            let p1 = p0 + Vector::from_angle(a0).perp() * (a.radius * k);
            let p2 = p3 - Vector::from_angle(a1).perp() * (a.radius * k);
            Curve::Cubic(CubicBez { p0, p1, p2, p3 })
        })
        .collect()
}

fn curves_to_path(curves: &[Curve], closed: bool) -> Path {
    let mut elements = Vec::new();
    if let Some(first) = curves.first() {
        elements.push(PathEl::MoveTo(first.start()));
    }
    for c in curves {
        match *c {
            Curve::Line(s) => elements.push(PathEl::LineTo(s.b)),
            Curve::Cubic(b) => elements.push(PathEl::CubicTo(b.p1, b.p2, b.p3)),
            Curve::Arc(a) => {
                for cb in arc_to_cubics(a) {
                    if let Curve::Cubic(b) = cb {
                        elements.push(PathEl::CubicTo(b.p1, b.p2, b.p3));
                    }
                }
            }
            Curve::Circle(ci) => {
                if let Ok(a) = Arc::new(ci.center, ci.radius, 0.0, TAU) {
                    for cb in arc_to_cubics(a) {
                        if let Curve::Cubic(b) = cb {
                            elements.push(PathEl::CubicTo(b.p1, b.p2, b.p3));
                        }
                    }
                }
            }
        }
    }
    if closed {
        elements.push(PathEl::Close);
    }
    Path { elements }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::f64::consts::PI;

    #[test]
    fn serde_shape_tagging() {
        let s = Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(1.0, 2.0)));
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"type":"line","a":[0.0,0.0],"b":[1.0,2.0]}"#);
        let back: Shape = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        let p = Shape::Path(Path {
            elements: vec![
                PathEl::MoveTo(Point::ORIGIN),
                PathEl::CubicTo(Point::new(1.0, 1.0), Point::new(2.0, 1.0), Point::new(3.0, 0.0)),
                PathEl::Close,
            ],
        });
        let j = serde_json::to_string(&p).unwrap();
        let back: Shape = serde_json::from_str(&j).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn rect_rotation_is_strict_by_default() {
        let r = Shape::Rect(Rect { origin: Point::ORIGIN, width: 2.0, height: 1.0 });
        assert!(r.transform(Affine::rotate(0.3), TransformPolicy::Strict).is_err());
        let conv = r.transform(Affine::rotate(PI / 2.0), TransformPolicy::Convert).unwrap();
        assert_eq!(conv.kind(), ShapeKind::Polygon);
        assert!((conv.area() - 2.0).abs() < 1e-12);
        let mirrored = r.transform(Affine::scale(-1.0, 1.0), TransformPolicy::Strict).unwrap();
        assert_eq!(mirrored.bbox().min, Point::new(-2.0, 0.0));
    }

    #[test]
    fn circle_nonuniform_scale_policy() {
        let c = Shape::Circle(Circle::new(Point::ORIGIN, 1.0).unwrap());
        let err = c.transform(Affine::scale(2.0, 1.0), TransformPolicy::Strict);
        assert!(matches!(err, Err(GeometryError::UnsupportedTransform { .. })));
        let e = c.transform(Affine::scale(2.0, 1.0), TransformPolicy::Convert).unwrap();
        assert_eq!(e.kind(), ShapeKind::Path);
        // Ellipse area π·a·b = 2π, cubic approximation is within 0.1 %.
        assert!((e.area() - 2.0 * PI).abs() / (2.0 * PI) < 1e-3, "{}", e.area());
        // Circle anchors are no longer reported as a center anchor after conversion.
        assert!(e.anchor("center").is_none());
    }

    #[test]
    fn contains_with_holes() {
        let poly = Shape::Polygon(Polygon {
            outer: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)],
            holes: vec![vec![Point::new(4.0, 4.0), Point::new(6.0, 4.0), Point::new(6.0, 6.0), Point::new(4.0, 6.0)]],
        });
        let tol = FlattenTolerance::default();
        assert!(poly.contains_point(Point::new(1.0, 1.0), tol));
        assert!(!poly.contains_point(Point::new(5.0, 5.0), tol));
        assert!(!poly.contains_point(Point::new(11.0, 5.0), tol));
        assert!((poly.area() - 96.0).abs() < 1e-12);
    }

    #[test]
    fn polyline_with_bulge_area_and_length() {
        // Half disc: diameter from (-1,0) to (1,0) closed by a CCW semicircle.
        let p = Shape::Polyline(Polyline {
            points: vec![Point::new(-1.0, 0.0), Point::new(1.0, 0.0)],
            bulges: vec![0.0, 1.0],
            closed: true,
        });
        assert!((p.area() - PI / 2.0).abs() < 1e-12, "{}", p.area());
        assert!((p.length() - (2.0 + PI)).abs() < 1e-12);
    }

    #[test]
    fn crossing_vs_window_selection() {
        let l = Shape::Line(Segment::new(Point::new(0.0, 0.0), Point::new(10.0, 0.0)));
        let r = Aabb::from_corners(Point::new(4.0, -1.0), Point::new(6.0, 1.0));
        assert!(l.intersects_rect(r, ModelTolerance::DEFAULT));
        assert!(!l.inside_rect(r));
        let big = Aabb::from_corners(Point::new(-1.0, -1.0), Point::new(11.0, 1.0));
        assert!(l.inside_rect(big));
        let circle = Shape::Circle(Circle::new(Point::ORIGIN, 10.0).unwrap());
        let inner = Aabb::from_corners(Point::new(-1.0, -1.0), Point::new(1.0, 1.0));
        assert!(circle.intersects_rect(inner, ModelTolerance::DEFAULT));
    }

    #[test]
    fn validate_rejects_bad_input() {
        assert!(Shape::Line(Segment::new(Point::new(f64::NAN, 0.0), Point::ORIGIN)).validate().is_err());
        assert!(
            Shape::Polyline(Polyline {
                points: vec![Point::ORIGIN, Point::new(1.0, 0.0)],
                bulges: vec![0.1, 0.2],
                closed: false
            })
            .validate()
            .is_err()
        );
        assert!(Shape::Path(Path { elements: vec![PathEl::LineTo(Point::ORIGIN)] }).validate().is_err());
        assert!(Shape::Circle(Circle { center: Point::ORIGIN, radius: 0.0 }).validate().is_err());
    }

    #[test]
    fn line_layout_matches_harfbuzz_kerning() {
        // Reference widths in font units from HarfBuzz 11 (uharfbuzz 0.56.2) shaping the
        // same font with only `kern` enabled — an independent implementation of
        // GPOS pair positioning (class pairs, explicit pairs, two lookups).
        let cases = [
            ("AV", 2686.0),
            ("To", 2390.0),
            ("Yo", 2461.0),
            ("Wa", 3064.0),
            ("LT", 2283.0),
            ("Ölçü planı — İğdır", 17152.0),
            ("TAVERN", 7926.0),
            ("P.", 1829.0),
            ("f)", 1505.0),
            ("Tığ", 3074.0),
            ("kv", 2275.0),
            ("Hello", 4936.0),
            ("AV\tA", 4675.0),
        ];
        for (s, expected) in cases {
            let (pens, width) = Text::line_pens(s);
            assert_eq!(width, expected, "{s}");
            assert_eq!(pens.len(), s.chars().count(), "{s}");
        }
        // Kerning really applies: "AV" is narrower than its advances.
        let (pens, _) = Text::line_pens("AV");
        let a_advance = pens[1].1;
        let (_, a_alone) = Text::line_pens("A");
        assert!(a_advance < a_alone, "{a_advance} vs {a_alone}");
        // Model units scale with the text height; controls are dropped.
        let h = 10.0;
        assert!((Text::line_width("AV", h) - 2686.0 / 2048.0 * Text::em_size(h)).abs() < 1e-12);
        assert_eq!(Text::line_pens("A\u{7}V").1, 2686.0);
    }

    #[test]
    fn text_similarity_only() {
        let t = Shape::Text(Text {
            position: Point::new(1.0, 1.0),
            content: "Ölçü ğüşıİç".into(),
            height: 2.5,
            rotation: 0.0,
            halign: HAlign::Center,
            valign: VAlign::Middle,
        });
        let r = t.transform(Affine::rotate(PI / 2.0).then(Affine::scale(2.0, 2.0)), TransformPolicy::Strict).unwrap();
        if let Shape::Text(tx) = r {
            assert!((tx.height - 5.0).abs() < 1e-12);
            assert!((tx.rotation - PI / 2.0).abs() < 1e-12);
        } else {
            unreachable!();
        }
        assert!(t.transform(Affine::scale(2.0, 1.0), TransformPolicy::Convert).is_err());
        // Width follows the default font's advances, character by character
        // (checked against the font file in dotloom-render).
        let b = t.bbox();
        let w = Text::line_width("Ölçü ğüşıİç", 2.5);
        assert!((b.width() - w).abs() < 1e-9);
        assert!(w > 11.0 * 2.5 * 0.4 && w < 11.0 * 2.5 * 0.8, "{w}");
    }
}
