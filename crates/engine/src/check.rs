//! Independent validation of hard rules before commit (DL-TEST-3).
//!
//! Residuals are recomputed from evaluated world geometry with plain `f64`
//! formulas from `dotloom-geometry`, independently of the solver's expression
//! trees and derivatives. A transaction commits only if every hard rule of the
//! affected set passes here.

use std::collections::BTreeMap;

use dotloom_document::{
    Cmp, Constraint, EntityId, LineRef, ParamRef, ParamSlot, RuleSpec, StrengthSpec,
    builtin::{is_builtin, types},
};
use dotloom_geometry::{Point, Shape, Vector, normalize_angle_signed, units::Dim};

use crate::{
    build::Scales,
    eval::{Ctx, Drawable, Evaluated, evaluate, leaf_value, param_value, plugin_for},
    lang::compile,
};

/// Evaluation cache for one validation pass.
pub(crate) struct Checker<'a> {
    ctx: Ctx<'a>,
    scales: Scales,
    cache: BTreeMap<EntityId, Evaluated>,
}

/// Relative tolerance of the independent check (looser than the solver's 1e-9).
pub const CHECK_REL_TOL: f64 = 1e-7;

impl<'a> Checker<'a> {
    pub fn new(ctx: Ctx<'a>, scales: Scales) -> Self {
        Self { ctx, scales, cache: BTreeMap::new() }
    }

    fn ev(&mut self, id: EntityId) -> &Evaluated {
        let ctx = self.ctx;
        self.cache.entry(id).or_insert_with(|| evaluate(ctx, id))
    }

    fn anchor(&mut self, id: EntityId, name: &str) -> Result<Point, String> {
        self.ev(id).anchor(name).ok_or_else(|| format!("{id} has no anchor `{name}`"))
    }

    fn line(&mut self, l: &LineRef) -> Result<(Point, Point), String> {
        Ok((self.anchor(l.from.entity, &l.from.anchor)?, self.anchor(l.to.entity, &l.to.anchor)?))
    }

    fn param(&self, p: &ParamRef) -> Result<f64, String> {
        let name = match &p.slot {
            ParamSlot::Geom(n) | ParamSlot::Prop(n) => n,
        };
        let e = self.ctx.view.entity(p.entity).ok_or_else(|| format!("{} does not exist", p.entity))?;
        param_value(self.ctx, e, name).ok_or_else(|| format!("{}.{name} has no value", p.entity))
    }

    fn circle(&mut self, id: EntityId) -> Result<(Point, f64), String> {
        let ctx = self.ctx;
        let e = ctx.view.entity(id).ok_or_else(|| format!("{id} does not exist"))?;
        if is_builtin(&e.type_id) && matches!(e.type_id.as_str(), types::CIRCLE | types::ARC) {
            return self
                .ev(id)
                .drawables
                .iter()
                .find_map(|d| match d {
                    Drawable::Shape(Shape::Circle(c), _) => Some((c.center, c.radius)),
                    Drawable::Shape(Shape::Arc(a), _) => Some((a.center, a.radius)),
                    _ => None,
                })
                .ok_or_else(|| format!("{id} is not a circle"));
        }
        let def = plugin_for(ctx.registry, e).map_err(|r| format!("{r:?}"))?;
        let (cc, rc) = def.circle.as_ref().ok_or("no circle interpretation")?;
        let c = cc.eval(&mut |l| leaf_value(ctx, e, def, l, 0)).ok_or("circle center")?;
        let r = rc.eval(&mut |l| leaf_value(ctx, e, def, l, 0)).ok_or("circle radius")?;
        let (Some(x), Some(y), Some(r)) = (c.first(), c.get(1), r.first()) else { return Err("circle".into()) };
        let scale = match e.transform.linear_kind(1e-9) {
            dotloom_geometry::LinearKind::Similarity { scale, .. } => scale,
            _ => return Err("non-uniform transform".into()),
        };
        Ok((e.transform.apply(Point::new(*x, *y)), r * scale))
    }

    fn len_tol(&self) -> f64 {
        (CHECK_REL_TOL * self.scales.length).max(1e-9)
    }

    fn tol_for(&self, d: Dim) -> f64 {
        (CHECK_REL_TOL * self.scales.of(d)).max(1e-12)
    }

    fn param_dim(&self, p: &ParamRef) -> Dim {
        let name = match &p.slot {
            ParamSlot::Geom(n) | ParamSlot::Prop(n) => n.as_str(),
        };
        if matches!(name, "start" | "sweep" | "rotation") {
            return Dim::ANGLE;
        }
        let Some(e) = self.ctx.view.entity(p.entity) else { return Dim::LENGTH };
        if is_builtin(&e.type_id) {
            return Dim::LENGTH;
        }
        plugin_for(self.ctx.registry, e)
            .ok()
            .and_then(|d| d.params.iter().find(|x| x.name == name).map(|x| x.dim))
            .unwrap_or(Dim::LENGTH)
    }

    fn cmp_residual(v: f64, op: Cmp) -> f64 {
        match op {
            Cmp::Eq => v.abs(),
            Cmp::Le => v.max(0.0),
            Cmp::Ge => (-v).max(0.0),
        }
    }

    /// Residual and tolerance of a document constraint.
    pub fn constraint(&mut self, c: &Constraint) -> Result<(f64, f64), String> {
        let lt = self.len_tol();
        let at = CHECK_REL_TOL;
        let dist = |a: Point, b: Point| a.distance(b);
        let signed = |p: Point, a: Point, b: Point| -> f64 {
            let d = b - a;
            let l = d.length();
            if l == 0.0 { f64::NAN } else { d.cross(p - a) / l }
        };
        let angle_between = |u: Vector, v: Vector| dotloom_geometry::math::atan2(u.cross(v), u.dot(v));
        Ok(match &c.rule {
            RuleSpec::Fix { param, value } => ((self.param(param)? - value).abs(), self.tol_for(self.param_dim(param))),
            RuleSpec::Equal { a, b } => ((self.param(a)? - self.param(b)?).abs(), self.tol_for(self.param_dim(a))),
            RuleSpec::AllEqual { params } | RuleSpec::EqualSpacing { params } => {
                let v = params.iter().map(|p| self.param(p)).collect::<Result<Vec<_>, _>>()?;
                let tol = params.first().map_or(lt, |p| self.tol_for(self.param_dim(p)));
                let r = if matches!(c.rule, RuleSpec::AllEqual { .. }) {
                    v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max)
                } else {
                    let d: Vec<f64> = v.windows(2).map(|w| w[1] - w[0]).collect();
                    d.iter().map(|x| (x - d.first().copied().unwrap_or(0.0)).abs()).fold(0.0, f64::max)
                };
                (r, tol)
            }
            RuleSpec::Linear { terms, op, rhs } => {
                let mut s = -rhs;
                for t in terms {
                    s += t.coef * self.param(&t.param)?;
                }
                let tol = terms.first().map_or(lt, |t| self.tol_for(self.param_dim(&t.param)));
                (Self::cmp_residual(s, *op), tol)
            }
            RuleSpec::Ratio { a, b, k } => {
                ((self.param(a)? - k * self.param(b)?).abs(), self.tol_for(self.param_dim(a)))
            }
            RuleSpec::Coincident { a, b } => {
                (dist(self.anchor(a.entity, &a.anchor)?, self.anchor(b.entity, &b.anchor)?), lt)
            }
            RuleSpec::Horizontal { a, b } => {
                ((self.anchor(a.entity, &a.anchor)?.y - self.anchor(b.entity, &b.anchor)?.y).abs(), lt)
            }
            RuleSpec::Vertical { a, b } => {
                ((self.anchor(a.entity, &a.anchor)?.x - self.anchor(b.entity, &b.anchor)?.x).abs(), lt)
            }
            RuleSpec::FixPoint { a, at: p } => (dist(self.anchor(a.entity, &a.anchor)?, *p), lt),
            RuleSpec::Distance { a, b, value } => {
                ((dist(self.anchor(a.entity, &a.anchor)?, self.anchor(b.entity, &b.anchor)?) - value).abs(), lt)
            }
            RuleSpec::PointLineDistance { point, line, value } => {
                let p = self.anchor(point.entity, &point.anchor)?;
                let (a, b) = self.line(line)?;
                ((signed(p, a, b) - value).abs(), lt)
            }
            RuleSpec::PointOnLine { point, line } => {
                let p = self.anchor(point.entity, &point.anchor)?;
                let (a, b) = self.line(line)?;
                (signed(p, a, b).abs(), lt)
            }
            RuleSpec::PointOnCircle { point, circle } => {
                let p = self.anchor(point.entity, &point.anchor)?;
                let (c, r) = self.circle(*circle)?;
                ((p.distance(c) - r).abs(), lt)
            }
            RuleSpec::Length { line, value } => {
                let (a, b) = self.line(line)?;
                ((a.distance(b) - value).abs(), lt)
            }
            RuleSpec::EqualLength { a, b } => {
                let ((a1, b1), (a2, b2)) = (self.line(a)?, self.line(b)?);
                ((a1.distance(b1) - a2.distance(b2)).abs(), lt)
            }
            RuleSpec::Parallel { a, b } | RuleSpec::Perpendicular { a, b } | RuleSpec::Angle { a, b, .. } => {
                let ((a1, b1), (a2, b2)) = (self.line(a)?, self.line(b)?);
                let th = angle_between(b1 - a1, b2 - a2);
                let r = match &c.rule {
                    RuleSpec::Parallel { .. } => dotloom_geometry::math::sin(th).abs(),
                    RuleSpec::Perpendicular { .. } => dotloom_geometry::math::cos(th).abs(),
                    RuleSpec::Angle { value, .. } => normalize_angle_signed(th - value).abs(),
                    _ => 0.0,
                };
                (r, at)
            }
            RuleSpec::Concentric { a, b } => {
                let ((c1, _), (c2, _)) = (self.circle(*a)?, self.circle(*b)?);
                (c1.distance(c2), lt)
            }
            RuleSpec::Radius { circle, value } => ((self.circle(*circle)?.1 - value).abs(), lt),
            RuleSpec::EqualRadius { a, b } => ((self.circle(*a)?.1 - self.circle(*b)?.1).abs(), lt),
            RuleSpec::TangentLineCircle { line, circle, side } => {
                let (a, b) = self.line(line)?;
                let (c, r) = self.circle(*circle)?;
                ((side.signum() * signed(c, a, b) - r).abs(), lt)
            }
            RuleSpec::TangentCircles { a, b, internal, sign } => {
                let ((c1, r1), (c2, r2)) = (self.circle(*a)?, self.circle(*b)?);
                let target = if *internal { sign.signum() * (r1 - r2) } else { r1 + r2 };
                ((c1.distance(c2) - target).abs(), lt)
            }
            RuleSpec::Expression { entity, lhs, op, rhs } => {
                let ctx = self.ctx;
                let e = ctx.view.entity(*entity).ok_or("missing entity")?;
                let def = plugin_for(ctx.registry, e).map_err(|r| format!("{r:?}"))?;
                let diff = compile(&format!("({lhs}) - ({rhs})"), &def.scope).map_err(|x| x.to_string())?;
                let dim = match diff.ty {
                    crate::lang::Ty::Scalar(d) => d,
                    crate::lang::Ty::Vector(_) => return Err("vector expression".into()),
                };
                let v = diff
                    .eval(&mut |l| leaf_value(ctx, e, def, l, 0))
                    .and_then(|v| v.first().copied())
                    .ok_or("cannot evaluate")?;
                (Self::cmp_residual(v, *op), self.tol_for(dim))
            }
        })
    }

    /// Residuals of the hard templates of a plugin entity: `(label, residual, tol)`.
    pub fn templates(&self, id: EntityId) -> Vec<(String, f64, f64)> {
        let ctx = self.ctx;
        let Some(e) = ctx.view.entity(id) else { return Vec::new() };
        let Ok(def) = plugin_for(ctx.registry, e) else { return Vec::new() };
        def.templates
            .iter()
            .filter(|t| t.def.strength == StrengthSpec::Required)
            .map(|t| {
                let v = t
                    .diff
                    .eval(&mut |l| leaf_value(ctx, e, def, l, 0))
                    .and_then(|v| v.first().copied())
                    .unwrap_or(f64::NAN);
                let r = Self::cmp_residual(v, t.def.op);
                (
                    t.def.label.clone().unwrap_or_else(|| t.def.lhs.clone()),
                    if r.is_nan() { f64::INFINITY } else { r },
                    self.tol_for(t.dim),
                )
            })
            .collect()
    }
}
