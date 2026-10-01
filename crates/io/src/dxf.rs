//! ASCII DXF import and export.
//!
//! Import supports `LINE`, `LWPOLYLINE` (bulges), 2D `POLYLINE`/`VERTEX` (bulges),
//! `CIRCLE`, `ARC`, `TEXT`, `MTEXT` (formatting stripped), `POINT` and the `LAYER`
//! table, for AC1009 (R12) through AC1032 (2018). Units come from `$INSUNITS`
//! (unitless files are read as millimetres and reported). Entities with an
//! extrusion of `(0, 0, -1)` are mirrored correctly; other OCS orientations are
//! reported. `INSERT`, `HATCH`, `SPLINE`, `ELLIPSE`, `DIMENSION` and every other
//! entity are counted in the report — never dropped silently. Binary DXF is
//! rejected. Text decoding: UTF-8 (AC1021+), otherwise `$DWGCODEPAGE`
//! ANSI_1252/ANSI_1254 plus `\U+XXXX` escapes.
//!
//! Export writes R12 (AC1009) ASCII — the most widely readable variant — with
//! `LINE`, `POLYLINE`/`VERTEX` (bulges), `CIRCLE`, `ARC`, `TEXT` and `POINT`, layers
//! and `$INSUNITS = 4` (mm). Béziers are flattened and reported.

use core::f64::consts::TAU;
use std::{collections::BTreeMap, fmt::Write as _};

use dotloom_document::{Color, LayerId, Style};
use dotloom_engine::{Engine, NewEntity, document::builtin::is_builtin, scene::Primitive};
use dotloom_geometry::{
    Arc, Circle, FlattenTolerance, HAlign, Point, PointShape, Polyline, Segment, Shape, Text, VAlign, normalize_angle,
    units::LengthUnit,
};
use thiserror::Error;

use crate::report::{ConversionReport, LossKind, Recorder};

/// DXF errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DxfError {
    /// Binary DXF.
    #[error("binary DXF is not supported; save as ASCII DXF")]
    Binary,
    /// Malformed group codes.
    #[error("malformed DXF at line {line}: {msg}")]
    Malformed {
        /// Line number (1-based).
        line: usize,
        /// Message.
        msg: String,
    },
    /// Input too large.
    #[error("DXF input exceeds {0} bytes")]
    TooLarge(usize),
}

/// Maximum DXF input size.
pub const MAX_DXF_BYTES: usize = 256 << 20;

/// Imported layers and entities.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfImport {
    /// Layer names with optional colors; index 0 is the DXF layer `0`.
    pub layers: Vec<(String, Option<Color>)>,
    /// `(layer index, entity)`.
    pub entities: Vec<(usize, NewEntity)>,
    /// Detected version (`$ACADVER`).
    pub version: Option<String>,
    /// Unit applied.
    pub unit: LengthUnit,
    /// Report.
    pub report: ConversionReport,
}

const CP1254_HIGH: [(u8, char); 6] = [(0xD0, 'Ğ'), (0xDD, 'İ'), (0xDE, 'Ş'), (0xF0, 'ğ'), (0xFD, 'ı'), (0xFE, 'ş')];
const CP1252_80: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’',
    '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

fn decode_legacy(bytes: &[u8], turkish: bool) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else if (0x80..0xA0).contains(&b) {
                CP1252_80.get((b - 0x80) as usize).copied().unwrap_or('?')
            } else if turkish && let Some((_, c)) = CP1254_HIGH.iter().find(|(x, _)| *x == b) {
                *c
            } else {
                char::from_u32(u32::from(b)).unwrap_or('?')
            }
        })
        .collect()
}

/// Replace `\U+XXXX` escapes.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("\\U+") {
        out.push_str(&rest[..i]);
        let hex = rest.get(i + 3..i + 7).unwrap_or("");
        match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
            Some(c) if hex.len() == 4 => {
                out.push(c);
                rest = &rest[i + 7..];
            }
            _ => {
                out.push_str("\\U+");
                rest = &rest[i + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Strip MTEXT formatting codes (`\P` → newline, `{\f...;}` groups, `\~`).
fn strip_mtext(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('P' | 'n') => out.push('\n'),
                Some('~') => out.push(' '),
                Some('\\') => out.push('\\'),
                Some('{') => out.push('{'),
                Some('}') => out.push('}'),
                Some(_) => {
                    // Skip a formatting code up to its terminating ';' (if any on this run).
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            '{' | '}' => {}
            c => out.push(c),
        }
    }
    out
}

struct Pairs<'a> {
    lines: Vec<&'a str>,
    i: usize,
}

impl<'a> Pairs<'a> {
    fn next(&mut self) -> Option<Result<(i32, &'a str, usize), DxfError>> {
        let code = *self.lines.get(self.i)?;
        // A trailing blank line (or an empty file) ends the stream.
        if code.trim().is_empty() && self.i + 1 >= self.lines.len() {
            return None;
        }
        let value = self.lines.get(self.i + 1).copied().unwrap_or("");
        let line = self.i + 1;
        self.i += 2;
        Some(match code.trim().parse::<i32>() {
            Ok(c) => Ok((c, value.trim_end_matches('\r'), line)),
            Err(_) => Err(DxfError::Malformed { line, msg: format!("expected a group code, found `{}`", code.trim()) }),
        })
    }
}

#[derive(Default)]
struct Ent {
    kind: String,
    codes: Vec<(i32, String)>,
}

impl Ent {
    fn f(&self, code: i32) -> Option<f64> {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.trim().parse().ok())
            .filter(|v: &f64| v.is_finite())
    }
    fn s(&self, code: i32) -> Option<&str> {
        self.codes.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
    fn i(&self, code: i32) -> Option<i64> {
        self.codes.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse().ok())
    }
}

fn aci(index: i64) -> Option<Color> {
    Some(match index {
        1 => Color::rgba(255, 0, 0, 255),
        2 => Color::rgba(255, 255, 0, 255),
        3 => Color::rgba(0, 255, 0, 255),
        4 => Color::rgba(0, 255, 255, 255),
        5 => Color::rgba(0, 0, 255, 255),
        6 => Color::rgba(255, 0, 255, 255),
        8 => Color::rgba(128, 128, 128, 255),
        9 => Color::rgba(192, 192, 192, 255),
        _ => return None,
    })
}

fn insunits(v: i64) -> Option<LengthUnit> {
    Some(match v {
        1 => LengthUnit::Inch,
        2 => LengthUnit::Foot,
        4 => LengthUnit::Millimetre,
        5 => LengthUnit::Centimetre,
        6 => LengthUnit::Metre,
        _ => return None,
    })
}

/// Import ASCII DXF bytes.
pub fn import_dxf(bytes: &[u8]) -> Result<DxfImport, DxfError> {
    if bytes.len() > MAX_DXF_BYTES {
        return Err(DxfError::TooLarge(MAX_DXF_BYTES));
    }
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err(DxfError::Binary);
    }
    let raw = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = match core::str::from_utf8(raw) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            let turkish = raw.windows(10).any(|w| w == b"ANSI_1254\r" || w == b"ANSI_1254\n")
                || raw.windows(9).any(|w| w == b"ANSI_1254");
            decode_legacy(raw, turkish)
        }
    };
    let mut pairs = Pairs { lines: text.split('\n').collect(), i: 0 };
    let mut rec = Recorder::default();
    let mut layers: Vec<(String, Option<Color>)> = vec![("0".into(), None)];
    let mut layer_index: BTreeMap<String, usize> = [("0".to_owned(), 0)].into_iter().collect();
    let mut version = None;
    let mut unit_code: Option<i64> = None;
    let mut section = String::new();
    let mut ents: Vec<Ent> = Vec::new();
    let mut cur: Option<Ent> = None;
    let mut header_var = String::new();
    let mut in_layer_table = false;
    let mut block_count = 0usize;
    let mut steps = 0usize;
    while let Some(p) = pairs.next() {
        let (code, value, _) = p?;
        steps += 1;
        if steps > 50_000_000 {
            return Err(DxfError::TooLarge(MAX_DXF_BYTES));
        }
        if code == 0 {
            if let Some(e) = cur.take() {
                match section.as_str() {
                    "ENTITIES" => ents.push(e),
                    "TABLES" if in_layer_table && e.kind == "LAYER" => {
                        if let Some(name) = e.s(2) {
                            let color = e.i(62).and_then(|c| aci(c.abs()));
                            if !layer_index.contains_key(name) {
                                layer_index.insert(name.to_owned(), layers.len());
                                layers.push((unescape(name), color));
                            }
                        }
                    }
                    _ => {}
                }
            }
            match value {
                "SECTION" => {
                    // The next pair (2, name) names the section.
                    if let Some(Ok((2, name, _))) = pairs.next() {
                        section = name.to_owned();
                    }
                }
                "ENDSEC" => section.clear(),
                "TABLE" => {
                    if let Some(Ok((2, name, _))) = pairs.next() {
                        in_layer_table = name == "LAYER";
                    }
                }
                "ENDTAB" => in_layer_table = false,
                "EOF" => break,
                "BLOCK" if section == "BLOCKS" => block_count += 1,
                other => {
                    if section == "ENTITIES" || section == "TABLES" {
                        cur = Some(Ent { kind: other.to_owned(), codes: Vec::new() });
                    }
                }
            }
            continue;
        }
        if section == "HEADER" {
            if code == 9 {
                header_var = value.to_owned();
            } else if header_var == "$ACADVER" && code == 1 {
                version = Some(value.trim().to_owned());
            } else if header_var == "$INSUNITS" && code == 70 {
                unit_code = value.trim().parse().ok();
            }
            continue;
        }
        if let Some(e) = cur.as_mut() {
            if e.codes.len() > 10_000_000 {
                return Err(DxfError::TooLarge(MAX_DXF_BYTES));
            }
            e.codes.push((code, value.to_owned()));
        }
    }
    if let Some(e) = cur.take()
        && section == "ENTITIES"
    {
        ents.push(e);
    }
    if block_count > 0 {
        rec.notes.push(format!("{block_count} block definition(s) not imported"));
    }
    let unit = match unit_code {
        Some(c) => insunits(c).unwrap_or_else(|| {
            rec.add(LossKind::Units, format!("$INSUNITS {c} read as millimetres"));
            LengthUnit::Millimetre
        }),
        None => {
            rec.add(LossKind::Units, "no $INSUNITS: read as millimetres");
            LengthUnit::Millimetre
        }
    };
    let k = unit.mm_per_unit();
    rec.notes.push(format!("version {}, unit {}", version.as_deref().unwrap_or("unknown"), unit.symbol()));
    let mut out = Vec::new();
    let mut i = 0;
    while i < ents.len() {
        let e = &ents[i];
        let layer = e.s(8).and_then(|n| layer_index.get(n).copied()).unwrap_or(0);
        let color = e.i(62).and_then(aci);
        let mirrored = e.f(230).is_some_and(|z| z < 0.0);
        if e.f(230).is_some_and(|z| z.abs() < 0.999_999)
            || e.f(210).is_some_and(|x| x.abs() > 1e-9)
            || e.f(220).is_some_and(|y| y.abs() > 1e-9)
        {
            rec.add(LossKind::Unsupported, format!("{} with a non-planar extrusion", e.kind));
            i += 1;
            continue;
        }
        // OCS (0,0,-1): x is mirrored.
        let pt = |x: f64, y: f64| Point::new(if mirrored { -x * k } else { x * k }, y * k);
        let mut push = |shape: Shape, rec: &mut Recorder| {
            if shape.validate().is_ok() {
                rec.entities += 1;
                out.push((
                    layer,
                    NewEntity {
                        geometry: Some(shape),
                        style: color.map(|c| Style { stroke: Some(c), ..Style::default() }),
                        ..NewEntity::default()
                    },
                ));
            } else {
                rec.add(LossKind::Unsupported, format!("degenerate {}", e.kind));
            }
        };
        match e.kind.as_str() {
            "LINE" => push(
                Shape::Line(Segment::new(
                    pt(e.f(10).unwrap_or(0.0), e.f(20).unwrap_or(0.0)),
                    pt(e.f(11).unwrap_or(0.0), e.f(21).unwrap_or(0.0)),
                )),
                &mut rec,
            ),
            "POINT" => {
                push(Shape::Point(PointShape { at: pt(e.f(10).unwrap_or(0.0), e.f(20).unwrap_or(0.0)) }), &mut rec)
            }
            "CIRCLE" => push(
                Shape::Circle(Circle {
                    center: pt(e.f(10).unwrap_or(0.0), e.f(20).unwrap_or(0.0)),
                    radius: e.f(40).unwrap_or(0.0) * k,
                }),
                &mut rec,
            ),
            "ARC" => {
                let (a0, a1) = (e.f(50).unwrap_or(0.0).to_radians(), e.f(51).unwrap_or(0.0).to_radians());
                let mut sweep = normalize_angle(a1 - a0);
                if sweep == 0.0 {
                    sweep = TAU;
                }
                let c = pt(e.f(10).unwrap_or(0.0), e.f(20).unwrap_or(0.0));
                // Mirroring flips the direction: start π−a0, sweep negative.
                let (start, sweep) = if mirrored { (core::f64::consts::PI - a0, -sweep) } else { (a0, sweep) };
                match Arc::new(c, e.f(40).unwrap_or(0.0) * k, start, sweep) {
                    Ok(a) => push(Shape::Arc(a), &mut rec),
                    Err(_) => rec.add(LossKind::Unsupported, "degenerate ARC"),
                }
            }
            "LWPOLYLINE" => {
                let closed = e.i(70).is_some_and(|f| f & 1 != 0);
                let mut pts = Vec::new();
                let mut bulges = Vec::new();
                let mut x = None;
                for (c, v) in &e.codes {
                    match c {
                        10 => x = v.trim().parse::<f64>().ok(),
                        20 => {
                            if let (Some(xv), Ok(yv)) = (x.take(), v.trim().parse::<f64>()) {
                                pts.push(pt(xv, yv));
                                bulges.push(0.0);
                            }
                        }
                        42 => {
                            if let (Some(b), Ok(bv)) = (bulges.last_mut(), v.trim().parse::<f64>()) {
                                *b = if mirrored { -bv } else { bv };
                            }
                        }
                        _ => {}
                    }
                }
                if !closed {
                    bulges.pop();
                }
                if bulges.iter().all(|b| *b == 0.0) {
                    bulges.clear();
                }
                if e.codes
                    .iter()
                    .any(|(c, v)| (*c == 40 || *c == 41 || *c == 43) && v.trim().parse::<f64>().is_ok_and(|w| w != 0.0))
                {
                    rec.add(LossKind::Style, "polyline width");
                }
                push(Shape::Polyline(Polyline { points: pts, bulges, closed }), &mut rec);
            }
            "POLYLINE" => {
                let flags = e.i(70).unwrap_or(0);
                let closed = flags & 1 != 0;
                let mut pts = Vec::new();
                let mut bulges = Vec::new();
                let mut j = i + 1;
                while let Some(v) = ents.get(j) {
                    if v.kind != "VERTEX" {
                        break;
                    }
                    pts.push(pt(v.f(10).unwrap_or(0.0), v.f(20).unwrap_or(0.0)));
                    bulges.push(v.f(42).map_or(0.0, |b| if mirrored { -b } else { b }));
                    j += 1;
                }
                if ents.get(j).is_some_and(|s| s.kind == "SEQEND") {
                    j += 1;
                }
                i = j;
                if flags & (8 | 16 | 64) != 0 {
                    rec.add(LossKind::Unsupported, "3D POLYLINE / mesh");
                    continue;
                }
                if !closed {
                    bulges.pop();
                }
                if bulges.iter().all(|b| *b == 0.0) {
                    bulges.clear();
                }
                push(Shape::Polyline(Polyline { points: pts, bulges, closed }), &mut rec);
                continue;
            }
            "TEXT" | "MTEXT" => {
                let is_m = e.kind == "MTEXT";
                let mut content = if is_m {
                    let mut s = String::new();
                    for (c, v) in &e.codes {
                        if *c == 3 || *c == 1 {
                            s.push_str(v);
                        }
                    }
                    rec.add(LossKind::Text, "MTEXT formatting");
                    strip_mtext(&unescape(&s))
                } else {
                    unescape(e.s(1).unwrap_or(""))
                };
                content = content.replace("%%d", "°").replace("%%c", "Ø").replace("%%p", "±");
                let (h_just, v_just) = (e.i(72).unwrap_or(0), e.i(73).unwrap_or(0));
                let use_align = !is_m && (h_just != 0 || v_just != 0) && e.f(11).is_some();
                let pos = if use_align {
                    pt(e.f(11).unwrap_or(0.0), e.f(21).unwrap_or(0.0))
                } else {
                    pt(e.f(10).unwrap_or(0.0), e.f(20).unwrap_or(0.0))
                };
                let (halign, valign) = if is_m {
                    let a = e.i(71).unwrap_or(1);
                    (
                        match (a - 1) % 3 {
                            1 => HAlign::Center,
                            2 => HAlign::Right,
                            _ => HAlign::Left,
                        },
                        match (a - 1) / 3 {
                            0 => VAlign::Top,
                            1 => VAlign::Middle,
                            _ => VAlign::Bottom,
                        },
                    )
                } else {
                    (
                        match h_just {
                            1 | 4 => HAlign::Center,
                            2 => HAlign::Right,
                            _ => HAlign::Left,
                        },
                        match v_just {
                            1 => VAlign::Bottom,
                            2 => VAlign::Middle,
                            3 => VAlign::Top,
                            _ => VAlign::Baseline,
                        },
                    )
                };
                let height = e.f(40).unwrap_or(1.0) * k;
                let rot = e.f(50).unwrap_or(0.0).to_radians();
                if content.trim().is_empty() {
                    i += 1;
                    continue;
                }
                push(
                    Shape::Text(Text {
                        position: pos,
                        content,
                        height,
                        rotation: if mirrored { core::f64::consts::PI - rot } else { rot },
                        halign,
                        valign,
                    }),
                    &mut rec,
                );
            }
            "VERTEX" | "SEQEND" => {}
            other => rec.add(LossKind::Unsupported, other.to_owned()),
        }
        i += 1;
    }
    Ok(DxfImport { layers, entities: out, version, unit, report: rec.finish() })
}

// ---------------------------------------------------------------------------
// Export

fn g(out: &mut String, code: i32, value: impl core::fmt::Display) {
    let _ = write!(out, "{code:>3}\r\n{value}\r\n");
}

fn f(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") }
}

fn dxf_text(s: &str) -> String {
    // R12 files are ASCII/ANSI: non-ASCII characters are written as \U+XXXX.
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() && !c.is_ascii_control() {
            o.push(c);
        } else if c == '\n' {
            o.push(' ');
        } else if (c as u32) <= 0xFFFF {
            let _ = write!(o, "\\U+{:04X}", c as u32);
        }
    }
    o
}

fn layer_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_uppercase() } else { '_' })
        .take(255)
        .collect();
    if n.is_empty() { "0".into() } else { n }
}

fn poly(out: &mut String, layer: &str, pts: &[Point], bulges: &[f64], closed: bool) {
    g(out, 0, "POLYLINE");
    g(out, 8, layer);
    g(out, 66, 1);
    g(out, 10, "0.0");
    g(out, 20, "0.0");
    g(out, 30, "0.0");
    g(out, 70, i32::from(closed));
    for (i, p) in pts.iter().enumerate() {
        g(out, 0, "VERTEX");
        g(out, 8, layer);
        g(out, 10, f(p.x));
        g(out, 20, f(p.y));
        g(out, 30, "0.0");
        if let Some(b) = bulges.get(i)
            && *b != 0.0
        {
            g(out, 42, f(*b));
        }
    }
    g(out, 0, "SEQEND");
    g(out, 8, layer);
}

fn shape_dxf(out: &mut String, layer: &str, s: &Shape, rec: &mut Recorder) {
    match s {
        Shape::Point(p) => {
            g(out, 0, "POINT");
            g(out, 8, layer);
            g(out, 10, f(p.at.x));
            g(out, 20, f(p.at.y));
            g(out, 30, "0.0");
        }
        Shape::Line(l) => {
            g(out, 0, "LINE");
            g(out, 8, layer);
            g(out, 10, f(l.a.x));
            g(out, 20, f(l.a.y));
            g(out, 30, "0.0");
            g(out, 11, f(l.b.x));
            g(out, 21, f(l.b.y));
            g(out, 31, "0.0");
        }
        Shape::Circle(c) => {
            g(out, 0, "CIRCLE");
            g(out, 8, layer);
            g(out, 10, f(c.center.x));
            g(out, 20, f(c.center.y));
            g(out, 30, "0.0");
            g(out, 40, f(c.radius));
        }
        Shape::Arc(a) => {
            // DXF arcs are counter-clockwise from 50 to 51 (degrees).
            let a = if a.sweep < 0.0 { a.reversed() } else { *a };
            g(out, 0, "ARC");
            g(out, 8, layer);
            g(out, 10, f(a.center.x));
            g(out, 20, f(a.center.y));
            g(out, 30, "0.0");
            g(out, 40, f(a.radius));
            g(out, 50, f(normalize_angle(a.start).to_degrees()));
            g(out, 51, f(normalize_angle(a.end_angle()).to_degrees()));
        }
        Shape::Polyline(p) => poly(out, layer, &p.points, &p.bulges, p.closed),
        Shape::Rect(r) => poly(out, layer, &r.corners(), &[], true),
        Shape::Polygon(pg) => {
            poly(out, layer, &pg.outer, &[], true);
            for h in &pg.holes {
                rec.add(LossKind::Approximated, "polygon hole written as separate polyline");
                poly(out, layer, h, &[], true);
            }
        }
        Shape::Path(_) => {
            rec.add(LossKind::Approximated, "Bézier path flattened");
            let tol = FlattenTolerance((s.bbox().size().length() * 1e-4).max(1e-6));
            for fp in s.flatten(tol) {
                poly(out, layer, &fp.points, &[], fp.closed);
            }
        }
        Shape::Text(t) => {
            g(out, 0, "TEXT");
            g(out, 8, layer);
            g(out, 10, f(t.position.x));
            g(out, 20, f(t.position.y));
            g(out, 30, "0.0");
            g(out, 40, f(t.height));
            g(out, 1, dxf_text(&t.content));
            if t.rotation != 0.0 {
                g(out, 50, f(t.rotation.to_degrees()));
            }
            let h = match t.halign {
                HAlign::Left => 0,
                HAlign::Center => 1,
                HAlign::Right => 2,
            };
            let v = match t.valign {
                VAlign::Baseline => 0,
                VAlign::Bottom => 1,
                VAlign::Middle => 2,
                VAlign::Top => 3,
            };
            if h != 0 || v != 0 {
                g(out, 72, h);
                g(out, 73, v);
                g(out, 11, f(t.position.x));
                g(out, 21, f(t.position.y));
                g(out, 31, "0.0");
            }
            if t.content.contains('\n') {
                rec.add(LossKind::Text, "multi-line text joined");
            }
        }
    }
}

/// Export the engine's committed document as R12 ASCII DXF (mm).
pub fn export_dxf(engine: &mut Engine) -> (String, ConversionReport) {
    let mut rec = Recorder::default();
    let scene = engine.full_scene();
    let doc = engine.document();
    for _ in 0..doc.constraint_count() {
        rec.add(LossKind::Constraints, "constraint");
    }
    let layers: Vec<(LayerId, String)> = doc.layers().iter().map(|l| (l.id, layer_name(&l.name))).collect();
    let mut out = String::new();
    g(&mut out, 999, "Dotloom DXF export (R12, millimetres)");
    g(&mut out, 0, "SECTION");
    g(&mut out, 2, "HEADER");
    g(&mut out, 9, "$ACADVER");
    g(&mut out, 1, "AC1009");
    g(&mut out, 9, "$INSUNITS");
    g(&mut out, 70, 4);
    g(&mut out, 0, "ENDSEC");
    g(&mut out, 0, "SECTION");
    g(&mut out, 2, "TABLES");
    g(&mut out, 0, "TABLE");
    g(&mut out, 2, "LAYER");
    g(&mut out, 70, layers.len());
    for (_, name) in &layers {
        g(&mut out, 0, "LAYER");
        g(&mut out, 2, name);
        g(&mut out, 70, 0);
        g(&mut out, 62, 7);
        g(&mut out, 6, "CONTINUOUS");
    }
    g(&mut out, 0, "ENDTAB");
    g(&mut out, 0, "ENDSEC");
    g(&mut out, 0, "SECTION");
    g(&mut out, 2, "ENTITIES");
    let order: Vec<u64> = scene.order.clone().unwrap_or_default();
    for id in order {
        let Some(item) = scene.upserts.iter().find(|i| i.id == id) else { continue };
        let Some(e) = doc.entity(dotloom_document::EntityId(id)) else { continue };
        let lname = layers.get(item.layer as usize).map_or("0", |(_, n)| n.as_str()).to_owned();
        rec.entities += 1;
        if !is_builtin(&e.type_id) {
            rec.add(LossKind::PluginGeometry, e.type_id.to_string());
        } else if e.type_id.as_str() == dotloom_document::builtin::types::DIMENSION {
            rec.add(LossKind::Dimensions, "dimension");
        }
        for prim in &item.prims {
            match prim {
                Primitive::Shape { shape, fill, .. } => {
                    if fill.is_some() {
                        rec.add(LossKind::Style, "fill");
                    }
                    shape_dxf(&mut out, &lname, shape, &mut rec);
                }
                Primitive::Text { text, .. } => shape_dxf(&mut out, &lname, &Shape::Text(text.clone()), &mut rec),
                Primitive::Arrow { tip, direction, size, .. } => {
                    let back = *tip - *direction * *size;
                    let n = direction.perp() * (*size * 0.3);
                    poly(&mut out, &lname, &[*tip, back + n, back - n], &[], true);
                }
            }
        }
    }
    g(&mut out, 0, "ENDSEC");
    g(&mut out, 0, "EOF");
    rec.notes.push("written as AC1009 (R12) ASCII, millimetres".into());
    (out, rec.finish())
}
