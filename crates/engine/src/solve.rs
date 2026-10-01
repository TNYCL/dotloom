//! Solving the affected part of a transaction and writing results back.

use std::collections::{BTreeMap, BTreeSet};

use dotloom_constraints::{Problem, Solution, SolveOptions, VarId};
use dotloom_document::{ConstraintId, Entity, EntityId, PropValue, builtin::is_builtin, builtin::set_geometry_param};
use dotloom_geometry::Aabb;

use crate::{
    DocView, Overlay,
    build::{Builder, RuleOrigin, Scales},
    check::Checker,
    command::ApplyNotes,
    error::{DiagnosticReport, EngineError, NearestValue, SolveFailure},
    eval::{Ctx, evaluate},
    index::DepIndex,
    registry::Registry,
};

/// Entities and constraints affected by a change.
#[derive(Debug, Clone, Default)]
pub struct Closure {
    /// Entities.
    pub entities: BTreeSet<EntityId>,
    /// Enabled constraints.
    pub constraints: BTreeSet<ConstraintId>,
    /// Whether any plugin entity with templates is involved.
    pub has_templates: bool,
}

/// Compute the dependency closure of `seeds` in the overlay.
pub(crate) fn closure(
    ov: &Overlay<'_>,
    deps: &DepIndex,
    registry: &Registry,
    seeds: impl IntoIterator<Item = EntityId>,
) -> Closure {
    let mut out = Closure::default();
    // Constraints and referencing entities that exist only in the overlay.
    let mut ov_constraints: BTreeMap<EntityId, Vec<ConstraintId>> = BTreeMap::new();
    for (cid, c) in &ov.constraints {
        if let Some(c) = c {
            for e in c.rule.entities().into_iter().chain(c.owner) {
                ov_constraints.entry(e).or_default().push(*cid);
            }
        }
    }
    let mut ov_referrers: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
    for (id, e) in &ov.entities {
        if let Some(e) = e {
            for t in e.referenced_entities() {
                ov_referrers.entry(t).or_default().push(*id);
            }
        }
    }
    let mut queue: Vec<EntityId> = seeds.into_iter().collect();
    while let Some(id) = queue.pop() {
        let Some(e) = ov.entity(id) else { continue };
        if !out.entities.insert(id) {
            continue;
        }
        if !e.type_id.namespace().eq("dotloom")
            && let Some(t) = registry.enabled(&e.type_id)
            && !t.templates.is_empty()
        {
            out.has_templates = true;
        }
        let cids: Vec<ConstraintId> =
            deps.constraints_of(id).chain(ov_constraints.get(&id).into_iter().flatten().copied()).collect();
        for cid in cids {
            let Some(c) = ov.constraint(cid) else { continue };
            if !c.enabled || out.constraints.contains(&cid) {
                continue;
            }
            let ents = c.rule.entities();
            if !ents.contains(&id) && c.owner != Some(id) {
                continue;
            }
            out.constraints.insert(cid);
            queue.extend(ents);
        }
        queue.extend(e.referenced_entities());
        let refs: Vec<EntityId> =
            deps.referrers_of(id).chain(ov_referrers.get(&id).into_iter().flatten().copied()).collect();
        for r in refs {
            if ov.entity(r).is_some_and(|x| x.referenced_entities().any(|t| t == id)) {
                queue.push(r);
            }
        }
    }
    out
}

/// A prepared solve.
#[derive(Debug)]
pub struct Plan {
    /// Variable map.
    pub vars: BTreeMap<(EntityId, String), VarId>,
    /// Rule origins.
    pub origins: Vec<RuleOrigin>,
    /// Scales.
    pub scales: Scales,
    /// Closure.
    pub closure: Closure,
    /// Edits `(entity, param, value, exact)`.
    pub edits: Vec<(EntityId, String, f64, bool)>,
}

/// Characteristic scales of a set of entities.
pub(crate) fn scales_for(ctx: Ctx<'_>, ids: &BTreeSet<EntityId>) -> Scales {
    let bb = ids.iter().fold(Aabb::EMPTY, |b, id| b.union(evaluate(ctx, *id).bbox));
    let diag = bb.size().length();
    Scales { length: if diag.is_finite() && diag > 1.0 { diag } else { 1000.0 }, time: 3600.0 }
}

/// Build the solver problem for a transaction. Returns `None` when nothing needs
/// solving (no rules involved and only exact edits).
pub(crate) fn plan(
    ov: &Overlay<'_>,
    deps: &DepIndex,
    registry: &Registry,
    notes: &ApplyNotes,
    prefer_all: bool,
    pin: bool,
) -> Option<(Problem, Plan)> {
    plan_with(ov, deps, registry, notes, prefer_all, None, pin)
}

/// [`plan`] with an optional anchor drag target `(entity, anchor, world point)`.
pub(crate) fn plan_with(
    ov: &Overlay<'_>,
    deps: &DepIndex,
    registry: &Registry,
    notes: &ApplyNotes,
    prefer_all: bool,
    anchor_target: Option<(EntityId, &str, dotloom_geometry::Point)>,
    pin: bool,
) -> Option<(Problem, Plan)> {
    let mut seeds: BTreeSet<EntityId> = notes.touched.iter().copied().filter(|id| ov.entity(*id).is_some()).collect();
    for cid in &notes.touched_constraints {
        if let Some(c) = ov.constraint(*cid) {
            seeds.extend(c.rule.entities());
        }
    }
    seeds.extend(notes.edits.iter().map(|e| e.0));
    seeds.extend(anchor_target.map(|a| a.0));
    let cl = closure(ov, deps, registry, seeds);
    let has_prefer = prefer_all || notes.edits.iter().any(|e| !e.3) || anchor_target.is_some();
    if cl.constraints.is_empty() && !cl.has_templates && !has_prefer {
        return None;
    }
    let ctx = Ctx { view: ov, registry };
    let scales = scales_for(ctx, &cl.entities);
    let mut b = Builder::new(ctx, scales);
    b.edited = notes.edits.iter().map(|e| e.0).chain(anchor_target.map(|a| a.0)).collect();
    b.edited_params = notes.edits.iter().map(|e| (e.0, e.1.clone())).collect();
    b.pin = pin;
    for id in &cl.entities {
        b.add_entity_params(*id);
    }
    for cid in &cl.constraints {
        if let Some(c) = ov.constraint(*cid) {
            b.add_constraint(c);
        }
    }
    for id in &cl.entities {
        b.add_templates(*id);
    }
    // Deduplicate edits (last wins) and add them after structural rules.
    let mut edits: BTreeMap<(EntityId, String), (f64, bool)> = BTreeMap::new();
    for (e, p, v, exact) in &notes.edits {
        edits.insert((*e, p.clone()), (*v, *exact && !prefer_all));
    }
    let mut list = Vec::new();
    for ((e, p), (v, exact)) in edits {
        if cl.entities.contains(&e) && b.add_edit(e, &p, v, exact).is_ok() {
            list.push((e, p, v, exact));
        }
    }
    if let Some((e, a, p)) = anchor_target {
        b.add_anchor_target(e, a, p).ok()?;
    }
    let problem = core::mem::take(&mut b.problem);
    Some((problem, Plan { vars: b.vars, origins: b.origins, scales, closure: cl, edits: list }))
}

/// Write a parameter value into an entity.
pub(crate) fn set_param(e: &mut Entity, name: &str, v: f64) -> bool {
    if is_builtin(&e.type_id) {
        return e.geometry.as_mut().is_some_and(|g| set_geometry_param(g, name, v).is_ok());
    }
    if let Some((base, axis)) = name.rsplit_once('.')
        && let Some(PropValue::Point(p)) = e.props.get_mut(base)
    {
        match axis {
            "x" => p.x = v,
            "y" => p.y = v,
            _ => return false,
        }
        return true;
    }
    match e.props.get_mut(name) {
        Some(PropValue::Number(n)) => {
            *n = v;
            true
        }
        None => {
            e.props.insert(name.to_owned(), PropValue::Number(v));
            true
        }
        _ => false,
    }
}

/// Whether every soft target (preferred edits, drag rules) is met by `sol`.
pub(crate) fn targets_met(problem: &Problem, plan: &Plan, sol: &Solution) -> bool {
    for t in &problem.targets {
        let Some(v) = sol.values.get(t.var.index()) else { return false };
        let scale = problem.vars.get(t.var.index()).map_or(1.0, |x| x.scale);
        if (v - t.value).abs() > 1e-6 * scale {
            return false;
        }
    }
    for (rule, origin) in problem.rules.iter().zip(&plan.origins) {
        if matches!(origin, RuleOrigin::Drag) {
            for row in &rule.rows {
                if row.expr.eval(&sol.values).abs() > 1e-6 * row.scale {
                    return false;
                }
            }
        }
    }
    true
}

/// Map solver diagnostics back to document objects.
pub(crate) fn reports(problem: &Problem, plan: &Plan, sol: &Solution) -> Vec<DiagnosticReport> {
    let by_id: BTreeMap<u64, usize> = problem.rules.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    sol.diagnostics
        .iter()
        .map(|d| {
            let mut rep = DiagnosticReport {
                kind: d.kind,
                certainty: d.certainty,
                constraints: Vec::new(),
                templates: Vec::new(),
                edits: Vec::new(),
                entities: d.entities.iter().map(|e| EntityId(*e)).collect(),
                labels: d.labels.clone(),
                residual: d.residual,
                message: d.message.clone(),
            };
            for (rid, label) in d.rules.iter().zip(d.labels.iter().chain(core::iter::repeat(&String::new()))) {
                match by_id.get(rid).and_then(|i| plan.origins.get(*i)) {
                    Some(RuleOrigin::Constraint(c)) => rep.constraints.push(*c),
                    Some(RuleOrigin::Template { entity, .. }) => rep.templates.push((*entity, label.clone())),
                    Some(RuleOrigin::Edit { entity, param }) => rep.edits.push((*entity, param.clone())),
                    Some(RuleOrigin::Drag) | None => {}
                }
            }
            rep
        })
        .collect()
}

/// Apply an accepted solution to the overlay and run the independent check.
pub(crate) fn finish(
    ov: &mut Overlay<'_>,
    registry: &Registry,
    problem: &Problem,
    plan: &Plan,
    sol: &Solution,
    notes: &mut ApplyNotes,
) -> Result<(), EngineError> {
    if !sol.accepted() {
        return Err(EngineError::Solve {
            failure: SolveFailure { status: sol.status, diagnostics: reports(problem, plan, sol), nearest: Vec::new() },
        });
    }
    let mut per_entity: BTreeMap<EntityId, Vec<(&str, f64)>> = BTreeMap::new();
    for ((e, p), v) in &plan.vars {
        let Some(var) = problem.vars.get(v.index()) else { continue };
        if var.fixed {
            continue;
        }
        let Some(val) = sol.values.get(v.index()).copied() else { continue };
        if val != var.value || plan.edits.iter().any(|x| x.0 == *e && x.1 == *p) {
            per_entity.entry(*e).or_default().push((p.as_str(), val));
        }
    }
    for (id, vals) in per_entity {
        let Some(e) = ov.entity_mut(id) else { continue };
        for (p, v) in vals {
            set_param(e, p, v);
        }
        notes.touched.insert(id);
    }
    // Geometry produced by the solver must still be valid (positive radii, ...).
    for id in &plan.closure.entities {
        if let Some(e) = ov.entity(*id)
            && let Some(g) = &e.geometry
            && let Err(err) = g.validate()
        {
            return Err(EngineError::Invalid { message: format!("{id}: {err}") });
        }
    }
    let ctx = Ctx { view: &*ov, registry };
    let mut checker = Checker::new(ctx, plan.scales);
    for cid in &plan.closure.constraints {
        let Some(c) = ov.constraint(*cid) else { continue };
        if c.strength != dotloom_document::StrengthSpec::Required || !c.enabled {
            continue;
        }
        let (residual, tolerance) =
            checker.constraint(c).map_err(|m| EngineError::Invalid { message: format!("{cid}: {m}") })?;
        if residual.is_nan() || residual > tolerance {
            return Err(EngineError::Validation { what: cid.to_string(), residual, tolerance });
        }
    }
    for id in &plan.closure.entities {
        for (label, residual, tolerance) in checker.templates(*id) {
            if residual.is_nan() || residual > tolerance {
                return Err(EngineError::Validation { what: format!("{id} {label}"), residual, tolerance });
            }
        }
    }
    Ok(())
}

/// For a failed solve with exact edits, compute the nearest feasible values by
/// demoting the edits to strong preferences.
pub(crate) fn nearest(
    ov: &Overlay<'_>,
    deps: &DepIndex,
    registry: &Registry,
    notes: &ApplyNotes,
    opts: &SolveOptions,
) -> Vec<NearestValue> {
    let Some((problem, plan)) = plan(ov, deps, registry, notes, true, false) else { return Vec::new() };
    let sol = dotloom_constraints::solve(&problem, opts);
    if !sol.accepted() {
        return Vec::new();
    }
    notes
        .edits
        .iter()
        .filter(|(_, _, _, exact)| *exact)
        .filter_map(|(e, p, requested, _)| {
            let v = plan.vars.get(&(*e, p.clone()))?;
            let feasible = sol.values.get(v.index()).copied()?;
            ((feasible - requested).abs() > plan.scales.length * 1e-9).then(|| NearestValue {
                entity: *e,
                param: p.clone(),
                requested: *requested,
                feasible,
            })
        })
        .collect()
}
