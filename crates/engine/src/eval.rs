//! Numeric evaluation of entities: world anchors, drawable shapes and texts.
//!
//! Built-in entities use their canonical geometry; dimensions are computed from
//! the anchors they reference; plugin entities are evaluated from their compiled
//! definition; entities whose plugin is missing, disabled or too old fall back to
//! their stored standard representation and are read-only.

use std::collections::BTreeMap;

use dotloom_document::{
    Entity, EntityId, PropValue,
    builtin::{get_geometry_param, is_builtin, types},
};
use dotloom_geometry::{
    Aabb, Anchor, AnchorKind, Arc, Circle, HAlign, Point, Polygon, Polyline, Segment, Shape, Text, TransformPolicy,
    VAlign, Vector,
    dimension::{self, DimensionStyle, LinearKind},
    units::LengthUnit,
};
use serde::{Deserialize, Serialize};

use crate::{
    DocView,
    lang::{Axis, Compiled, Leaf},
    registry::{CompiledPrim, CompiledType, PrimStyle, Registry},
};

/// Maximum reference depth while evaluating anchors of referenced entities.
pub const MAX_REF_DEPTH: usize = 16;

/// Why an entity cannot be edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum ReadOnly {
    /// Plugin type not registered.
    MissingPlugin {
        /// Type.
        type_id: String,
    },
    /// Plugin type registered but disabled.
    DisabledPlugin {
        /// Type.
        type_id: String,
    },
    /// Entity was written by a newer version of the type.
    NewerVersion {
        /// Type.
        type_id: String,
        /// Entity version.
        found: u32,
        /// Registered version.
        supported: u32,
    },
}

/// A drawable piece.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawable {
    /// Shape with recipe style.
    Shape(Shape, PrimStyle),
    /// Text with recipe style.
    Text(Text, PrimStyle),
    /// Arrow head (dimensions).
    Arrow {
        /// Tip.
        tip: Point,
        /// Direction.
        direction: Vector,
        /// Size (model units).
        size: f64,
    },
}

/// Result of evaluating one entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluated {
    /// World-space anchors.
    pub anchors: Vec<Anchor>,
    /// World-space drawables.
    pub drawables: Vec<Drawable>,
    /// Bounding box of drawables.
    pub bbox: Aabb,
    /// Set when the entity cannot be edited.
    pub read_only: Option<ReadOnly>,
    /// Evaluation problem (bad expression input, broken reference, ...).
    pub error: Option<String>,
    /// Measured value for dimensions (mm or rad).
    pub measured: Option<f64>,
}

impl Evaluated {
    fn empty() -> Self {
        Self {
            anchors: Vec::new(),
            drawables: Vec::new(),
            bbox: Aabb::EMPTY,
            read_only: None,
            error: None,
            measured: None,
        }
    }

    /// Anchor by name.
    #[must_use]
    pub fn anchor(&self, name: &str) -> Option<Point> {
        self.anchors.iter().find(|a| a.name == name).map(|a| a.point)
    }

    /// World-space shapes usable for hit testing (texts as their estimated box).
    pub fn shapes(&self) -> impl Iterator<Item = Shape> + '_ {
        self.drawables.iter().filter_map(|d| match d {
            Drawable::Shape(s, _) => Some(s.clone()),
            Drawable::Text(t, _) => Some(Shape::Text(t.clone())),
            Drawable::Arrow { .. } => None,
        })
    }
}

fn bbox_of(ds: &[Drawable]) -> Aabb {
    ds.iter().fold(Aabb::EMPTY, |b, d| match d {
        Drawable::Shape(s, _) => b.union(s.bbox()),
        Drawable::Text(t, _) => b.union(Shape::Text(t.clone()).bbox()),
        Drawable::Arrow { tip, direction, size } => b.include(*tip).include(*tip - *direction * *size),
    })
}

/// Evaluation context.
#[derive(Debug, Clone, Copy)]
pub struct Ctx<'a> {
    /// Document view.
    pub view: &'a dyn DocView,
    /// Registry.
    pub registry: &'a Registry,
}

impl core::fmt::Debug for dyn DocView + '_ {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("DocView")
    }
}

/// Compatibility of an entity with the registered plugin type.
pub fn plugin_for<'r>(registry: &'r Registry, e: &Entity) -> Result<&'r CompiledType, ReadOnly> {
    let t = e.type_id.to_string();
    let Some(entry) = registry.get(&e.type_id) else {
        return Err(ReadOnly::MissingPlugin { type_id: t });
    };
    if !entry.enabled {
        return Err(ReadOnly::DisabledPlugin { type_id: t });
    }
    if e.type_version > entry.compiled.def.version {
        return Err(ReadOnly::NewerVersion {
            type_id: t,
            found: e.type_version,
            supported: entry.compiled.def.version,
        });
    }
    Ok(&entry.compiled)
}

/// Evaluate an entity.
#[must_use]
pub fn evaluate(ctx: Ctx<'_>, id: EntityId) -> Evaluated {
    evaluate_depth(ctx, id, 0)
}

fn evaluate_depth(ctx: Ctx<'_>, id: EntityId, depth: usize) -> Evaluated {
    let Some(e) = ctx.view.entity(id) else {
        let mut ev = Evaluated::empty();
        ev.error = Some(format!("{id} does not exist"));
        return ev;
    };
    if depth > MAX_REF_DEPTH {
        let mut ev = Evaluated::empty();
        ev.error = Some("reference chain too deep or cyclic".into());
        return ev;
    }
    if e.type_id.as_str() == types::DIMENSION {
        return evaluate_dimension(ctx, e, depth);
    }
    if is_builtin(&e.type_id) {
        return evaluate_builtin(e);
    }
    match plugin_for(ctx.registry, e) {
        Ok(def) => evaluate_plugin(ctx, e, def, depth),
        Err(ro) => {
            let mut ev = Evaluated::empty();
            ev.drawables =
                e.fallback.iter().flatten().map(|s| Drawable::Shape(s.clone(), PrimStyle::default())).collect();
            ev.bbox = bbox_of(&ev.drawables);
            ev.read_only = Some(ro);
            ev
        }
    }
}

fn evaluate_builtin(e: &Entity) -> Evaluated {
    let mut ev = Evaluated::empty();
    let Some(g) = &e.geometry else {
        ev.error = Some("missing geometry".into());
        return ev;
    };
    let t = e.transform;
    match g.transform(t, TransformPolicy::Convert) {
        Ok(Shape::Text(tx)) => ev.drawables.push(Drawable::Text(tx, PrimStyle::default())),
        Ok(s) => ev.drawables.push(Drawable::Shape(s, PrimStyle::default())),
        Err(err) => ev.error = Some(err.to_string()),
    }
    ev.anchors = g.anchors().into_iter().map(|a| Anchor { point: t.apply(a.point), ..a }).collect();
    ev.bbox = bbox_of(&ev.drawables);
    ev
}

/// World anchor of another entity.
fn world_anchor(ctx: Ctx<'_>, id: EntityId, name: &str, depth: usize) -> Option<Point> {
    evaluate_depth(ctx, id, depth + 1).anchor(name)
}

fn num_prop(e: &Entity, def: Option<&CompiledType>, name: &str) -> Option<f64> {
    match e.props.get(name) {
        Some(PropValue::Number(v)) => Some(*v),
        Some(_) => None,
        None => def.and_then(|d| match d.def.props.get(name) {
            Some(crate::registry::PropDef::Number { dim, default: Some(l), .. }) => l.value(dim.dim()).ok(),
            _ => None,
        }),
    }
}

fn point_prop(e: &Entity, def: Option<&CompiledType>, name: &str) -> Option<Point> {
    match e.props.get(name) {
        Some(PropValue::Point(p)) => Some(*p),
        Some(_) => None,
        None => def.and_then(|d| match d.def.props.get(name) {
            Some(crate::registry::PropDef::Point { default: Some([x, y]), .. }) => Some(Point::new(*x, *y)),
            _ => None,
        }),
    }
}

/// Numeric parameter of any entity (`width`, `start.x`, `r`, ...), in its local units.
#[must_use]
pub fn param_value(ctx: Ctx<'_>, e: &Entity, name: &str) -> Option<f64> {
    if is_builtin(&e.type_id) {
        return e.geometry.as_ref().and_then(|g| get_geometry_param(g, name));
    }
    let def = ctx.registry.get(&e.type_id).map(|t| t.compiled.as_ref());
    if let Some((base, axis)) = name.rsplit_once('.') {
        let p = point_prop(e, def, base)?;
        return match axis {
            "x" => Some(p.x),
            "y" => Some(p.y),
            _ => None,
        };
    }
    num_prop(e, def, name)
}

/// Leaf values for a plugin entity.
pub(crate) fn leaf_value(ctx: Ctx<'_>, e: &Entity, def: &CompiledType, leaf: &Leaf, depth: usize) -> Option<f64> {
    match leaf {
        Leaf::Prop(n) => num_prop(e, Some(def), n),
        Leaf::PropPoint(n, a) => point_prop(e, Some(def), n).map(|p| if *a == Axis::X { p.x } else { p.y }),
        Leaf::RefAnchor(prop, anchor, axis) => {
            let target = match e.props.get(prop) {
                Some(PropValue::Ref(r)) => r.entity,
                _ => return None,
            };
            let w = world_anchor(ctx, target, anchor, depth)?;
            let local = e.transform.inverse().ok()?.apply(w);
            Some(if *axis == Axis::X { local.x } else { local.y })
        }
        Leaf::RefParam(prop, name) => {
            let target = match e.props.get(prop) {
                Some(PropValue::Ref(r)) => r.entity,
                _ => return None,
            };
            let t = ctx.view.entity(target)?;
            param_value(ctx, t, name)
        }
        Leaf::AxisScale => ctx.view.settings().time_axis.map(|a| a.mm_per_second),
        Leaf::AxisOrigin => ctx.view.settings().time_axis.map(|a| a.origin_s),
    }
}

fn eval_point(ctx: Ctx<'_>, e: &Entity, def: &CompiledType, c: &Compiled, depth: usize) -> Option<Point> {
    let v = c.eval(&mut |l| leaf_value(ctx, e, def, l, depth))?;
    Some(Point::new(*v.first()?, *v.get(1)?))
}

fn eval_scalar(ctx: Ctx<'_>, e: &Entity, def: &CompiledType, c: &Compiled, depth: usize) -> Option<f64> {
    c.eval(&mut |l| leaf_value(ctx, e, def, l, depth))?.first().copied()
}

/// Fill `{prop}` placeholders.
#[must_use]
pub fn fill_template(template: &str, e: &Entity) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) => {
                let key = &after[..j];
                match e.props.get(key) {
                    Some(PropValue::Text(t)) => out.push_str(t),
                    Some(PropValue::Number(n)) => out.push_str(&format_number(*n, 1)),
                    Some(PropValue::Bool(b)) => out.push_str(if *b { "yes" } else { "no" }),
                    _ => {}
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Format a number with at most `decimals` fraction digits, trimming zeros.
#[must_use]
pub fn format_number(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        if t == "-0" { "0".into() } else { t.to_owned() }
    } else {
        s
    }
}

fn evaluate_plugin(ctx: Ctx<'_>, e: &Entity, def: &CompiledType, depth: usize) -> Evaluated {
    let mut ev = Evaluated::empty();
    let t = e.transform;
    for (name, kind, c) in &def.anchors {
        match eval_point(ctx, e, def, c, depth) {
            Some(p) => ev.anchors.push(Anchor { name: name.clone().into(), kind: *kind, point: t.apply(p) }),
            None => ev.error = Some(format!("anchor `{name}` cannot be evaluated")),
        }
    }
    for prim in &def.primitives {
        let local: Option<Drawable> = match prim {
            CompiledPrim::Line(a, b, st) => (|| {
                Some(Drawable::Shape(
                    Shape::Line(Segment::new(eval_point(ctx, e, def, a, depth)?, eval_point(ctx, e, def, b, depth)?)),
                    st.clone(),
                ))
            })(),
            CompiledPrim::Polyline(pts, closed, st) => {
                pts.iter().map(|c| eval_point(ctx, e, def, c, depth)).collect::<Option<Vec<_>>>().map(|p| {
                    Drawable::Shape(
                        Shape::Polyline(Polyline { points: p, bulges: Vec::new(), closed: *closed }),
                        st.clone(),
                    )
                })
            }
            CompiledPrim::Polygon(pts, st) => pts
                .iter()
                .map(|c| eval_point(ctx, e, def, c, depth))
                .collect::<Option<Vec<_>>>()
                .map(|p| Drawable::Shape(Shape::Polygon(Polygon { outer: p, holes: Vec::new() }), st.clone())),
            CompiledPrim::Circle(c, r, st) => (|| {
                let center = eval_point(ctx, e, def, c, depth)?;
                let radius = eval_scalar(ctx, e, def, r, depth)?;
                Circle::new(center, radius).ok().map(|c| Drawable::Shape(Shape::Circle(c), st.clone()))
            })(),
            CompiledPrim::Arc(c, r, s0, sw, st) => (|| {
                let a = Arc::new(
                    eval_point(ctx, e, def, c, depth)?,
                    eval_scalar(ctx, e, def, r, depth)?,
                    eval_scalar(ctx, e, def, s0, depth)?,
                    eval_scalar(ctx, e, def, sw, depth)?,
                )
                .ok()?;
                Some(Drawable::Shape(Shape::Arc(a), st.clone()))
            })(),
            CompiledPrim::Text { position, content, height, rotation, halign, valign, style } => (|| {
                let pos = eval_point(ctx, e, def, position, depth)?;
                let h = eval_scalar(ctx, e, def, height, depth)?;
                let rot = match rotation {
                    Some(r) => eval_scalar(ctx, e, def, r, depth)?,
                    None => 0.0,
                };
                (h > 0.0).then(|| {
                    Drawable::Text(
                        Text {
                            position: pos,
                            content: fill_template(content, e),
                            height: h,
                            rotation: rot,
                            halign: *halign,
                            valign: *valign,
                        },
                        style.clone(),
                    )
                })
            })(),
        };
        let Some(d) = local else {
            ev.error = Some("a primitive cannot be evaluated".into());
            continue;
        };
        let world = match d {
            Drawable::Shape(s, st) => match s.transform(t, TransformPolicy::Convert) {
                Ok(w) => Some(Drawable::Shape(w, st)),
                Err(_) => None,
            },
            Drawable::Text(tx, st) => match Shape::Text(tx).transform(t, TransformPolicy::Strict) {
                Ok(Shape::Text(w)) => Some(Drawable::Text(w, st)),
                _ => None,
            },
            other => Some(other),
        };
        if let Some(w) = world {
            ev.drawables.push(w);
        }
    }
    ev.bbox = bbox_of(&ev.drawables);
    if ev.bbox.is_empty() {
        ev.bbox = Aabb::from_points(ev.anchors.iter().map(|a| a.point));
    }
    ev
}

fn anchor_prop(ctx: Ctx<'_>, e: &Entity, key: &str, depth: usize) -> Option<Point> {
    match e.props.get(key) {
        Some(PropValue::Anchor(a)) => world_anchor(ctx, a.entity, &a.anchor, depth),
        Some(PropValue::Point(p)) => Some(e.transform.apply(*p)),
        _ => None,
    }
}

fn text_prop<'e>(e: &'e Entity, key: &str) -> Option<&'e str> {
    match e.props.get(key) {
        Some(PropValue::Text(t)) => Some(t),
        _ => None,
    }
}

/// Format a measured value in the document's display unit.
#[must_use]
pub fn format_measure(value: f64, angular: bool, unit: LengthUnit, decimals: usize) -> String {
    if angular {
        format!("{}°", format_number(value.to_degrees(), decimals))
    } else {
        format!("{} {}", format_number(unit.from_mm(value), decimals), unit.symbol())
    }
}

/// Associative dimension evaluation.
///
/// Properties: `kind` (`linear` | `radial` | `angular`), `a`/`b` (anchors or points),
/// `through` (point the dimension line passes through), `orientation`
/// (`aligned` | `horizontal` | `vertical`), `circle` (ref), `angle`, `line1`/`line2`
/// (refs), `radius`, `textHeight`, `precision`, `prefix`.
fn evaluate_dimension(ctx: Ctx<'_>, e: &Entity, depth: usize) -> Evaluated {
    let mut ev = Evaluated::empty();
    let num = |k: &str| e.props.get(k).and_then(PropValue::as_number);
    let text_h = num("textHeight").filter(|v| *v > 0.0).unwrap_or(100.0);
    let style = DimensionStyle {
        extension_gap: text_h * 0.3,
        extension_overshoot: text_h * 0.5,
        arrow_size: text_h * 0.8,
        text_height: text_h,
    };
    let decimals = num("precision").map_or(0, |p| p.clamp(0.0, 6.0) as usize);
    let unit = ctx.view.settings().display_unit;
    let kind = text_prop(e, "kind").unwrap_or("linear");
    let geom = match kind {
        "linear" => (|| {
            let a = anchor_prop(ctx, e, "a", depth)?;
            let b = anchor_prop(ctx, e, "b", depth)?;
            let through = match e.props.get("through") {
                Some(PropValue::Point(p)) => e.transform.apply(*p),
                _ => a.midpoint(b),
            };
            let orient = match text_prop(e, "orientation") {
                Some("horizontal") => LinearKind::Horizontal,
                Some("vertical") => LinearKind::Vertical,
                _ => LinearKind::Aligned,
            };
            dimension::linear(a, b, through, orient, style).ok()
        })(),
        "radial" => (|| {
            let target = match e.props.get("circle") {
                Some(PropValue::Ref(r)) => r.entity,
                _ => return None,
            };
            let te = evaluate_depth(ctx, target, depth + 1);
            let (center, radius) = te.drawables.iter().find_map(|d| match d {
                Drawable::Shape(Shape::Circle(c), _) => Some((c.center, c.radius)),
                Drawable::Shape(Shape::Arc(a), _) => Some((a.center, a.radius)),
                _ => None,
            })?;
            dimension::radial(center, radius, num("angle").unwrap_or(core::f64::consts::FRAC_PI_4), style).ok()
        })(),
        "angular" => (|| {
            let line_pts = |k: &str| -> Option<(Point, Point)> {
                let id = match e.props.get(k) {
                    Some(PropValue::Ref(r)) => r.entity,
                    _ => return None,
                };
                let te = evaluate_depth(ctx, id, depth + 1);
                Some((te.anchor("start")?, te.anchor("end")?))
            };
            let (a0, a1) = line_pts("line1")?;
            let (b0, b1) = line_pts("line2")?;
            let hit = dotloom_geometry::intersect::intersect(
                &dotloom_geometry::Curve::Line(Segment::new(a0 - (a1 - a0) * 1e6, a1 + (a1 - a0) * 1e6)),
                &dotloom_geometry::Curve::Line(Segment::new(b0 - (b1 - b0) * 1e6, b1 + (b1 - b0) * 1e6)),
                dotloom_geometry::ModelTolerance::DEFAULT,
            );
            let vertex = hit.points.first()?.point;
            let far = |p: Point, q: Point| if vertex.distance(p) > vertex.distance(q) { p } else { q };
            dimension::angular(vertex, far(a0, a1), far(b0, b1), num("radius").unwrap_or(text_h * 10.0), style).ok()
        })(),
        _ => None,
    };
    let Some(g) = geom else {
        ev.error = Some("dimension references cannot be resolved".into());
        return ev;
    };
    let style = PrimStyle::default();
    for l in &g.lines {
        ev.drawables.push(Drawable::Shape(Shape::Line(*l), style.clone()));
    }
    if let Some(a) = g.arc {
        ev.drawables.push(Drawable::Shape(Shape::Arc(a), style.clone()));
    }
    for a in &g.arrows {
        ev.drawables.push(Drawable::Arrow { tip: a.tip, direction: a.direction, size: text_h * 0.8 });
    }
    let label = format_measure(g.value, kind == "angular", unit, decimals);
    let prefix = text_prop(e, "prefix").unwrap_or(if kind == "radial" { "R " } else { "" });
    ev.drawables.push(Drawable::Text(
        Text {
            position: g.text_position,
            content: format!("{prefix}{label}"),
            height: text_h,
            rotation: g.text_rotation,
            halign: HAlign::Center,
            valign: VAlign::Middle,
        },
        style,
    ));
    ev.anchors.push(Anchor { name: "text".into(), kind: AnchorKind::Insert, point: g.text_position });
    ev.measured = Some(g.value);
    ev.bbox = bbox_of(&ev.drawables);
    ev
}

/// Cache of evaluations keyed by entity.
#[derive(Debug, Clone, Default)]
pub struct EvalCache {
    map: BTreeMap<EntityId, Evaluated>,
}

impl EvalCache {
    /// Cached or fresh evaluation.
    pub fn get(&mut self, ctx: Ctx<'_>, id: EntityId) -> &Evaluated {
        self.map.entry(id).or_insert_with(|| evaluate(ctx, id))
    }

    /// Cached evaluation without computing.
    #[must_use]
    pub fn peek(&self, id: EntityId) -> Option<&Evaluated> {
        self.map.get(&id)
    }

    /// Drop entries.
    pub fn invalidate(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        for id in ids {
            self.map.remove(&id);
        }
    }

    /// Drop everything.
    pub fn clear(&mut self) {
        self.map.clear();
    }
}
