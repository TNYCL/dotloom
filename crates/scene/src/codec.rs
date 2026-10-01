//! Compact binary encoding of [`SceneDelta`] (little endian).
//!
//! ```text
//! "DLSC" u16 version u16 flags(preview=1, reset=2, order=4) u64 revision
//! u32 upserts u32 removals [u32 order]
//! removals: u64…   order: u64…   upserts: item…
//! item: u64 id u32 layer u8 flags f64×4 bbox u32 prims prim…
//! ```
//!
//! The decoder is bounded: every count is checked against the remaining input
//! before allocating, and malformed input returns [`DecodeError`] instead of
//! panicking.

use dotloom_geometry::{
    Aabb, Arc, Circle, HAlign, Path, PathEl, Point, PointShape, Polygon, Polyline, Rect, Segment, Shape, Text, VAlign,
    Vector,
};
use thiserror::Error;

use crate::{Primitive, SceneDelta, SceneItem, Stroke};

/// Magic bytes.
pub const MAGIC: &[u8; 4] = b"DLSC";
/// Encoding version.
pub const SCENE_FORMAT_VERSION: u16 = 1;

/// Decoding failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// Input ended early.
    #[error("unexpected end of scene data")]
    Truncated,
    /// Wrong magic bytes.
    #[error("not a Dotloom scene delta")]
    BadMagic,
    /// Unsupported version.
    #[error("unsupported scene format version {0}")]
    Version(u16),
    /// Unknown tag.
    #[error("invalid tag {0}")]
    Tag(u8),
    /// A count is larger than the remaining input allows.
    #[error("count {0} exceeds remaining input")]
    Count(u32),
    /// Invalid UTF-8 text.
    #[error("invalid utf-8 text")]
    Utf8,
    /// Trailing bytes after the delta.
    #[error("trailing bytes")]
    Trailing,
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).unwrap_or(u32::MAX));
    }
    fn pt(&mut self, p: Point) {
        self.f64(p.x);
        self.f64(p.y);
    }
    fn str(&mut self, s: &str) {
        self.len(s.len());
        self.0.extend_from_slice(s.as_bytes());
    }
    fn text(&mut self, t: &Text) {
        self.pt(t.position);
        self.f64(t.height);
        self.f64(t.rotation);
        self.u8(match t.halign {
            HAlign::Left => 0,
            HAlign::Center => 1,
            HAlign::Right => 2,
        });
        self.u8(match t.valign {
            VAlign::Baseline => 0,
            VAlign::Middle => 1,
            VAlign::Top => 2,
            VAlign::Bottom => 3,
        });
        self.str(&t.content);
    }
    fn points(&mut self, pts: &[Point]) {
        self.len(pts.len());
        for p in pts {
            self.pt(*p);
        }
    }
    fn shape(&mut self, s: &Shape) {
        match s {
            Shape::Point(p) => {
                self.u8(0);
                self.pt(p.at);
            }
            Shape::Line(l) => {
                self.u8(1);
                self.pt(l.a);
                self.pt(l.b);
            }
            Shape::Polyline(p) => {
                self.u8(2);
                self.u8(u8::from(p.closed));
                self.points(&p.points);
                self.len(p.bulges.len());
                for b in &p.bulges {
                    self.f64(*b);
                }
            }
            Shape::Rect(r) => {
                self.u8(3);
                self.pt(r.origin);
                self.f64(r.width);
                self.f64(r.height);
            }
            Shape::Circle(c) => {
                self.u8(4);
                self.pt(c.center);
                self.f64(c.radius);
            }
            Shape::Arc(a) => {
                self.u8(5);
                self.pt(a.center);
                self.f64(a.radius);
                self.f64(a.start);
                self.f64(a.sweep);
            }
            Shape::Path(p) => {
                self.u8(6);
                self.len(p.elements.len());
                for el in &p.elements {
                    match *el {
                        PathEl::MoveTo(a) => {
                            self.u8(0);
                            self.pt(a);
                        }
                        PathEl::LineTo(a) => {
                            self.u8(1);
                            self.pt(a);
                        }
                        PathEl::QuadTo(a, b) => {
                            self.u8(2);
                            self.pt(a);
                            self.pt(b);
                        }
                        PathEl::CubicTo(a, b, c) => {
                            self.u8(3);
                            self.pt(a);
                            self.pt(b);
                            self.pt(c);
                        }
                        PathEl::Close => self.u8(4),
                    }
                }
            }
            Shape::Polygon(p) => {
                self.u8(7);
                self.len(1 + p.holes.len());
                self.points(&p.outer);
                for h in &p.holes {
                    self.points(h);
                }
            }
            Shape::Text(t) => {
                self.u8(8);
                self.text(t);
            }
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.i.checked_add(n).ok_or(DecodeError::Truncated)?;
        let s = self.b.get(self.i..end).ok_or(DecodeError::Truncated)?;
        self.i = end;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.arr::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_le_bytes(self.arr()?))
    }
    fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_le_bytes(self.arr()?))
    }
    fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.i)
    }
    /// Count whose elements need at least `min_size` bytes each.
    fn count(&mut self, min_size: usize) -> Result<usize, DecodeError> {
        let n = self.u32()?;
        let need = (n as usize).checked_mul(min_size.max(1)).ok_or(DecodeError::Count(n))?;
        if need > self.remaining() {
            return Err(DecodeError::Count(n));
        }
        Ok(n as usize)
    }
    fn pt(&mut self) -> Result<Point, DecodeError> {
        Ok(Point::new(self.f64()?, self.f64()?))
    }
    fn str(&mut self) -> Result<String, DecodeError> {
        let n = self.count(1)?;
        let s = self.take(n)?;
        String::from_utf8(s.to_vec()).map_err(|_| DecodeError::Utf8)
    }
    fn text(&mut self) -> Result<Text, DecodeError> {
        let position = self.pt()?;
        let height = self.f64()?;
        let rotation = self.f64()?;
        let halign = match self.u8()? {
            0 => HAlign::Left,
            1 => HAlign::Center,
            2 => HAlign::Right,
            t => return Err(DecodeError::Tag(t)),
        };
        let valign = match self.u8()? {
            0 => VAlign::Baseline,
            1 => VAlign::Middle,
            2 => VAlign::Top,
            3 => VAlign::Bottom,
            t => return Err(DecodeError::Tag(t)),
        };
        let content = self.str()?;
        Ok(Text { position, content, height, rotation, halign, valign })
    }
    fn points(&mut self) -> Result<Vec<Point>, DecodeError> {
        let n = self.count(16)?;
        (0..n).map(|_| self.pt()).collect()
    }
    fn shape(&mut self) -> Result<Shape, DecodeError> {
        Ok(match self.u8()? {
            0 => Shape::Point(PointShape { at: self.pt()? }),
            1 => Shape::Line(Segment::new(self.pt()?, self.pt()?)),
            2 => {
                let closed = self.u8()? != 0;
                let points = self.points()?;
                let nb = self.count(8)?;
                let bulges = (0..nb).map(|_| self.f64()).collect::<Result<_, _>>()?;
                Shape::Polyline(Polyline { points, bulges, closed })
            }
            3 => Shape::Rect(Rect { origin: self.pt()?, width: self.f64()?, height: self.f64()? }),
            4 => Shape::Circle(Circle { center: self.pt()?, radius: self.f64()? }),
            5 => Shape::Arc(Arc { center: self.pt()?, radius: self.f64()?, start: self.f64()?, sweep: self.f64()? }),
            6 => {
                let n = self.count(1)?;
                let mut elements = Vec::with_capacity(n);
                for _ in 0..n {
                    elements.push(match self.u8()? {
                        0 => PathEl::MoveTo(self.pt()?),
                        1 => PathEl::LineTo(self.pt()?),
                        2 => PathEl::QuadTo(self.pt()?, self.pt()?),
                        3 => PathEl::CubicTo(self.pt()?, self.pt()?, self.pt()?),
                        4 => PathEl::Close,
                        t => return Err(DecodeError::Tag(t)),
                    });
                }
                Shape::Path(Path { elements })
            }
            7 => {
                let rings = self.count(4)?;
                if rings == 0 {
                    return Err(DecodeError::Count(0));
                }
                let outer = self.points()?;
                let holes = (1..rings).map(|_| self.points()).collect::<Result<_, _>>()?;
                Shape::Polygon(Polygon { outer, holes })
            }
            8 => Shape::Text(self.text()?),
            t => return Err(DecodeError::Tag(t)),
        })
    }
}

impl SceneDelta {
    /// Encode to the binary format.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W(Vec::with_capacity(64 + self.upserts.len() * 96));
        w.0.extend_from_slice(MAGIC);
        w.u16(SCENE_FORMAT_VERSION);
        let flags = u16::from(self.preview) | (u16::from(self.reset) << 1) | (u16::from(self.order.is_some()) << 2);
        w.u16(flags);
        w.u64(self.revision);
        w.len(self.upserts.len());
        w.len(self.removals.len());
        if let Some(o) = &self.order {
            w.len(o.len());
        }
        for r in &self.removals {
            w.u64(*r);
        }
        if let Some(o) = &self.order {
            for id in o {
                w.u64(*id);
            }
        }
        for item in &self.upserts {
            w.u64(item.id);
            w.u32(item.layer);
            w.u8(item.flags);
            w.pt(item.bbox.min);
            w.pt(item.bbox.max);
            w.len(item.prims.len());
            for p in &item.prims {
                match p {
                    Primitive::Shape { shape, stroke, fill } => {
                        w.u8(0);
                        w.u8(u8::from(stroke.is_some()) | (u8::from(fill.is_some()) << 1));
                        if let Some(s) = stroke {
                            w.u32(s.color);
                            w.f32(s.width);
                            w.len(s.dash.len());
                            for d in &s.dash {
                                w.f32(*d);
                            }
                        }
                        if let Some(f) = fill {
                            w.u32(*f);
                        }
                        w.shape(shape);
                    }
                    Primitive::Text { text, color } => {
                        w.u8(1);
                        w.text(text);
                        w.u32(*color);
                    }
                    Primitive::Arrow { tip, direction, size, color } => {
                        w.u8(2);
                        w.pt(*tip);
                        w.f64(direction.x);
                        w.f64(direction.y);
                        w.f64(*size);
                        w.u32(*color);
                    }
                }
            }
        }
        w.0
    }

    /// Decode from the binary format.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = R { b: bytes, i: 0 };
        if r.take(4)? != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let version = r.u16()?;
        if version != SCENE_FORMAT_VERSION {
            return Err(DecodeError::Version(version));
        }
        let flags = r.u16()?;
        let revision = r.u64()?;
        let n_up = r.count(4)?;
        let n_rm = r.count(8)?;
        let n_order = if flags & 4 != 0 { Some(r.count(8)?) } else { None };
        let removals = (0..n_rm).map(|_| r.u64()).collect::<Result<_, _>>()?;
        let order = match n_order {
            Some(n) => Some((0..n).map(|_| r.u64()).collect::<Result<_, _>>()?),
            None => None,
        };
        let mut upserts = Vec::with_capacity(n_up);
        for _ in 0..n_up {
            let id = r.u64()?;
            let layer = r.u32()?;
            let item_flags = r.u8()?;
            let bbox = Aabb { min: r.pt()?, max: r.pt()? };
            let np = r.count(1)?;
            let mut prims = Vec::with_capacity(np);
            for _ in 0..np {
                prims.push(match r.u8()? {
                    0 => {
                        let f = r.u8()?;
                        let stroke = if f & 1 != 0 {
                            let color = r.u32()?;
                            let width = r.f32()?;
                            let nd = r.count(4)?;
                            let dash = (0..nd).map(|_| r.f32()).collect::<Result<_, _>>()?;
                            Some(Stroke { color, width, dash })
                        } else {
                            None
                        };
                        let fill = if f & 2 != 0 { Some(r.u32()?) } else { None };
                        Primitive::Shape { shape: r.shape()?, stroke, fill }
                    }
                    1 => Primitive::Text { text: r.text()?, color: r.u32()? },
                    2 => Primitive::Arrow {
                        tip: r.pt()?,
                        direction: Vector::new(r.f64()?, r.f64()?),
                        size: r.f64()?,
                        color: r.u32()?,
                    },
                    t => return Err(DecodeError::Tag(t)),
                });
            }
            upserts.push(SceneItem { id, layer, bbox, flags: item_flags, prims });
        }
        if r.remaining() != 0 {
            return Err(DecodeError::Trailing);
        }
        Ok(Self { revision, preview: flags & 1 != 0, reset: flags & 2 != 0, upserts, removals, order })
    }
}
