//! Building solver problems from documents (document → [`dotloom_constraints::Problem`]).

use std::collections::BTreeMap;

use dotloom_constraints::{
    Expr, PointExpr, Problem, Rule, Strength, Target, VarId, Variable,
    rules::{self, CircleTangency},
};
use dotloom_document::{
    Cmp, Constraint, ConstraintId, Entity, EntityId, LineRef, ParamRef, ParamSlot, PropValue, RuleSpec, StrengthSpec,
    builtin::{ParamDim, geometry_params, is_builtin, types},
};
use dotloom_geometry::{LinearKind, Shape, units::Dim};

use crate::{
    eval::{Ctx, param_value, plugin_for},
    lang::{Axis, Leaf, compile},
    registry::{CompiledType, PropDef},
};

/// Bit marking synthetic rule IDs (templates, edits, drags) so they never collide
/// with document constraint IDs.
pub const SYNTHETIC: u64 = 1 << 63;

/// Origin of a solver rule, for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleOrigin {
    /// A document constraint.
    Constraint(ConstraintId),
    /// A plugin template instantiated for an entity.
    Template {
        /// Entity.
        entity: EntityId,
        /// Template index.
        index: usize,
    },
    /// A typed edit (exact value).
    Edit {
        /// Entity.
        entity: EntityId,
        /// Parameter.
        param: String,
    },
    /// Drag target.
    Drag,
}

/// Characteristic scales used for variables and rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scales {
    /// Length (mm).
    pub length: f64,
    /// Time (s).
    pub time: f64,
}

impl Scales {
    /// Scale for a dimension.
    #[must_use]
    pub fn of(&self, d: Dim) -> f64 {
        if d == Dim::LENGTH {
            self.length
        } else if d == Dim::TIME {
            self.time
        } else {
            1.0
        }
    }
}

/// Problem under construction.
#[derive(Debug)]
pub struct Builder<'a> {
    ctx: Ctx<'a>,
    /// Problem.
    pub problem: Problem,
    /// `(entity, param)` → variable.
    pub vars: BTreeMap<(EntityId, String), VarId>,
    /// Origin per rule index.
    pub origins: Vec<RuleOrigin>,
    /// Scales.
    pub scales: Scales,
    /// Entities edited directly by the transaction: their other parameters keep
    /// their values more strongly than those of merely connected entities.
    pub edited: std::collections::BTreeSet<EntityId>,
    /// First (strict) attempt: parameters of edited entities that are not edited
    /// themselves, and `high`-stay properties, are held fixed.
    pub pin: bool,
    /// Parameters edited directly (never pinned).
    pub edited_params: std::collections::BTreeSet<(EntityId, String)>,
    next_synthetic: u64,
}

fn strength(s: StrengthSpec) -> Strength {
    match s {
        StrengthSpec::Required => Strength::Required,
        StrengthSpec::Strong => Strength::Strong,
        StrengthSpec::Medium => Strength::Medium,
        StrengthSpec::Weak => Strength::Weak,
    }
}

fn cmp(c: Cmp) -> rules::Cmp {
    match c {
        Cmp::Eq => rules::Cmp::Eq,
        Cmp::Le => rules::Cmp::Le,
        Cmp::Ge => rules::Cmp::Ge,
    }
}

/// Error while compiling a rule; the rule is reported as unsupported.
type BResult<T> = Result<T, String>;

fn pe_add(a: &PointExpr, b: &PointExpr) -> PointExpr {
    PointExpr::new(Expr::add(a.x.clone(), b.x.clone()), Expr::add(a.y.clone(), b.y.clone()))
}

fn pe_mid(a: &PointExpr, b: &PointExpr) -> PointExpr {
    PointExpr::new(
        Expr::mul(Expr::c(0.5), Expr::add(a.x.clone(), b.x.clone())),
        Expr::mul(Expr::c(0.5), Expr::add(a.y.clone(), b.y.clone())),
    )
}

impl<'a> Builder<'a> {
    /// New builder.
    #[must_use]
    pub fn new(ctx: Ctx<'a>, scales: Scales) -> Self {
        Self {
            ctx,
            problem: Problem::default(),
            vars: BTreeMap::new(),
            origins: Vec::new(),
            scales,
            edited: std::collections::BTreeSet::new(),
            pin: false,
            edited_params: std::collections::BTreeSet::new(),
            next_synthetic: 0,
        }
    }

    fn entity(&self, id: EntityId) -> BResult<&'a Entity> {
        self.ctx.view.entity(id).ok_or_else(|| format!("{id} does not exist"))
    }

    /// Dimension, solver-ability and stay multiplier of a parameter.
    fn param_info(&self, e: &Entity, name: &str) -> Option<(Dim, bool, f64)> {
        if is_builtin(&e.type_id) {
            let g = e.geometry.as_ref()?;
            return geometry_params(g)
                .into_iter()
                .find(|(n, _, _)| n == name)
                .map(|(_, _, d)| (if d == ParamDim::Angle { Dim::ANGLE } else { Dim::LENGTH }, true, 1.0));
        }
        let def = plugin_for(self.ctx.registry, e).ok()?;
        if let Some(p) = def.params.iter().find(|p| p.name == name) {
            return Some((p.dim, true, p.stay));
        }
        match def.def.props.get(name) {
            Some(PropDef::Number { dim, .. }) => Some((dim.dim(), false, 1.0)),
            _ => None,
        }
    }

    /// Variable for a parameter (created on first use). Locked entities, read-only
    /// entities and non-solver properties produce fixed variables.
    pub fn var(&mut self, id: EntityId, name: &str) -> BResult<VarId> {
        if let Some(v) = self.vars.get(&(id, name.to_owned())) {
            return Ok(*v);
        }
        let e = self.entity(id)?;
        let (dim, solvable, stay) =
            self.param_info(e, name).ok_or_else(|| format!("{id} has no numeric parameter `{name}`"))?;
        let value = param_value(self.ctx, e, name).ok_or_else(|| format!("{id}.{name} has no value"))?;
        let read_only = !is_builtin(&e.type_id) && plugin_for(self.ctx.registry, e).is_err();
        let pinned = self.pin
            && !self.edited_params.contains(&(id, name.to_owned()))
            && (self.edited.contains(&id) || stay >= 5.0);
        let fixed = e.locked || !solvable || read_only || pinned;
        let stay = if self.edited.contains(&id) { stay * 5.0 } else { stay };
        let v = self.problem.add_var(
            Variable::new(value).scale(self.scales.of(dim)).fixed(fixed).label(format!("{id}.{name}")).stay(stay),
        );
        self.vars.insert((id, name.to_owned()), v);
        Ok(v)
    }

    /// Expression of a parameter.
    pub fn param_expr(&mut self, p: &ParamRef) -> BResult<Expr> {
        let name = match &p.slot {
            ParamSlot::Geom(n) | ParamSlot::Prop(n) => n,
        };
        Ok(Expr::Var(self.var(p.entity, name)?))
    }

    fn pv(&mut self, id: EntityId, x: &str, y: &str) -> BResult<PointExpr> {
        Ok(PointExpr::vars(self.var(id, x)?, self.var(id, y)?))
    }

    /// World-space anchor expression.
    pub fn anchor_expr(&mut self, id: EntityId, name: &str) -> BResult<PointExpr> {
        self.anchor_expr_depth(id, name, 0)
    }

    fn anchor_expr_depth(&mut self, id: EntityId, name: &str, depth: usize) -> BResult<PointExpr> {
        if depth > crate::eval::MAX_REF_DEPTH {
            return Err("reference chain too deep or cyclic".into());
        }
        let e = self.entity(id)?;
        let local = if is_builtin(&e.type_id) {
            self.builtin_anchor(e, name)?
        } else {
            let def = plugin_for(self.ctx.registry, e).map_err(|r| format!("{id} is read-only: {r:?}"))?;
            let (_, _, c) =
                def.anchors.iter().find(|(n, _, _)| n == name).ok_or_else(|| format!("{id} has no anchor `{name}`"))?;
            let parts = self.instantiate(e, def, c, depth)?;
            match parts.as_slice() {
                [x, y] => PointExpr::new(x.clone(), y.clone()),
                _ => return Err(format!("anchor `{name}` is not a point")),
            }
        };
        Ok(local.transformed(e.transform.m))
    }

    fn builtin_anchor(&mut self, e: &Entity, name: &str) -> BResult<PointExpr> {
        let id = e.id;
        let g = e.geometry.as_ref().ok_or("missing geometry")?;
        let unsupported = || format!("anchor `{name}` of {} cannot be constrained", g.kind().name());
        Ok(match g {
            Shape::Point(_) if name == "point" => self.pv(id, "x", "y")?,
            Shape::Line(_) => match name {
                "start" => self.pv(id, "a.x", "a.y")?,
                "end" => self.pv(id, "b.x", "b.y")?,
                "mid" => {
                    let (a, b) = (self.pv(id, "a.x", "a.y")?, self.pv(id, "b.x", "b.y")?);
                    pe_mid(&a, &b)
                }
                _ => return Err(unsupported()),
            },
            Shape::Polyline(p) => {
                let n = p.points.len();
                let vert = |s: &mut Self, i: usize| s.pv(id, &format!("v{i}.x"), &format!("v{i}.y"));
                if let Some(i) = name.strip_prefix('v').and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < n) {
                    vert(self, i)?
                } else if name == "start" && !p.closed && n > 0 {
                    vert(self, 0)?
                } else if name == "end" && !p.closed && n > 0 {
                    vert(self, n - 1)?
                } else if let Some(i) =
                    name.strip_prefix('m').and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < p.segment_count())
                {
                    let (a, b) = (vert(self, i)?, vert(self, (i + 1) % n)?);
                    let m = pe_mid(&a, &b);
                    let bulge = p.bulge(i);
                    if bulge == 0.0 {
                        m
                    } else {
                        // Arc midpoint = chord midpoint − perp(B − A)·bulge/2 (bulge is a constant).
                        let k = bulge * 0.5;
                        PointExpr::new(
                            Expr::add(m.x, Expr::mul(Expr::c(k), Expr::sub(b.y.clone(), a.y.clone()))),
                            Expr::sub(m.y, Expr::mul(Expr::c(k), Expr::sub(b.x, a.x))),
                        )
                    }
                } else {
                    return Err(unsupported());
                }
            }
            Shape::Rect(_) => {
                let (x, y, w, h) = (
                    Expr::Var(self.var(id, "x")?),
                    Expr::Var(self.var(id, "y")?),
                    Expr::Var(self.var(id, "width")?),
                    Expr::Var(self.var(id, "height")?),
                );
                let c = |i: usize| -> PointExpr {
                    match i {
                        0 => PointExpr::new(x.clone(), y.clone()),
                        1 => PointExpr::new(Expr::add(x.clone(), w.clone()), y.clone()),
                        2 => PointExpr::new(Expr::add(x.clone(), w.clone()), Expr::add(y.clone(), h.clone())),
                        _ => PointExpr::new(x.clone(), Expr::add(y.clone(), h.clone())),
                    }
                };
                match name {
                    "c0" => c(0),
                    "c1" => c(1),
                    "c2" => c(2),
                    "c3" => c(3),
                    "e0" => pe_mid(&c(0), &c(1)),
                    "e1" => pe_mid(&c(1), &c(2)),
                    "e2" => pe_mid(&c(2), &c(3)),
                    "e3" => pe_mid(&c(3), &c(0)),
                    "center" => pe_mid(&c(0), &c(2)),
                    _ => return Err(unsupported()),
                }
            }
            Shape::Circle(_) => {
                let c = self.pv(id, "cx", "cy")?;
                let r = Expr::Var(self.var(id, "r")?);
                let q = |dx: f64, dy: f64| {
                    pe_add(&c, &PointExpr::new(Expr::mul(Expr::c(dx), r.clone()), Expr::mul(Expr::c(dy), r.clone())))
                };
                match name {
                    "center" => c.clone(),
                    "q0" => q(1.0, 0.0),
                    "q1" => q(0.0, 1.0),
                    "q2" => q(-1.0, 0.0),
                    "q3" => q(0.0, -1.0),
                    _ => return Err(unsupported()),
                }
            }
            Shape::Arc(_) => {
                let (cx, cy, r) =
                    (Expr::Var(self.var(id, "cx")?), Expr::Var(self.var(id, "cy")?), Expr::Var(self.var(id, "r")?));
                let (s, sw) = (Expr::Var(self.var(id, "start")?), Expr::Var(self.var(id, "sweep")?));
                match name {
                    "center" => PointExpr::new(cx, cy),
                    "start" => rules::polar_point(cx, cy, r, s),
                    "end" => rules::polar_point(cx, cy, r, Expr::add(s, sw)),
                    "mid" => rules::polar_point(cx, cy, r, Expr::add(s, Expr::mul(Expr::c(0.5), sw))),
                    _ => return Err(unsupported()),
                }
            }
            Shape::Polygon(p) => {
                match name.strip_prefix('v').and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < p.outer.len()) {
                    Some(i) => self.pv(id, &format!("v{i}.x"), &format!("v{i}.y"))?,
                    None => return Err(unsupported()),
                }
            }
            Shape::Path(p) => {
                let n = p.on_curve_points().len();
                let on_curve: Vec<usize> = {
                    // Index of each on-curve point in the parameter numbering (p0, p1, ...).
                    let mut idx = Vec::new();
                    let mut k = 0;
                    for el in &p.elements {
                        let count = match el {
                            dotloom_geometry::PathEl::MoveTo(_) | dotloom_geometry::PathEl::LineTo(_) => 1,
                            dotloom_geometry::PathEl::QuadTo(..) => 2,
                            dotloom_geometry::PathEl::CubicTo(..) => 3,
                            dotloom_geometry::PathEl::Close => 0,
                        };
                        if count > 0 {
                            idx.push(k + count - 1);
                        }
                        k += count;
                    }
                    idx
                };
                let pick = match name {
                    "start" if n > 0 => Some(0),
                    "end" if n > 0 => Some(n - 1),
                    _ => name.strip_prefix('v').and_then(|s| s.parse::<usize>().ok()).filter(|i| *i < n),
                };
                match pick.and_then(|i| on_curve.get(i).copied()) {
                    Some(k) => self.pv(id, &format!("p{k}.x"), &format!("p{k}.y"))?,
                    None => return Err(unsupported()),
                }
            }
            Shape::Text(_) if name == "insert" => self.pv(id, "x", "y")?,
            _ => return Err(unsupported()),
        })
    }

    /// Instantiate a compiled plugin expression for entity `e` (local coordinates).
    fn instantiate(
        &mut self,
        e: &Entity,
        def: &CompiledType,
        c: &crate::lang::Compiled,
        depth: usize,
    ) -> BResult<Vec<Expr>> {
        let mut err = None;
        let mut leaves: BTreeMap<Leaf, Expr> = BTreeMap::new();
        for leaf in &c.leaves {
            let r: BResult<Expr> = match leaf {
                Leaf::Prop(n) => {
                    if def.params.iter().any(|p| &p.name == n) {
                        self.var(e.id, n).map(Expr::Var)
                    } else {
                        param_value(self.ctx, e, n).map(Expr::c).ok_or_else(|| format!("{}.{n} has no value", e.id))
                    }
                }
                Leaf::PropPoint(n, a) => {
                    self.var(e.id, &format!("{n}.{}", if *a == Axis::X { "x" } else { "y" })).map(Expr::Var)
                }
                Leaf::RefAnchor(prop, anchor, a) => match e.props.get(prop) {
                    Some(PropValue::Ref(r)) => {
                        let target = r.entity;
                        self.anchor_expr_depth(target, anchor, depth + 1).and_then(|w| {
                            let inv = e.transform.inverse().map_err(|x| x.to_string())?;
                            let l = w.transformed(inv.m);
                            Ok(if *a == Axis::X { l.x } else { l.y })
                        })
                    }
                    _ => Err(format!("{}.{prop} is not a reference", e.id)),
                },
                Leaf::RefParam(prop, name) => match e.props.get(prop) {
                    Some(PropValue::Ref(r)) => self.var(r.entity, name).map(Expr::Var),
                    _ => Err(format!("{}.{prop} is not a reference", e.id)),
                },
                Leaf::AxisScale => self
                    .ctx
                    .view
                    .settings()
                    .time_axis
                    .map(|a| Expr::c(a.mm_per_second))
                    .ok_or_else(|| "document has no time axis".into()),
                Leaf::AxisOrigin => self
                    .ctx
                    .view
                    .settings()
                    .time_axis
                    .map(|a| Expr::c(a.origin_s))
                    .ok_or_else(|| "document has no time axis".into()),
            };
            match r {
                Ok(x) => {
                    leaves.insert(leaf.clone(), x);
                }
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = err {
            return Err(e);
        }
        c.instantiate(&mut |l| leaves.get(l).cloned()).ok_or_else(|| "expression input missing".into())
    }

    /// Circle center (world) and radius (world).
    pub fn circle_expr(&mut self, id: EntityId) -> BResult<(PointExpr, Expr)> {
        let e = self.entity(id)?;
        let scale = match e.transform.linear_kind(1e-9) {
            LinearKind::Similarity { scale, .. } => scale,
            _ => return Err(format!("{id} has a non-uniform transform; circle rules are not supported")),
        };
        if is_builtin(&e.type_id) {
            if !matches!(e.type_id.as_str(), types::CIRCLE | types::ARC) {
                return Err(format!("{id} is not a circle or arc"));
            }
            let c = self.pv(id, "cx", "cy")?.transformed(e.transform.m);
            let r = Expr::mul(Expr::c(scale), Expr::Var(self.var(id, "r")?));
            return Ok((c, r));
        }
        let def = plugin_for(self.ctx.registry, e).map_err(|r| format!("{id} is read-only: {r:?}"))?;
        let (cc, rc) = def.circle.as_ref().ok_or_else(|| format!("type {} has no circle interpretation", e.type_id))?;
        let c = self.instantiate(e, def, cc, 0)?;
        let r = self.instantiate(e, def, rc, 0)?;
        match (c.as_slice(), r.as_slice()) {
            ([x, y], [r]) => Ok((
                PointExpr::new(x.clone(), y.clone()).transformed(e.transform.m),
                Expr::mul(Expr::c(scale), r.clone()),
            )),
            _ => Err("circle definition has wrong types".into()),
        }
    }

    fn line(&mut self, l: &LineRef) -> BResult<(PointExpr, PointExpr)> {
        Ok((self.anchor_expr(l.from.entity, &l.from.anchor)?, self.anchor_expr(l.to.entity, &l.to.anchor)?))
    }

    fn push(&mut self, mut rule: Rule, origin: RuleOrigin) {
        if rule.id & SYNTHETIC != 0 && matches!(origin, RuleOrigin::Edit { .. } | RuleOrigin::Drag) {
            rule.id = SYNTHETIC | (1 << 62) | self.next_synthetic;
            self.next_synthetic += 1;
        }
        self.problem.rules.push(rule);
        self.origins.push(origin);
    }

    /// Compile a document constraint (disabled constraints are skipped).
    pub fn add_constraint(&mut self, c: &Constraint) {
        if !c.enabled {
            return;
        }
        let len = self.scales.length;
        let rows = self.rule_rows(&c.rule, len);
        let mut rule = match rows {
            Ok(rows) => Rule::new(c.id.0, rows, strength(c.strength)),
            Err(why) => {
                let mut r = Rule::new(c.id.0, Vec::new(), strength(c.strength));
                r.unsupported = Some(why);
                r
            }
        };
        rule.label = c.label.clone().unwrap_or_else(|| c.rule.kind_name().to_owned());
        rule.source = c.source.clone().unwrap_or_else(|| "user".into());
        rule.entities = c.rule.entities().into_iter().map(|e| e.0).collect();
        self.push(rule, RuleOrigin::Constraint(c.id));
    }

    fn param_scale(&self, p: &ParamRef) -> f64 {
        let name = match &p.slot {
            ParamSlot::Geom(n) | ParamSlot::Prop(n) => n,
        };
        self.ctx.view.entity(p.entity).and_then(|e| self.param_info(e, name)).map_or(1.0, |(d, _, _)| self.scales.of(d))
    }

    fn rule_rows(&mut self, r: &RuleSpec, len: f64) -> BResult<Vec<dotloom_constraints::Row>> {
        Ok(match r {
            RuleSpec::Fix { param, value } => rules::fix(self.param_expr(param)?, *value, self.param_scale(param)),
            RuleSpec::Equal { a, b } => rules::equal(self.param_expr(a)?, self.param_expr(b)?, self.param_scale(a)),
            RuleSpec::AllEqual { params } => {
                let s = params.first().map_or(1.0, |p| self.param_scale(p));
                let e = params.iter().map(|p| self.param_expr(p)).collect::<BResult<Vec<_>>>()?;
                rules::all_equal(&e, s)
            }
            RuleSpec::Linear { terms, op, rhs } => {
                let s = terms.first().map_or(1.0, |t| self.param_scale(&t.param));
                let t = terms.iter().map(|t| Ok((t.coef, self.param_expr(&t.param)?))).collect::<BResult<Vec<_>>>()?;
                rules::linear(&t, cmp(*op), *rhs, s)
            }
            RuleSpec::Ratio { a, b, k } => {
                rules::ratio(self.param_expr(a)?, self.param_expr(b)?, *k, self.param_scale(a))
            }
            RuleSpec::EqualSpacing { params } => {
                let s = params.first().map_or(1.0, |p| self.param_scale(p));
                let e = params.iter().map(|p| self.param_expr(p)).collect::<BResult<Vec<_>>>()?;
                rules::equal_spacing(&e, s)
            }
            RuleSpec::Coincident { a, b } => {
                rules::coincident(&self.anchor_expr(a.entity, &a.anchor)?, &self.anchor_expr(b.entity, &b.anchor)?, len)
            }
            RuleSpec::Horizontal { a, b } => {
                rules::horizontal(&self.anchor_expr(a.entity, &a.anchor)?, &self.anchor_expr(b.entity, &b.anchor)?, len)
            }
            RuleSpec::Vertical { a, b } => {
                rules::vertical(&self.anchor_expr(a.entity, &a.anchor)?, &self.anchor_expr(b.entity, &b.anchor)?, len)
            }
            RuleSpec::FixPoint { a, at } => {
                rules::fix_point(&self.anchor_expr(a.entity, &a.anchor)?, (at.x, at.y), len)
            }
            RuleSpec::Distance { a, b, value } => rules::distance(
                &self.anchor_expr(a.entity, &a.anchor)?,
                &self.anchor_expr(b.entity, &b.anchor)?,
                *value,
                len,
            ),
            RuleSpec::PointLineDistance { point, line, value } => {
                let p = self.anchor_expr(point.entity, &point.anchor)?;
                let (a, b) = self.line(line)?;
                rules::point_line_distance(&p, &a, &b, *value, len)
            }
            RuleSpec::PointOnLine { point, line } => {
                let p = self.anchor_expr(point.entity, &point.anchor)?;
                let (a, b) = self.line(line)?;
                rules::point_on_line(&p, &a, &b, len)
            }
            RuleSpec::PointOnCircle { point, circle } => {
                let p = self.anchor_expr(point.entity, &point.anchor)?;
                let (c, r) = self.circle_expr(*circle)?;
                rules::point_on_circle(&p, &c, r, len)
            }
            RuleSpec::Length { line, value } => {
                let (a, b) = self.line(line)?;
                rules::length(&a, &b, *value, len)
            }
            RuleSpec::EqualLength { a, b } => {
                let ((a1, b1), (a2, b2)) = (self.line(a)?, self.line(b)?);
                rules::equal_length(&a1, &b1, &a2, &b2, len)
            }
            RuleSpec::Parallel { a, b } => {
                let ((a1, b1), (a2, b2)) = (self.line(a)?, self.line(b)?);
                rules::parallel(&a1, &b1, &a2, &b2)
            }
            RuleSpec::Perpendicular { a, b } => {
                let ((a1, b1), (a2, b2)) = (self.line(a)?, self.line(b)?);
                rules::perpendicular(&a1, &b1, &a2, &b2)
            }
            RuleSpec::Angle { a, b, value } => {
                let ((a1, b1), (a2, b2)) = (self.line(a)?, self.line(b)?);
                rules::angle(&a1, &b1, &a2, &b2, *value)
            }
            RuleSpec::Concentric { a, b } => {
                let ((c1, _), (c2, _)) = (self.circle_expr(*a)?, self.circle_expr(*b)?);
                rules::concentric(&c1, &c2, len)
            }
            RuleSpec::Radius { circle, value } => {
                let (_, r) = self.circle_expr(*circle)?;
                rules::radius(r, *value, len)
            }
            RuleSpec::EqualRadius { a, b } => {
                let ((_, r1), (_, r2)) = (self.circle_expr(*a)?, self.circle_expr(*b)?);
                rules::equal_radius(r1, r2, len)
            }
            RuleSpec::TangentLineCircle { line, circle, side } => {
                let (a, b) = self.line(line)?;
                let (c, r) = self.circle_expr(*circle)?;
                rules::tangent_line_circle(&a, &b, &c, r, *side, len)
            }
            RuleSpec::TangentCircles { a, b, internal, sign } => {
                let ((c1, r1), (c2, r2)) = (self.circle_expr(*a)?, self.circle_expr(*b)?);
                let kind = if *internal { CircleTangency::Internal { sign: *sign } } else { CircleTangency::External };
                rules::tangent_circles(&c1, r1, &c2, r2, kind, len)
            }
            RuleSpec::Expression { entity, lhs, op, rhs } => {
                let e = self.entity(*entity)?;
                let def = plugin_for(self.ctx.registry, e).map_err(|r| format!("{entity} is read-only: {r:?}"))?;
                let diff = compile(&format!("({lhs}) - ({rhs})"), &def.scope).map_err(|x| x.to_string())?;
                let crate::lang::Ty::Scalar(dim) = diff.ty else {
                    return Err("expression rule sides must be scalars".into());
                };
                let parts = self.instantiate(e, def, &diff, 0)?;
                let d = parts.into_iter().next().ok_or("empty expression")?;
                rules::linear(&[(1.0, d)], cmp(*op), 0.0, self.scales.of(dim))
            }
        })
    }

    /// Instantiate the templates of a plugin entity.
    pub fn add_templates(&mut self, id: EntityId) {
        let Some(e) = self.ctx.view.entity(id) else { return };
        let Ok(def) = plugin_for(self.ctx.registry, e) else { return };
        for (i, t) in def.templates.iter().enumerate() {
            let rid = SYNTHETIC | (id.0 << 8) | (i as u64 & 0xff);
            let mut rule = match self.instantiate(e, def, &t.diff, 0) {
                Ok(parts) => {
                    let d = parts.into_iter().next().unwrap_or(Expr::c(f64::NAN));
                    Rule::new(
                        rid,
                        rules::linear(&[(1.0, d)], cmp(t.def.op), 0.0, self.scales.of(t.dim)),
                        strength(t.def.strength),
                    )
                }
                Err(why) => {
                    let mut r = Rule::new(rid, Vec::new(), strength(t.def.strength));
                    r.unsupported = Some(why);
                    r
                }
            };
            rule.label = t.def.label.clone().unwrap_or_else(|| format!("{} {:?} {}", t.def.lhs, t.def.op, t.def.rhs));
            rule.source = format!("plugin:{}", e.type_id);
            rule.entities = vec![id.0];
            self.push(rule, RuleOrigin::Template { entity: id, index: i });
        }
    }

    /// Exact edit (hard) or preferred value (strong target) for a parameter.
    pub fn add_edit(&mut self, id: EntityId, param: &str, value: f64, exact: bool) -> BResult<()> {
        let v = self.var(id, param)?;
        if exact {
            let scale = self.problem.vars.get(v.index()).map_or(1.0, |x| x.scale);
            let mut rule = Rule::new(SYNTHETIC, rules::fix(Expr::Var(v), value, scale), Strength::Required);
            rule.label = format!("{id}.{param} = {value}");
            rule.source = "edit".into();
            rule.entities = vec![id.0];
            self.push(rule, RuleOrigin::Edit { entity: id, param: param.to_owned() });
        } else {
            self.problem.targets.push(Target { var: v, value, strength: Strength::Strong });
        }
        Ok(())
    }

    /// Drag an anchor towards a world point (strong preference).
    pub fn add_anchor_target(&mut self, id: EntityId, anchor: &str, to: dotloom_geometry::Point) -> BResult<()> {
        let p = self.anchor_expr(id, anchor)?;
        // The dragged anchor's own parameters are never pinned.
        let e = self.entity(id)?;
        let locked = e.locked;
        for v in p.x.vars().into_iter().chain(p.y.vars()) {
            if let Some(var) = self.problem.vars.get_mut(v.index())
                && var.label.starts_with(&format!("{id}."))
                && !locked
            {
                var.fixed = false;
            }
        }
        let mut rule = Rule::new(SYNTHETIC, rules::fix_point(&p, (to.x, to.y), self.scales.length), Strength::Strong);
        rule.label = format!("drag {id}.{anchor}");
        rule.source = "drag".into();
        rule.entities = vec![id.0];
        self.push(rule, RuleOrigin::Drag);
        Ok(())
    }

    /// Ensure every solver parameter of an entity has a variable (so stays apply).
    pub fn add_entity_params(&mut self, id: EntityId) {
        let Some(e) = self.ctx.view.entity(id) else { return };
        let names: Vec<String> = if is_builtin(&e.type_id) {
            e.geometry.as_ref().map(|g| geometry_params(g).into_iter().map(|(n, _, _)| n).collect()).unwrap_or_default()
        } else {
            plugin_for(self.ctx.registry, e)
                .map(|d| d.params.iter().map(|p| p.name.clone()).collect())
                .unwrap_or_default()
        };
        for n in names {
            let _ = self.var(id, &n);
        }
    }
}
