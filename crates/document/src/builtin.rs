//! Built-in entity types and their geometry parameters.

use dotloom_geometry::{PathEl, Point, Shape, ShapeKind};

use crate::{DocError, TypeId};

/// Built-in geometry type IDs.
pub mod types {
    /// Point marker.
    pub const POINT: &str = "dotloom.point";
    /// Line segment.
    pub const LINE: &str = "dotloom.line";
    /// Polyline.
    pub const POLYLINE: &str = "dotloom.polyline";
    /// Rectangle.
    pub const RECT: &str = "dotloom.rect";
    /// Circle.
    pub const CIRCLE: &str = "dotloom.circle";
    /// Arc.
    pub const ARC: &str = "dotloom.arc";
    /// Bézier path.
    pub const PATH: &str = "dotloom.path";
    /// Polygon.
    pub const POLYGON: &str = "dotloom.polygon";
    /// Text.
    pub const TEXT: &str = "dotloom.text";
    /// Associative dimension (props-based, see `crate::dimension`).
    pub const DIMENSION: &str = "dotloom.dimension";
}

/// Shape kind required by a built-in geometry type.
#[must_use]
pub fn builtin_shape_kind(t: &TypeId) -> Option<ShapeKind> {
    Some(match t.as_str() {
        types::POINT => ShapeKind::Point,
        types::LINE => ShapeKind::Line,
        types::POLYLINE => ShapeKind::Polyline,
        types::RECT => ShapeKind::Rect,
        types::CIRCLE => ShapeKind::Circle,
        types::ARC => ShapeKind::Arc,
        types::PATH => ShapeKind::Path,
        types::POLYGON => ShapeKind::Polygon,
        types::TEXT => ShapeKind::Text,
        _ => return None,
    })
}

/// Built-in type ID for a shape kind.
#[must_use]
pub fn builtin_type_for(kind: ShapeKind) -> TypeId {
    let s = match kind {
        ShapeKind::Point => types::POINT,
        ShapeKind::Line => types::LINE,
        ShapeKind::Polyline => types::POLYLINE,
        ShapeKind::Rect => types::RECT,
        ShapeKind::Circle => types::CIRCLE,
        ShapeKind::Arc => types::ARC,
        ShapeKind::Path => types::PATH,
        ShapeKind::Polygon => types::POLYGON,
        ShapeKind::Text => types::TEXT,
    };
    TypeId(s.to_owned())
}

/// Whether the type ID belongs to the `dotloom` namespace.
#[must_use]
pub fn is_builtin(t: &TypeId) -> bool {
    t.namespace() == "dotloom"
}

/// Physical meaning of a geometry parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamDim {
    /// Length (mm).
    Length,
    /// Angle (rad).
    Angle,
}

fn pt_params(prefix: &str, p: Point, out: &mut Vec<(String, f64, ParamDim)>) {
    out.push((format!("{prefix}.x"), p.x, ParamDim::Length));
    out.push((format!("{prefix}.y"), p.y, ParamDim::Length));
}

/// Named numeric parameters of a shape (solver variables), in stable order.
#[must_use]
pub fn geometry_params(s: &Shape) -> Vec<(String, f64, ParamDim)> {
    let mut out = Vec::new();
    match s {
        Shape::Point(p) => {
            out.push(("x".into(), p.at.x, ParamDim::Length));
            out.push(("y".into(), p.at.y, ParamDim::Length));
        }
        Shape::Line(l) => {
            pt_params("a", l.a, &mut out);
            pt_params("b", l.b, &mut out);
        }
        Shape::Polyline(p) => {
            for (i, v) in p.points.iter().enumerate() {
                pt_params(&format!("v{i}"), *v, &mut out);
            }
        }
        Shape::Polygon(p) => {
            for (i, v) in p.outer.iter().enumerate() {
                pt_params(&format!("v{i}"), *v, &mut out);
            }
        }
        Shape::Rect(r) => {
            out.push(("x".into(), r.origin.x, ParamDim::Length));
            out.push(("y".into(), r.origin.y, ParamDim::Length));
            out.push(("width".into(), r.width, ParamDim::Length));
            out.push(("height".into(), r.height, ParamDim::Length));
        }
        Shape::Circle(c) => {
            out.push(("cx".into(), c.center.x, ParamDim::Length));
            out.push(("cy".into(), c.center.y, ParamDim::Length));
            out.push(("r".into(), c.radius, ParamDim::Length));
        }
        Shape::Arc(a) => {
            out.push(("cx".into(), a.center.x, ParamDim::Length));
            out.push(("cy".into(), a.center.y, ParamDim::Length));
            out.push(("r".into(), a.radius, ParamDim::Length));
            out.push(("start".into(), a.start, ParamDim::Angle));
            out.push(("sweep".into(), a.sweep, ParamDim::Angle));
        }
        Shape::Path(p) => {
            let mut i = 0;
            for el in &p.elements {
                let pts: Vec<Point> = match *el {
                    PathEl::MoveTo(a) | PathEl::LineTo(a) => vec![a],
                    PathEl::QuadTo(a, b) => vec![a, b],
                    PathEl::CubicTo(a, b, c) => vec![a, b, c],
                    PathEl::Close => vec![],
                };
                for q in pts {
                    pt_params(&format!("p{i}"), q, &mut out);
                    i += 1;
                }
            }
        }
        Shape::Text(t) => {
            out.push(("x".into(), t.position.x, ParamDim::Length));
            out.push(("y".into(), t.position.y, ParamDim::Length));
            out.push(("height".into(), t.height, ParamDim::Length));
            out.push(("rotation".into(), t.rotation, ParamDim::Angle));
        }
    }
    out
}

/// Read one geometry parameter.
#[must_use]
pub fn get_geometry_param(s: &Shape, name: &str) -> Option<f64> {
    geometry_params(s).into_iter().find(|(n, _, _)| n == name).map(|(_, v, _)| v)
}

fn set_point(p: &mut Point, axis: &str, v: f64) -> bool {
    match axis {
        "x" => p.x = v,
        "y" => p.y = v,
        _ => return false,
    }
    true
}

fn indexed(name: &str, prefix: char) -> Option<(usize, &str)> {
    let rest = name.strip_prefix(prefix)?;
    let (idx, axis) = rest.split_once('.')?;
    Some((idx.parse().ok()?, axis))
}

/// Write one geometry parameter. Does not validate the resulting shape.
pub fn set_geometry_param(s: &mut Shape, name: &str, v: f64) -> Result<(), DocError> {
    if !v.is_finite() {
        return Err(DocError::InvalidValue(format!("parameter `{name}` must be finite")));
    }
    let ok = match s {
        Shape::Point(p) => set_point(&mut p.at, name, v),
        Shape::Line(l) => match name.split_once('.') {
            Some(("a", axis)) => set_point(&mut l.a, axis, v),
            Some(("b", axis)) => set_point(&mut l.b, axis, v),
            _ => false,
        },
        Shape::Polyline(p) => {
            indexed(name, 'v').is_some_and(|(i, axis)| p.points.get_mut(i).is_some_and(|q| set_point(q, axis, v)))
        }
        Shape::Polygon(p) => {
            indexed(name, 'v').is_some_and(|(i, axis)| p.outer.get_mut(i).is_some_and(|q| set_point(q, axis, v)))
        }
        Shape::Rect(r) => match name {
            "x" => set_point(&mut r.origin, "x", v),
            "y" => set_point(&mut r.origin, "y", v),
            "width" => {
                r.width = v;
                true
            }
            "height" => {
                r.height = v;
                true
            }
            _ => false,
        },
        Shape::Circle(c) => match name {
            "cx" => set_point(&mut c.center, "x", v),
            "cy" => set_point(&mut c.center, "y", v),
            "r" => {
                c.radius = v;
                true
            }
            _ => false,
        },
        Shape::Arc(a) => match name {
            "cx" => set_point(&mut a.center, "x", v),
            "cy" => set_point(&mut a.center, "y", v),
            "r" => {
                a.radius = v;
                true
            }
            "start" => {
                a.start = v;
                true
            }
            "sweep" => {
                a.sweep = v;
                true
            }
            _ => false,
        },
        Shape::Path(p) => match indexed(name, 'p') {
            Some((target, axis)) => {
                let mut i = 0;
                let mut done = false;
                for el in &mut p.elements {
                    let pts: Vec<&mut Point> = match el {
                        PathEl::MoveTo(a) | PathEl::LineTo(a) => vec![a],
                        PathEl::QuadTo(a, b) => vec![a, b],
                        PathEl::CubicTo(a, b, c) => vec![a, b, c],
                        PathEl::Close => vec![],
                    };
                    for q in pts {
                        if i == target {
                            done = set_point(q, axis, v);
                        }
                        i += 1;
                    }
                }
                done
            }
            None => false,
        },
        Shape::Text(t) => match name {
            "x" => set_point(&mut t.position, "x", v),
            "y" => set_point(&mut t.position, "y", v),
            "height" => {
                t.height = v;
                true
            }
            "rotation" => {
                t.rotation = v;
                true
            }
            _ => false,
        },
    };
    if ok { Ok(()) } else { Err(DocError::InvalidValue(format!("unknown geometry parameter `{name}`"))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotloom_geometry::{Arc, Polyline, Segment};

    #[test]
    fn params_roundtrip_for_every_shape() {
        let shapes = vec![
            Shape::Line(Segment::new(Point::new(1.0, 2.0), Point::new(3.0, 4.0))),
            Shape::Polyline(Polyline::open(vec![Point::ORIGIN, Point::new(1.0, 1.0), Point::new(2.0, 0.0)])),
            Shape::Arc(Arc::new(Point::ORIGIN, 5.0, 0.1, 1.0).unwrap()),
            Shape::Path(dotloom_geometry::Path {
                elements: vec![
                    PathEl::MoveTo(Point::ORIGIN),
                    PathEl::CubicTo(Point::new(1.0, 1.0), Point::new(2.0, 1.0), Point::new(3.0, 0.0)),
                ],
            }),
        ];
        for mut s in shapes {
            for (name, value, _) in geometry_params(&s) {
                set_geometry_param(&mut s, &name, value + 0.5).unwrap();
                assert_eq!(get_geometry_param(&s, &name), Some(value + 0.5), "{name}");
            }
            assert!(set_geometry_param(&mut s, "nope", 1.0).is_err());
            assert!(set_geometry_param(&mut s, "v0.x", f64::NAN).is_err());
        }
    }
}
