//! SVG import and export.
//!
//! **Export** writes world geometry (model Y up → SVG Y down) in millimetres, one
//! `<g>` per layer, styles as attributes, and reports what SVG cannot carry
//! (constraints, plugin parameters, associative dimensions).
//!
//! **Import** supports `svg`, `g`, `line`, `rect`, `circle`, `ellipse`, `polyline`,
//! `polygon`, `path` (all commands; elliptical arcs become Béziers) and `text`, with
//! nested `transform`s, `viewBox` and absolute size units, and `stroke`/`fill`/
//! `stroke-width` presentation attributes or inline `style`. DTDs are rejected;
//! `script`, `style`, `use`, `image`, `foreignObject`, gradients, filters, masks and
//! clip paths are skipped and reported; `href`s are never followed.

use core::f64::consts::{PI, TAU};
use std::fmt::Write as _;

use dotloom_document::{Color, LayerId, Style};
use dotloom_engine::{Engine, NewEntity, document::builtin::is_builtin, scene::Primitive};
use dotloom_geometry::{
    Affine, Circle, CubicBez, HAlign, Path, PathEl, Point, Polyline, Rect, Segment, Shape, Text, TransformPolicy,
    VAlign, Vector, arc_to_cubics,
};
use thiserror::Error;

use crate::report::{ConversionReport, LossKind, Recorder};

/// SVG errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SvgError {
    /// XML is malformed, uses a DTD or exceeds limits.
    #[error("invalid svg: {0}")]
    Xml(String),
    /// Not an SVG document.
    #[error("root element is not <svg>")]
    NotSvg,
    /// Input too large.
    #[error("svg input exceeds {0} bytes")]
    TooLarge(usize),
}

/// Export options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgExportOptions {
    /// Margin around the drawing (mm).
    pub margin: f64,
    /// Color used for theme-default strokes and text.
    pub foreground: Color,
    /// Optional background rectangle.
    pub background: Option<Color>,
}

impl Default for SvgExportOptions {
    fn default() -> Self {
        Self { margin: 10.0, foreground: Color::BLACK, background: None }
    }
}

fn num(v: f64) -> String {
    let s = format!("{v:.6}");
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() { "0".into() } else { t.to_owned() }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

fn color(c: u32, fg: Color) -> (String, Option<f64>) {
    let c = if c == 0 { fg.0 } else { c };
    let [r, g, b, a] = c.to_be_bytes();
    (format!("#{r:02x}{g:02x}{b:02x}"), (a != 0xff).then(|| f64::from(a) / 255.0))
}

fn p(pt: Point) -> String {
    format!("{} {}", num(pt.x), num(-pt.y))
}

fn arc_path(a: &dotloom_geometry::Arc, start_move: bool) -> String {
    let mut d = String::new();
    // Split full circles (and sweeps ≥ 2π) into two halves.
    let pieces = if a.sweep.abs() >= TAU - 1e-12 { 2 } else { 1 };
    let step = a.sweep / f64::from(pieces);
    if start_move {
        let _ = write!(d, "M{}", p(a.start_point()));
    }
    for k in 0..pieces {
        let sub = a.subarc(f64::from(k) / f64::from(pieces), f64::from(k + 1) / f64::from(pieces));
        let large = u8::from(step.abs() > PI);
        // Model CCW (sweep > 0) becomes clockwise on the Y-down canvas: sweep-flag 1.
        let sweep_flag = u8::from(step > 0.0);
        let _ = write!(d, " A{} {} 0 {} {} {}", num(a.radius), num(a.radius), large, sweep_flag, p(sub.end_point()));
    }
    d
}

fn shape_element(s: &Shape) -> Option<(String, String)> {
    Some(match s {
        Shape::Point(pt) => ("circle".into(), format!(r#"cx="{}" cy="{}" r="0.5""#, num(pt.at.x), num(-pt.at.y))),
        Shape::Line(l) => (
            "line".into(),
            format!(r#"x1="{}" y1="{}" x2="{}" y2="{}""#, num(l.a.x), num(-l.a.y), num(l.b.x), num(-l.b.y)),
        ),
        Shape::Rect(r) => (
            "rect".into(),
            format!(
                r#"x="{}" y="{}" width="{}" height="{}""#,
                num(r.origin.x),
                num(-(r.origin.y + r.height)),
                num(r.width),
                num(r.height)
            ),
        ),
        Shape::Circle(c) => {
            ("circle".into(), format!(r#"cx="{}" cy="{}" r="{}""#, num(c.center.x), num(-c.center.y), num(c.radius)))
        }
        Shape::Arc(a) => ("path".into(), format!(r#"d="{}""#, arc_path(a, true))),
        Shape::Polyline(pl) if !pl.has_arcs() => {
            let pts: Vec<String> = pl.points.iter().map(|q| format!("{},{}", num(q.x), num(-q.y))).collect();
            (if pl.closed { "polygon" } else { "polyline" }.into(), format!(r#"points="{}""#, pts.join(" ")))
        }
        Shape::Polyline(pl) => {
            let mut d = String::new();
            if let Some(f) = pl.points.first() {
                let _ = write!(d, "M{}", p(*f));
            }
            for i in 0..pl.segment_count() {
                match pl.segment(i)? {
                    dotloom_geometry::Curve::Arc(a) => d.push_str(&arc_path(&a, false)),
                    c => {
                        let _ = write!(d, " L{}", p(c.end()));
                    }
                }
            }
            if pl.closed {
                d.push_str(" Z");
            }
            ("path".into(), format!(r#"d="{d}""#))
        }
        Shape::Polygon(pg) => {
            let mut d = String::new();
            for ring in core::iter::once(&pg.outer).chain(pg.holes.iter()) {
                for (i, q) in ring.iter().enumerate() {
                    let _ = write!(d, "{}{}", if i == 0 { " M" } else { " L" }, p(*q));
                }
                d.push_str(" Z");
            }
            ("path".into(), format!(r#"d="{}" fill-rule="evenodd""#, d.trim()))
        }
        Shape::Path(path) => {
            let mut d = String::new();
            for el in &path.elements {
                let _ = match *el {
                    PathEl::MoveTo(a) => write!(d, " M{}", p(a)),
                    PathEl::LineTo(a) => write!(d, " L{}", p(a)),
                    PathEl::QuadTo(a, b) => write!(d, " Q{} {}", p(a), p(b)),
                    PathEl::CubicTo(a, b, c) => write!(d, " C{} {} {}", p(a), p(b), p(c)),
                    PathEl::Close => write!(d, " Z"),
                };
            }
            ("path".into(), format!(r#"d="{}""#, d.trim()))
        }
        Shape::Text(_) => return None,
    })
}

fn text_element(t: &Text, fill: &str) -> String {
    let anchor = match t.halign {
        HAlign::Left => "start",
        HAlign::Center => "middle",
        HAlign::Right => "end",
    };
    let baseline = match t.valign {
        VAlign::Baseline => "alphabetic",
        VAlign::Middle => "middle",
        VAlign::Top => "hanging",
        VAlign::Bottom => "text-after-edge",
    };
    let (x, y) = (num(t.position.x), num(-t.position.y));
    let rot = if t.rotation == 0.0 {
        String::new()
    } else {
        format!(r#" transform="rotate({} {x} {y})""#, num(-t.rotation.to_degrees()))
    };
    let lines: Vec<&str> = t.content.split('\n').collect();
    let body = if lines.len() == 1 {
        esc(&t.content)
    } else {
        lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                format!(
                    r#"<tspan x="{x}" dy="{}">{}</tspan>"#,
                    if i == 0 { "0".into() } else { num(t.height * Text::LINE_SPACING) },
                    esc(l)
                )
            })
            .collect()
    };
    format!(
        r#"<text x="{x}" y="{y}" font-size="{}" font-family="Noto Sans, sans-serif" text-anchor="{anchor}" dominant-baseline="{baseline}" fill="{fill}"{rot}>{body}</text>"#,
        num(t.height)
    )
}

/// Export the engine's committed document as SVG.
pub fn export_svg(engine: &mut Engine, opts: &SvgExportOptions) -> (String, ConversionReport) {
    let mut rec = Recorder::default();
    let scene = engine.full_scene();
    let doc = engine.document();
    let constraints = doc.constraint_count();
    if constraints > 0 {
        for _ in 0..constraints {
            rec.add(LossKind::Constraints, "constraint");
        }
    }
    let bbox = scene.upserts.iter().fold(dotloom_geometry::Aabb::EMPTY, |b, i| b.union(i.bbox));
    let bbox = if bbox.is_empty() {
        dotloom_geometry::Aabb::from_corners(Point::ORIGIN, Point::new(100.0, 100.0))
    } else {
        bbox
    };
    let m = opts.margin.max(0.0);
    let (x0, y0) = (bbox.min.x - m, -(bbox.max.y + m));
    let (w, h) = (bbox.width() + 2.0 * m, bbox.height() + 2.0 * m);
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="{}mm" height="{}mm" viewBox="{} {} {} {}">
"#,
        num(w),
        num(h),
        num(x0),
        num(y0),
        num(w),
        num(h)
    );
    if let Some(bg) = opts.background {
        let (c, _) = color(bg.0, opts.foreground);
        let _ = writeln!(
            out,
            r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{c}"/>"#,
            num(x0),
            num(y0),
            num(w),
            num(h)
        );
    }
    let layers: Vec<(LayerId, String)> = doc.layers().iter().map(|l| (l.id, l.name.clone())).collect();
    let order: Vec<u64> = scene.order.clone().unwrap_or_default();
    for (li, (lid, name)) in layers.iter().enumerate() {
        let items: Vec<&dotloom_engine::scene::SceneItem> = order
            .iter()
            .filter_map(|id| scene.upserts.iter().find(|i| i.id == *id))
            .filter(|i| i.layer as usize == li)
            .collect();
        if items.is_empty() {
            continue;
        }
        let _ = writeln!(out, r#"<g id="layer-{}" inkscape:groupmode="layer" inkscape:label="{}">"#, lid.0, esc(name));
        for item in items {
            let Some(e) = doc.entity(dotloom_document::EntityId(item.id)) else { continue };
            rec.entities += 1;
            if !is_builtin(&e.type_id) {
                rec.add(
                    if item.flags & dotloom_engine::scene::flags::READONLY != 0 {
                        LossKind::Fallback
                    } else {
                        LossKind::PluginGeometry
                    },
                    e.type_id.to_string(),
                );
            } else if e.type_id.as_str() == dotloom_document::builtin::types::DIMENSION {
                rec.add(LossKind::Dimensions, "dimension");
            }
            let _ =
                writeln!(out, r#"<g data-dotloom-id="{}" data-dotloom-type="{}">"#, item.id, esc(e.type_id.as_str()));
            for prim in &item.prims {
                match prim {
                    Primitive::Shape { shape, stroke, fill } => {
                        let Some((tag, geom)) = shape_element(shape) else { continue };
                        let mut attrs = geom;
                        match fill {
                            Some(f) => {
                                let (c, a) = color(*f, opts.foreground);
                                let _ = write!(attrs, r#" fill="{c}""#);
                                if let Some(a) = a {
                                    let _ = write!(attrs, r#" fill-opacity="{}""#, num(a));
                                }
                            }
                            None => attrs.push_str(r#" fill="none""#),
                        }
                        match stroke {
                            Some(s) => {
                                let (c, a) = color(s.color, opts.foreground);
                                let _ = write!(
                                    attrs,
                                    r#" stroke="{c}" stroke-width="{}" vector-effect="non-scaling-stroke""#,
                                    num(f64::from(s.width))
                                );
                                if let Some(a) = a {
                                    let _ = write!(attrs, r#" stroke-opacity="{}""#, num(a));
                                }
                                if !s.dash.is_empty() {
                                    let d: Vec<String> = s.dash.iter().map(|x| num(f64::from(*x))).collect();
                                    let _ = write!(attrs, r#" stroke-dasharray="{}""#, d.join(" "));
                                }
                            }
                            None => attrs.push_str(r#" stroke="none""#),
                        }
                        let _ = writeln!(out, "<{tag} {attrs}/>");
                    }
                    Primitive::Text { text, color: c } => {
                        let (c, _) = color(*c, opts.foreground);
                        let _ = writeln!(out, "{}", text_element(text, &c));
                    }
                    Primitive::Arrow { tip, direction, size, color: c } => {
                        let (c, _) = color(*c, opts.foreground);
                        let back = *tip - *direction * *size;
                        let n = direction.perp() * (*size * 0.3);
                        let _ = writeln!(
                            out,
                            r#"<polygon points="{},{} {},{} {},{}" fill="{c}"/>"#,
                            num(tip.x),
                            num(-tip.y),
                            num((back + n).x),
                            num(-(back + n).y),
                            num((back - n).x),
                            num(-(back - n).y)
                        );
                    }
                }
            }
            out.push_str("</g>\n");
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    rec.notes.push("units: millimetres; model Y axis flipped to SVG Y-down".into());
    (out, rec.finish())
}

// ---------------------------------------------------------------------------
// Import

/// Result of an SVG import: layers and entities ready for `Command::CreateEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgImport {
    /// Layer names (top-level groups), first entry is the default layer.
    pub layers: Vec<String>,
    /// `(layer index, entity)`.
    pub entities: Vec<(usize, NewEntity)>,
    /// Report.
    pub report: ConversionReport,
}

/// Maximum SVG input size.
pub const MAX_SVG_BYTES: usize = 64 << 20;

#[derive(Debug, Clone, Default)]
struct Paint {
    stroke: Option<Option<Color>>,
    fill: Option<Option<Color>>,
    width: Option<f64>,
    hidden: bool,
}

fn parse_color(s: &str, rec: &mut Recorder) -> Option<Option<Color>> {
    let s = s.trim();
    if s.is_empty() || s == "inherit" {
        return None;
    }
    if s == "none" || s == "transparent" {
        return Some(None);
    }
    if s.starts_with('#') {
        return match Color::parse(s) {
            Ok(c) => Some(Some(c)),
            Err(_) => {
                rec.add(LossKind::Style, "color");
                None
            }
        };
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|x| x.strip_suffix(')')) {
        let v: Vec<u8> = inner
            .split(',')
            .filter_map(|c| c.trim().trim_end_matches('%').parse::<f64>().ok())
            .map(|c| c.clamp(0.0, 255.0) as u8)
            .collect();
        if let [r, g, b] = v.as_slice() {
            return Some(Some(Color::rgba(*r, *g, *b, 255)));
        }
    }
    let named = match s {
        "black" => Some(Color::rgba(0, 0, 0, 255)),
        "white" => Some(Color::rgba(255, 255, 255, 255)),
        "red" => Some(Color::rgba(255, 0, 0, 255)),
        "green" => Some(Color::rgba(0, 128, 0, 255)),
        "blue" => Some(Color::rgba(0, 0, 255, 255)),
        "gray" | "grey" => Some(Color::rgba(128, 128, 128, 255)),
        "yellow" => Some(Color::rgba(255, 255, 0, 255)),
        "orange" => Some(Color::rgba(255, 165, 0, 255)),
        _ => None,
    };
    if named.is_none() {
        rec.add(LossKind::Style, if s.starts_with("url(") { "paint server (gradient/pattern)" } else { "color" });
    }
    named.map(Some)
}

fn length_mm(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(s.len());
    let (n, u) = s.split_at(split);
    let v: f64 = n.trim().parse().ok()?;
    let mm = match u.trim() {
        "" | "px" => v * 25.4 / 96.0,
        "mm" => v,
        "cm" => v * 10.0,
        "in" => v * 25.4,
        "pt" => v * 25.4 / 72.0,
        "pc" => v * 25.4 / 6.0,
        _ => return None,
    };
    mm.is_finite().then_some(mm)
}

fn numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut seen_dot = c == b'.';
            let mut seen_e = false;
            while i < b.len() {
                let d = b[i];
                if d.is_ascii_digit() {
                    i += 1;
                } else if d == b'.' && !seen_dot && !seen_e {
                    seen_dot = true;
                    i += 1;
                } else if (d == b'e' || d == b'E') && !seen_e {
                    seen_e = true;
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = s[start..i].parse::<f64>()
                && v.is_finite()
            {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn parse_transform(s: &str) -> Affine {
    let mut t = Affine::IDENTITY;
    let mut rest = s;
    // SVG lists apply right-to-left: "A B" means A(B(x)); compose in reading order.
    let mut list = Vec::new();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else { break };
        let args = numbers(&rest[open + 1..open + close]);
        let m = match (name, args.as_slice()) {
            ("matrix", [a, b, c, d, e, f]) => Affine { m: [*a, *b, *c, *d, *e, *f] },
            ("translate", [x]) => Affine::translate(Vector::new(*x, 0.0)),
            ("translate", [x, y]) => Affine::translate(Vector::new(*x, *y)),
            ("scale", [s]) => Affine::scale(*s, *s),
            ("scale", [x, y]) => Affine::scale(*x, *y),
            ("rotate", [a]) => Affine::rotate(a.to_radians()),
            ("rotate", [a, cx, cy]) => Affine::rotate_about(a.to_radians(), Point::new(*cx, *cy)),
            ("skewX", [a]) => Affine { m: [1.0, 0.0, dotloom_geometry::math::tan(a.to_radians()), 1.0, 0.0, 0.0] },
            ("skewY", [a]) => Affine { m: [1.0, dotloom_geometry::math::tan(a.to_radians()), 0.0, 1.0, 0.0, 0.0] },
            _ => Affine::IDENTITY,
        };
        list.push(m);
        rest = &rest[open + close + 1..];
    }
    for m in list.into_iter().rev() {
        t = t.then(m);
    }
    t
}

fn style_map(node: &roxmltree::Node<'_, '_>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    for key in ["stroke", "fill", "stroke-width", "display", "visibility"] {
        if let Some(val) = node.attribute(key) {
            v.push((key.into(), val.into()));
        }
    }
    if let Some(st) = node.attribute("style") {
        for decl in st.split(';') {
            if let Some((k, val)) = decl.split_once(':') {
                v.push((k.trim().to_owned(), val.trim().to_owned()));
            }
        }
    }
    v
}

struct Importer<'r> {
    rec: &'r mut Recorder,
    out: Vec<(usize, NewEntity)>,
    layers: Vec<String>,
    unit_scale: f64,
    elements: usize,
}

fn ellipse_path(c: Point, rx: f64, ry: f64) -> Option<Path> {
    let arc = dotloom_geometry::Arc::new(Point::ORIGIN, 1.0, 0.0, TAU).ok()?;
    let mut elements = Vec::new();
    let s = Affine::scale(rx, ry).then(Affine::translate(c.to_vector()));
    for (i, cv) in arc_to_cubics(arc).into_iter().enumerate() {
        if let dotloom_geometry::Curve::Cubic(b) = cv {
            let b = b.transform(s);
            if i == 0 {
                elements.push(PathEl::MoveTo(b.p0));
            }
            elements.push(PathEl::CubicTo(b.p1, b.p2, b.p3));
        }
    }
    elements.push(PathEl::Close);
    Some(Path { elements })
}

/// SVG elliptical arc (endpoint form) to cubic Béziers.
#[allow(clippy::too_many_arguments)]
fn svg_arc(p0: Point, rx: f64, ry: f64, phi_deg: f64, large: bool, sweep: bool, p1: Point, out: &mut Vec<PathEl>) {
    if p0 == p1 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(PathEl::LineTo(p1));
        return;
    }
    let phi = phi_deg.to_radians();
    let (s, c) = dotloom_geometry::math::sin_cos(phi);
    let dx = (p0.x - p1.x) / 2.0;
    let dy = (p0.y - p1.y) / 2.0;
    let x1 = c * dx + s * dy;
    let y1 = -s * dx + c * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let k = lambda.sqrt();
        rx *= k;
        ry *= k;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 { (num / den).max(0.0).sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * (rx * y1 / ry);
    let cy1 = coef * (-ry * x1 / rx);
    let cx = c * cx1 - s * cy1 + (p0.x + p1.x) / 2.0;
    let cy = s * cx1 + c * cy1 + (p0.y + p1.y) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| dotloom_geometry::math::atan2(ux * vy - uy * vx, ux * vx + uy * vy);
    let th1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dth = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dth > 0.0 {
        dth -= TAU;
    } else if sweep && dth < 0.0 {
        dth += TAU;
    }
    let Ok(unit) = dotloom_geometry::Arc::new(Point::ORIGIN, 1.0, th1, dth) else {
        out.push(PathEl::LineTo(p1));
        return;
    };
    let t = Affine::scale(rx, ry).then(Affine::rotate(phi)).then(Affine::translate(Vector::new(cx, cy)));
    for cv in arc_to_cubics(unit) {
        if let dotloom_geometry::Curve::Cubic(b) = cv {
            let b: CubicBez = b.transform(t);
            out.push(PathEl::CubicTo(b.p1, b.p2, b.p3));
        }
    }
    if let Some(PathEl::CubicTo(_, _, last)) = out.last_mut() {
        *last = p1;
    }
}

fn parse_path(d: &str, rec: &mut Recorder) -> Option<Path> {
    let mut els: Vec<PathEl> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    let mut cmd = b'M';
    let mut cur = Point::ORIGIN;
    let mut start = Point::ORIGIN;
    let mut last_ctrl: Option<Point> = None;
    let mut last_q: Option<Point> = None;
    let mut arcs = false;
    let mut steps = 0usize;
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        steps += 1;
        if steps > 2_000_000 {
            return None;
        }
        if b[i].is_ascii_alphabetic() {
            cmd = b[i];
            i += 1;
            if cmd == b'Z' || cmd == b'z' {
                els.push(PathEl::Close);
                cur = start;
                last_ctrl = None;
                last_q = None;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let arity = match cmd.to_ascii_uppercase() {
            b'M' | b'L' | b'T' => 2,
            b'H' | b'V' => 1,
            b'C' => 6,
            b'S' | b'Q' => 4,
            b'A' => 7,
            _ => return None,
        };
        // Read `arity` numbers (arc flags may be written without separators).
        let mut vals = Vec::with_capacity(arity);
        while vals.len() < arity {
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            if cmd.eq_ignore_ascii_case(&b'A') && (vals.len() == 3 || vals.len() == 4) && (b[i] == b'0' || b[i] == b'1')
            {
                vals.push(f64::from(b[i] - b'0'));
                i += 1;
                continue;
            }
            let s = i;
            if b[i] == b'-' || b[i] == b'+' {
                i += 1;
            }
            let mut dot = false;
            let mut exp = false;
            while i < b.len() {
                let c = b[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == b'.' && !dot && !exp {
                    dot = true;
                    i += 1;
                } else if (c == b'e' || c == b'E') && !exp {
                    exp = true;
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            let v: f64 = d.get(s..i)?.parse().ok()?;
            if !v.is_finite() {
                return None;
            }
            vals.push(v);
        }
        if vals.len() < arity {
            break;
        }
        let pt = |x: f64, y: f64| if rel { Point::new(cur.x + x, cur.y + y) } else { Point::new(x, y) };
        match cmd.to_ascii_uppercase() {
            b'M' => {
                cur = pt(vals[0], vals[1]);
                start = cur;
                els.push(PathEl::MoveTo(cur));
                cmd = if rel { b'l' } else { b'L' };
                last_ctrl = None;
                last_q = None;
            }
            b'L' => {
                cur = pt(vals[0], vals[1]);
                els.push(PathEl::LineTo(cur));
                last_ctrl = None;
                last_q = None;
            }
            b'H' => {
                cur = Point::new(if rel { cur.x + vals[0] } else { vals[0] }, cur.y);
                els.push(PathEl::LineTo(cur));
                last_ctrl = None;
                last_q = None;
            }
            b'V' => {
                cur = Point::new(cur.x, if rel { cur.y + vals[0] } else { vals[0] });
                els.push(PathEl::LineTo(cur));
                last_ctrl = None;
                last_q = None;
            }
            b'C' => {
                let (c1, c2, e) = (pt(vals[0], vals[1]), pt(vals[2], vals[3]), pt(vals[4], vals[5]));
                els.push(PathEl::CubicTo(c1, c2, e));
                last_ctrl = Some(c2);
                last_q = None;
                cur = e;
            }
            b'S' => {
                let c1 = last_ctrl.map_or(cur, |c| cur + (cur - c));
                let (c2, e) = (pt(vals[0], vals[1]), pt(vals[2], vals[3]));
                els.push(PathEl::CubicTo(c1, c2, e));
                last_ctrl = Some(c2);
                last_q = None;
                cur = e;
            }
            b'Q' => {
                let (c, e) = (pt(vals[0], vals[1]), pt(vals[2], vals[3]));
                els.push(PathEl::QuadTo(c, e));
                last_q = Some(c);
                last_ctrl = None;
                cur = e;
            }
            b'T' => {
                let c = last_q.map_or(cur, |q| cur + (cur - q));
                let e = pt(vals[0], vals[1]);
                els.push(PathEl::QuadTo(c, e));
                last_q = Some(c);
                last_ctrl = None;
                cur = e;
            }
            b'A' => {
                let e = pt(vals[5], vals[6]);
                svg_arc(cur, vals[0], vals[1], vals[2], vals[3] != 0.0, vals[4] != 0.0, e, &mut els);
                arcs = true;
                cur = e;
                last_ctrl = None;
                last_q = None;
            }
            _ => return None,
        }
    }
    if arcs {
        rec.add(LossKind::Approximated, "elliptical arc → Bézier");
    }
    if !matches!(els.first(), Some(PathEl::MoveTo(_))) {
        return None;
    }
    Some(Path { elements: els })
}

impl Importer<'_> {
    fn emit(&mut self, layer: usize, shape: Shape, paint: &Paint) {
        if shape.validate().is_err() {
            self.rec.add(LossKind::Unsupported, "degenerate shape");
            return;
        }
        let stroke_w = paint.width.map(|w| (w * self.unit_scale * 96.0 / 25.4).clamp(0.0, 1000.0));
        let style = Style {
            stroke: paint.stroke.unwrap_or(Some(Color::BLACK)).filter(|_| paint.stroke != Some(None)),
            fill: paint.fill.flatten().filter(|_| shape.is_region()),
            stroke_width: stroke_w,
            dash: None,
        };
        self.rec.entities += 1;
        self.out.push((layer, NewEntity { geometry: Some(shape), style: Some(style), ..NewEntity::default() }));
    }

    fn walk(&mut self, node: roxmltree::Node<'_, '_>, t: Affine, paint: &Paint, layer: usize, depth: usize) {
        if depth > 256 {
            self.rec.add(LossKind::Unsupported, "nesting deeper than 256");
            return;
        }
        for child in node.children().filter(roxmltree::Node::is_element) {
            self.elements += 1;
            if self.elements > 1_000_000 {
                return;
            }
            let tag = child.tag_name().name();
            if child.attribute("href").is_some() || child.attribute(("http://www.w3.org/1999/xlink", "href")).is_some()
            {
                self.rec.add(LossKind::ExternalReference, format!("href on <{tag}>"));
            }
            let mut p = paint.clone();
            for (k, v) in style_map(&child) {
                match k.as_str() {
                    "stroke" => p.stroke = parse_color(&v, self.rec).or(p.stroke),
                    "fill" => p.fill = parse_color(&v, self.rec).or(p.fill),
                    "stroke-width" => p.width = numbers(&v).first().copied().or(p.width),
                    "display" if v == "none" => p.hidden = true,
                    "visibility" if v == "hidden" => p.hidden = true,
                    _ => {}
                }
            }
            if p.hidden {
                continue;
            }
            let ct = child.attribute("transform").map_or(Affine::IDENTITY, parse_transform).then(t);
            let f = |name: &str| child.attribute(name).and_then(|v| numbers(v).first().copied()).unwrap_or(0.0);
            let tr = |s: Shape| s.transform(ct, TransformPolicy::Convert);
            match tag {
                "g" | "a" | "switch" => {
                    let l = if depth == 0 && tag == "g" {
                        let name = child
                            .attribute(("http://www.inkscape.org/namespaces/inkscape", "label"))
                            .or_else(|| child.attribute("id"))
                            .unwrap_or("Layer");
                        self.layers.push(name.chars().take(256).collect());
                        self.layers.len() - 1
                    } else {
                        layer
                    };
                    self.walk(child, ct, &p, l, depth + 1);
                }
                "line" => {
                    if let Ok(s) =
                        tr(Shape::Line(Segment::new(Point::new(f("x1"), f("y1")), Point::new(f("x2"), f("y2")))))
                    {
                        self.emit(layer, s, &p);
                    }
                }
                "rect" => {
                    if child.attribute("rx").is_some() || child.attribute("ry").is_some() {
                        self.rec.add(LossKind::Approximated, "rounded rect corners");
                    }
                    let r = Rect { origin: Point::new(f("x"), f("y")), width: f("width"), height: f("height") };
                    if let Ok(s) = tr(Shape::Rect(r)) {
                        self.emit(layer, s, &p);
                    }
                }
                "circle" => {
                    let c = Circle { center: Point::new(f("cx"), f("cy")), radius: f("r") };
                    match tr(Shape::Circle(c)) {
                        Ok(s) => {
                            if s.kind() != dotloom_geometry::ShapeKind::Circle {
                                self.rec.add(LossKind::Approximated, "transformed circle → Bézier");
                            }
                            self.emit(layer, s, &p);
                        }
                        Err(_) => self.rec.add(LossKind::Unsupported, "circle"),
                    }
                }
                "ellipse" => {
                    let (rx, ry) = (f("rx"), f("ry"));
                    let c = Point::new(f("cx"), f("cy"));
                    let shape = if (rx - ry).abs() <= 1e-12 * rx.abs().max(1.0) {
                        Some(Shape::Circle(Circle { center: c, radius: rx }))
                    } else {
                        self.rec.add(LossKind::Approximated, "ellipse → Bézier");
                        ellipse_path(c, rx, ry).map(Shape::Path)
                    };
                    if let Some(s) = shape.and_then(|s| tr(s).ok()) {
                        self.emit(layer, s, &p);
                    }
                }
                "polyline" | "polygon" => {
                    let v = numbers(child.attribute("points").unwrap_or(""));
                    let pts: Vec<Point> = v.as_chunks::<2>().0.iter().map(|[x, y]| Point::new(*x, *y)).collect();
                    let pl = Polyline { points: pts, bulges: Vec::new(), closed: tag == "polygon" };
                    if let Ok(s) = tr(Shape::Polyline(pl)) {
                        self.emit(layer, s, &p);
                    }
                }
                "path" => match parse_path(child.attribute("d").unwrap_or(""), self.rec) {
                    Some(path) => {
                        if let Ok(s) = tr(Shape::Path(path)) {
                            self.emit(layer, s, &p);
                        }
                    }
                    None => self.rec.add(LossKind::Unsupported, "malformed path data"),
                },
                "text" => {
                    let content: String = child
                        .descendants()
                        .filter(roxmltree::Node::is_text)
                        .filter_map(|n| n.text())
                        .collect::<Vec<_>>()
                        .join("");
                    if child.descendants().any(|n| n.tag_name().name() == "tspan") {
                        self.rec.add(LossKind::Text, "tspan flattened");
                    }
                    let size = child
                        .attribute("font-size")
                        .and_then(|v| numbers(v).first().copied())
                        .filter(|v| *v > 0.0)
                        .unwrap_or(16.0);
                    let halign = match child.attribute("text-anchor") {
                        Some("middle") => HAlign::Center,
                        Some("end") => HAlign::Right,
                        _ => HAlign::Left,
                    };
                    let t0 = Text {
                        position: Point::new(f("x"), f("y")),
                        content: content.chars().take(10_000).collect(),
                        height: size,
                        rotation: 0.0,
                        halign,
                        valign: VAlign::Baseline,
                    };
                    // Text is upright in SVG (Y down); after the Y flip it must stay readable.
                    let pos = ct.apply(t0.position);
                    let dir = ct.apply_vector(Vector::new(1.0, 0.0));
                    let scale = ct.apply_vector(Vector::new(0.0, 1.0)).length();
                    let text = Text { position: pos, height: size * scale, rotation: dir.angle(), ..t0 };
                    if !text.content.trim().is_empty() {
                        self.emit(layer, Shape::Text(text), &p);
                    }
                }
                "script" => self.rec.add(LossKind::Unsupported, "script (never executed)"),
                "style" => self.rec.add(LossKind::Style, "CSS <style> sheet"),
                "use" | "image" | "foreignObject" | "iframe" | "video" | "audio" => {
                    self.rec.add(LossKind::Unsupported, tag)
                }
                "linearGradient" | "radialGradient" | "pattern" | "filter" | "mask" | "clipPath" | "marker" => {
                    self.rec.add(LossKind::Style, tag)
                }
                "defs" | "symbol" => self.rec.add(LossKind::Unsupported, format!("{tag} content")),
                "title" | "desc" | "metadata" => {}
                other => self.rec.add(LossKind::Unsupported, other.to_owned()),
            }
        }
    }
}

/// Import SVG text into built-in Dotloom geometry (mm, Y up).
pub fn import_svg(text: &str) -> Result<SvgImport, SvgError> {
    if text.len() > MAX_SVG_BYTES {
        return Err(SvgError::TooLarge(MAX_SVG_BYTES));
    }
    let opts =
        roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 2_000_000, ..roxmltree::ParsingOptions::default() };
    let doc = roxmltree::Document::parse_with_options(text, opts).map_err(|e| SvgError::Xml(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(SvgError::NotSvg);
    }
    let mut rec = Recorder::default();
    // User units → mm, with the Y axis flipped.
    let vb = root.attribute("viewBox").map(numbers).filter(|v| v.len() == 4 && v[2] > 0.0 && v[3] > 0.0);
    // The viewBox only selects the visible area; user coordinates are kept absolute.
    let (sx, sy) =
        match (&vb, root.attribute("width").and_then(length_mm), root.attribute("height").and_then(length_mm)) {
            (Some(v), Some(w), Some(h)) => (w / v[2], h / v[3]),
            (Some(_), _, _) => {
                rec.notes.push("no absolute size: 1 user unit = 1 px at 96 dpi".into());
                (25.4 / 96.0, 25.4 / 96.0)
            }
            _ => {
                rec.notes.push("no viewBox: 1 user unit = 1 px at 96 dpi".into());
                (25.4 / 96.0, 25.4 / 96.0)
            }
        };
    if (sx - sy).abs() > 1e-9 * sx.abs().max(sy.abs()) {
        rec.add(LossKind::Units, "non-uniform viewBox scaling (preserveAspectRatio ignored)");
    }
    let to_model = Affine::scale(sx, -sy);
    let mut imp =
        Importer { rec: &mut rec, out: Vec::new(), layers: vec!["Imported".into()], unit_scale: sx, elements: 0 };
    let paint = Paint::default();
    imp.walk(root, to_model, &paint, 0, 0);
    let (layers, entities) = (imp.layers, imp.out);
    rec.notes.push(format!("scale: {} mm per user unit", num(sx)));
    Ok(SvgImport { layers, entities, report: rec.finish() })
}
