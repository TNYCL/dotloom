//! Solver behaviour tests (DL-SOLVE, DL-TEST-2, DL-TEST-3).
//!
//! Final geometry is validated with independent closed-form checks, never only with
//! the solver's own residual report.

// Independent reference values use the platform math functions on purpose.
#![allow(clippy::disallowed_methods)]

use dotloom_constraints::{
    Backend, Certainty, DiagnosticKind, Expr, PointExpr, Problem, Progress, Rule, SolveJob, SolveOptions, Status,
    Strength, Target, VarId, Variable,
    rules::{self, Cmp},
    solve,
};

const L: f64 = 1000.0; // characteristic length (mm)

fn v(id: VarId) -> Expr {
    Expr::Var(id)
}

fn var(p: &mut Problem, value: f64) -> VarId {
    p.add_var(Variable::new(value).scale(L))
}

fn point(p: &mut Problem, x: f64, y: f64) -> (VarId, VarId, PointExpr) {
    let a = var(p, x);
    let b = var(p, y);
    (a, b, PointExpr::vars(a, b))
}

fn at(s: &[f64], id: VarId) -> f64 {
    s[id.index()]
}

fn rule(p: &mut Problem, id: u64, rows: Vec<dotloom_constraints::Row>) {
    p.rules.push(Rule::new(id, rows, Strength::Required).label(format!("rule {id}")));
}

// ---------------------------------------------------------------------------
// Shelf acceptance scenario (linear component, kasuari backend)

struct Shelf {
    p: Problem,
    total: VarId,
    w: [VarId; 3],
}

fn shelf(total: f64, locked: bool) -> Shelf {
    let mut p = Problem::default();
    let t = var(&mut p, 1800.0);
    let w1 = var(&mut p, 600.0);
    let w2 = var(&mut p, 600.0);
    let w3 = var(&mut p, 600.0);
    p.rules.push(
        Rule::new(
            1,
            rules::linear(&[(1.0, v(w1)), (1.0, v(w2)), (1.0, v(w3)), (-1.0, v(t))], Cmp::Eq, 0.0, L),
            Strength::Required,
        )
        .label("sum of compartments = inner width"),
    );
    p.rules.push(Rule::new(2, rules::equal(v(w2), v(w3), L), Strength::Required).label("w2 = w3"));
    p.rules.push(Rule::new(3, rules::at_least(v(w2), 400.0, L), Strength::Required).label("w2 ≥ 40 cm"));
    p.rules.push(Rule::new(4, rules::at_least(v(w3), 400.0, L), Strength::Required).label("w3 ≥ 40 cm"));
    if locked {
        p.rules.push(
            Rule::new(5, rules::fix(v(w1), 600.0, L), Strength::Required).label("w1 locked 60 cm").source("user"),
        );
    }
    p.rules
        .push(Rule::new(6, rules::fix(v(t), total, L), Strength::Required).label("edit: inner width").source("edit"));
    Shelf { p, total: t, w: [w1, w2, w3] }
}

#[test]
fn shelf_160_gives_60_50_50() {
    let s = shelf(1600.0, true);
    let sol = solve(&s.p, &SolveOptions::default());
    assert_eq!(sol.status, Status::Solved, "{sol:?}");
    assert_eq!(sol.components[0].backend, Backend::Linear);
    let w: Vec<f64> = s.w.iter().map(|id| at(&sol.values, *id)).collect();
    // Independent checks.
    assert!((w[0] - 600.0).abs() < 1e-6);
    assert!((w[1] - 500.0).abs() < 1e-6 && (w[2] - 500.0).abs() < 1e-6);
    assert!((w.iter().sum::<f64>() - at(&sol.values, s.total)).abs() < 1e-6);
}

#[test]
fn shelf_130_is_rejected_with_certain_minimal_conflict() {
    let s = shelf(1300.0, true);
    let sol = solve(&s.p, &SolveOptions::default());
    assert_eq!(sol.status, Status::Conflicting);
    assert!(!sol.accepted());
    // Values are returned unchanged.
    assert_eq!(sol.values, s.p.values());
    let d = sol.diagnostics.iter().find(|d| d.kind == DiagnosticKind::Conflict).unwrap();
    assert_eq!(d.certainty, Certainty::Certain);
    // Minimal set: sum, both minimums, the lock and the edit (w2 = w3 is not needed).
    let mut ids = d.rules.clone();
    ids.sort_unstable();
    assert_eq!(ids, vec![1, 3, 4, 5, 6], "{d:?}");
}

#[test]
fn shelf_nearest_feasible_value_is_140() {
    // Demote the edit to a strong preference: the solver reports the closest
    // feasible inner width.
    let mut s = shelf(1300.0, true);
    s.p.rules.pop();
    s.p.targets.push(Target { var: s.total, value: 1300.0, strength: Strength::Strong });
    let sol = solve(&s.p, &SolveOptions::default());
    assert!(sol.accepted());
    assert!((at(&sol.values, s.total) - 1400.0).abs() < 1e-6, "{:?}", sol.values);
}

#[test]
fn shelf_unlocked_130_is_feasible_and_deterministic() {
    let s = shelf(1300.0, false);
    let first = solve(&s.p, &SolveOptions::default());
    assert!(first.accepted(), "{first:?}");
    let w: Vec<f64> = s.w.iter().map(|id| at(&first.values, *id)).collect();
    assert!((w.iter().sum::<f64>() - 1300.0).abs() < 1e-6);
    assert!((w[1] - w[2]).abs() < 1e-6 && w[1] >= 400.0 - 1e-6);
    // kasuari uses randomly seeded hash maps; stay weights break ties by stable order.
    for _ in 0..50 {
        let again = solve(&s.p, &SolveOptions::default());
        assert_eq!(again.values, first.values);
    }
}

// ---------------------------------------------------------------------------
// Geometric rules, each validated with an independent formula.

fn opts() -> SolveOptions {
    SolveOptions::default()
}

fn dist(s: &[f64], a: (VarId, VarId), b: (VarId, VarId)) -> f64 {
    (at(s, b.0) - at(s, a.0)).hypot(at(s, b.1) - at(s, a.1))
}

#[test]
fn distance_and_fixed_point() {
    let mut p = Problem::default();
    let (ax, ay, a) = point(&mut p, 10.0, 10.0);
    let (bx, by, b) = point(&mut p, 50.0, 40.0);
    rule(&mut p, 1, rules::fix_point(&a, (0.0, 0.0), L));
    rule(&mut p, 2, rules::distance(&a, &b, 100.0, L));
    let sol = solve(&p, &opts());
    assert!(matches!(sol.status, Status::Underconstrained { dof: 1 }), "{sol:?}");
    assert!(at(&sol.values, ax).abs() < 1e-9 && at(&sol.values, ay).abs() < 1e-9);
    assert!((dist(&sol.values, (ax, ay), (bx, by)) - 100.0).abs() < 1e-6);
    // Branch preservation: B stays in the same direction from A (minimum-norm step).
    let ang = at(&sol.values, by).atan2(at(&sol.values, bx));
    assert!((ang - 40f64.atan2(50.0)).abs() < 1e-3, "{ang}");
}

#[test]
fn horizontal_vertical_coincident() {
    let mut p = Problem::default();
    let (ax, ay, a) = point(&mut p, 0.0, 0.0);
    let (bx, by, b) = point(&mut p, 100.0, 7.0);
    let (cx, cy, c) = point(&mut p, 103.0, 90.0);
    let (dx, dy, d) = point(&mut p, 101.0, 95.0);
    rule(&mut p, 1, rules::fix_point(&a, (0.0, 0.0), L));
    rule(&mut p, 2, rules::horizontal(&a, &b, L));
    rule(&mut p, 3, rules::vertical(&b, &c, L));
    rule(&mut p, 4, rules::coincident(&c, &d, L));
    let s = solve(&p, &opts()).values;
    assert!((at(&s, ay) - at(&s, by)).abs() < 1e-9);
    assert!((at(&s, bx) - at(&s, cx)).abs() < 1e-9);
    assert!((at(&s, cx) - at(&s, dx)).abs() < 1e-9 && (at(&s, cy) - at(&s, dy)).abs() < 1e-9);
    assert!(at(&s, ax).abs() < 1e-9);
}

/// Line endpoints helper.
fn line(p: &mut Problem, x1: f64, y1: f64, x2: f64, y2: f64) -> ([VarId; 4], PointExpr, PointExpr) {
    let (a0, a1, a) = point(p, x1, y1);
    let (b0, b1, b) = point(p, x2, y2);
    ([a0, a1, b0, b1], a, b)
}

fn dir(s: &[f64], l: [VarId; 4]) -> (f64, f64) {
    (at(s, l[2]) - at(s, l[0]), at(s, l[3]) - at(s, l[1]))
}

#[test]
fn parallel_perpendicular_angle_equal_length() {
    let mut p = Problem::default();
    let (l1, a1, b1) = line(&mut p, 0.0, 0.0, 100.0, 0.0);
    let (l2, a2, b2) = line(&mut p, 0.0, 50.0, 90.0, 60.0);
    let (l3, a3, b3) = line(&mut p, 0.0, 0.0, 10.0, 80.0);
    let (l4, a4, b4) = line(&mut p, 200.0, 0.0, 260.0, 30.0);
    rule(&mut p, 1, rules::fix_point(&a1, (0.0, 0.0), L));
    rule(&mut p, 2, rules::fix_point(&b1, (100.0, 0.0), L));
    rule(&mut p, 3, rules::parallel(&a1, &b1, &a2, &b2));
    rule(&mut p, 4, rules::perpendicular(&a1, &b1, &a3, &b3));
    rule(&mut p, 5, rules::angle(&a1, &b1, &a4, &b4, 30f64.to_radians()));
    rule(&mut p, 6, rules::equal_length(&a1, &b1, &a4, &b4, L));
    let sol = solve(&p, &opts());
    assert!(sol.accepted(), "{sol:?}");
    let s = &sol.values;
    let (u1, u2, u3, u4) = (dir(s, l1), dir(s, l2), dir(s, l3), dir(s, l4));
    let cross = |a: (f64, f64), b: (f64, f64)| a.0 * b.1 - a.1 * b.0;
    let dot = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1;
    let len = |a: (f64, f64)| a.0.hypot(a.1);
    assert!(cross(u1, u2).abs() / (len(u1) * len(u2)) < 1e-9);
    assert!(dot(u1, u3).abs() / (len(u1) * len(u3)) < 1e-9);
    let ang = cross(u1, u4).atan2(dot(u1, u4));
    assert!((ang - 30f64.to_radians()).abs() < 1e-9, "{}", ang.to_degrees());
    assert!((len(u4) - 100.0).abs() < 1e-6);
}

#[test]
fn circles_concentric_radius_tangency() {
    let mut p = Problem::default();
    let (c1x, c1y, c1) = point(&mut p, 0.0, 0.0);
    let r1 = var(&mut p, 40.0);
    let (c2x, c2y, c2) = point(&mut p, 5.0, 3.0);
    let r2 = var(&mut p, 10.0);
    let (c3x, c3y, c3) = point(&mut p, 100.0, 20.0);
    let r3 = var(&mut p, 25.0);
    let (ln, la, lb) = line(&mut p, -100.0, -70.0, 100.0, -60.0);
    rule(&mut p, 1, rules::fix_point(&c1, (0.0, 0.0), L));
    rule(&mut p, 2, rules::radius(v(r1), 50.0, L));
    rule(&mut p, 3, rules::concentric(&c1, &c2, L));
    rule(&mut p, 4, rules::equal_radius(v(r2), v(r3), L));
    rule(&mut p, 5, rules::tangent_circles(&c1, v(r1), &c3, v(r3), rules::CircleTangency::External, L));
    rule(&mut p, 6, rules::tangent_line_circle(&la, &lb, &c1, v(r1), 1.0, L));
    let sol = solve(&p, &opts());
    assert!(sol.accepted(), "{sol:?}");
    let s = &sol.values;
    assert!((at(s, r1) - 50.0).abs() < 1e-9);
    assert!((at(s, c2x) - at(s, c1x)).abs() < 1e-9 && (at(s, c2y) - at(s, c1y)).abs() < 1e-9);
    assert!((at(s, r2) - at(s, r3)).abs() < 1e-9);
    let d13 = (at(s, c3x) - at(s, c1x)).hypot(at(s, c3y) - at(s, c1y));
    assert!((d13 - (at(s, r1) + at(s, r3))).abs() < 1e-6);
    // Independent line–circle distance.
    let (ax, ay, bx, by) = (at(s, ln[0]), at(s, ln[1]), at(s, ln[2]), at(s, ln[3]));
    let cross = (bx - ax) * (at(s, c1y) - ay) - (by - ay) * (at(s, c1x) - ax);
    let dline = cross / (bx - ax).hypot(by - ay);
    assert!((dline - 50.0).abs() < 1e-6, "{dline}");
}

#[test]
fn point_on_line_and_circle_and_point_line_distance() {
    let mut p = Problem::default();
    let (_, a, b) = line(&mut p, 0.0, 0.0, 100.0, 0.0);
    rule(&mut p, 1, rules::fix_point(&a, (0.0, 0.0), L));
    rule(&mut p, 2, rules::fix_point(&b, (100.0, 0.0), L));
    let (px, py, pp) = point(&mut p, 30.0, 12.0);
    rule(&mut p, 3, rules::point_on_line(&pp, &a, &b, L));
    let (qx, qy, q) = point(&mut p, 20.0, 35.0);
    rule(&mut p, 4, rules::point_on_circle(&q, &a, Expr::c(50.0), L));
    let (_, ry, r) = point(&mut p, 60.0, 5.0);
    rule(&mut p, 5, rules::point_line_distance(&r, &a, &b, 25.0, L));
    let s = solve(&p, &opts()).values;
    assert!(at(&s, py).abs() < 1e-9);
    assert!((at(&s, px) - 30.0).abs() < 1e-3, "stays near: {}", at(&s, px));
    assert!((at(&s, qx).hypot(at(&s, qy)) - 50.0).abs() < 1e-6);
    assert!((at(&s, ry) - 25.0).abs() < 1e-6);
}

#[test]
fn equal_spacing_ratio_and_bounds() {
    let mut p = Problem::default();
    let xs: Vec<VarId> = [0.0, 13.0, 41.0, 77.0].iter().map(|x| var(&mut p, *x)).collect();
    rule(&mut p, 1, rules::fix(v(xs[0]), 0.0, L));
    rule(&mut p, 2, rules::fix(v(xs[3]), 90.0, L));
    rule(&mut p, 3, rules::equal_spacing(&xs.iter().map(|x| v(*x)).collect::<Vec<_>>(), L));
    let a = var(&mut p, 10.0);
    let b = var(&mut p, 10.0);
    rule(&mut p, 4, rules::ratio(v(a), v(b), 2.5, L));
    rule(&mut p, 5, rules::at_most(v(a), 50.0, L));
    p.targets.push(Target { var: b, value: 100.0, strength: Strength::Strong });
    let sol = solve(&p, &opts());
    assert!(sol.accepted(), "{sol:?}");
    let s = &sol.values;
    assert!((at(s, xs[1]) - 30.0).abs() < 1e-9 && (at(s, xs[2]) - 60.0).abs() < 1e-9);
    // Hard max wins over the strong target: a = 50, b = 20.
    assert!((at(s, a) - 50.0).abs() < 1e-9 && (at(s, b) - 20.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// Hard vs soft in the numeric backend.

#[test]
fn drag_target_respects_hard_rules_numeric() {
    // Point constrained to a circle of radius 100; dragged towards (300, 0).
    let mut p = Problem::default();
    let (cx, cy, c) = point(&mut p, 0.0, 0.0);
    let (px, py, pp) = point(&mut p, 0.0, 100.0);
    rule(&mut p, 1, rules::fix_point(&c, (0.0, 0.0), L));
    rule(&mut p, 2, rules::distance(&c, &pp, 100.0, L));
    rule(&mut p, 3, rules::at_least(v(py), 20.0, L));
    p.targets.push(Target { var: px, value: 300.0, strength: Strength::Strong });
    p.targets.push(Target { var: py, value: -50.0, strength: Strength::Strong });
    let sol = solve(&p, &opts());
    assert!(sol.accepted(), "{sol:?}");
    let s = &sol.values;
    assert_eq!(sol.components.iter().find(|c| !c.vars.is_empty()).unwrap().backend, Backend::Numeric);
    let r = (at(s, px) - at(s, cx)).hypot(at(s, py) - at(s, cy));
    assert!((r - 100.0).abs() < 1e-6, "{r}");
    // Inequality active: y = 20 exactly, x = sqrt(100² − 20²).
    assert!((at(s, py) - 20.0).abs() < 1e-6, "{}", at(s, py));
    assert!((at(s, px) - (100f64.powi(2) - 400.0).sqrt()).abs() < 1e-5);
}

#[test]
fn soft_rules_only_apply_inside_hard_set() {
    let mut p = Problem::default();
    let a = var(&mut p, 0.0);
    let b = var(&mut p, 0.0);
    rule(&mut p, 1, rules::linear(&[(1.0, v(a)), (1.0, v(b))], Cmp::Eq, 10.0, L));
    p.rules.push(Rule::new(2, rules::fix(v(a), 8.0, L), Strength::Medium));
    p.rules.push(Rule::new(3, rules::fix(v(b), 8.0, L), Strength::Weak));
    let s = solve(&p, &opts()).values;
    assert!((at(&s, a) + at(&s, b) - 10.0).abs() < 1e-9);
    // Medium beats weak.
    assert!((at(&s, a) - 8.0).abs() < 1e-6, "{s:?}");
}

// ---------------------------------------------------------------------------
// Failure classification.

#[test]
fn constant_conflict_is_certain() {
    let mut p = Problem::default();
    let a = p.add_var(Variable::new(0.0).fixed(true));
    let b = p.add_var(Variable::new(30.0).fixed(true));
    rule(&mut p, 1, rules::equal(v(a), v(b), L));
    let sol = solve(&p, &opts());
    assert_eq!(sol.status, Status::Conflicting);
    assert_eq!(sol.diagnostics[0].certainty, Certainty::Certain);
}

#[test]
fn linear_conflict_inside_mixed_component_is_certain() {
    let mut p = Problem::default();
    let (_, _, a) = point(&mut p, 0.0, 0.0);
    let (bx, _, b) = point(&mut p, 10.0, 0.0);
    rule(&mut p, 1, rules::distance(&a, &b, 50.0, L));
    rule(&mut p, 2, rules::fix(v(bx), 20.0, L));
    rule(&mut p, 3, rules::fix(v(bx), 30.0, L));
    let sol = solve(&p, &opts());
    assert_eq!(sol.status, Status::Conflicting, "{sol:?}");
    let d = sol.diagnostics.iter().find(|d| d.kind == DiagnosticKind::Conflict).unwrap();
    assert!(d.rules.contains(&2) && d.rules.contains(&3));
}

#[test]
fn impossible_nonlinear_is_suspected_not_certain() {
    // Two points on a fixed circle of radius 10 must be 50 apart: impossible, but the
    // solver only has local evidence.
    let mut p = Problem::default();
    let (_, _, c) = point(&mut p, 0.0, 0.0);
    let (_, _, a) = point(&mut p, 10.0, 0.0);
    let (_, _, b) = point(&mut p, -10.0, 1.0);
    rule(&mut p, 1, rules::fix_point(&c, (0.0, 0.0), L));
    rule(&mut p, 2, rules::distance(&c, &a, 10.0, L));
    rule(&mut p, 3, rules::distance(&c, &b, 10.0, L));
    rule(&mut p, 4, rules::distance(&a, &b, 50.0, L));
    let sol = solve(&p, &opts());
    assert!(!sol.accepted());
    assert!(matches!(sol.status, Status::NotConverged { suspected_conflict: true }), "{:?}", sol.status);
    assert!(sol.diagnostics.iter().all(|d| d.certainty == Certainty::Suspected), "{:?}", sol.diagnostics);
    assert_eq!(sol.values, p.values());
}

#[test]
fn budget_exhaustion_keeps_input_values() {
    // No iteration allowed: the starting point violates the chain lengths.
    let p = chain(30);
    let o = SolveOptions { max_iterations: 0, ..SolveOptions::default() };
    let sol = solve(&p, &o);
    assert!(matches!(sol.status, Status::NotConverged { .. }), "{:?}", sol.status);
    assert_eq!(sol.values, p.values());
}

#[test]
fn redundant_consistent_rules_are_reported_not_rejected() {
    let mut p = Problem::default();
    let (_, _, a) = point(&mut p, 0.0, 0.0);
    let (_, _, b) = point(&mut p, 10.0, 5.0);
    rule(&mut p, 1, rules::fix_point(&a, (0.0, 0.0), L));
    rule(&mut p, 2, rules::horizontal(&a, &b, L));
    rule(&mut p, 3, rules::horizontal(&b, &a, L));
    rule(&mut p, 4, rules::distance(&a, &b, 40.0, L));
    let sol = solve(&p, &opts());
    assert!(sol.accepted(), "{sol:?}");
    assert!(sol.diagnostics.iter().any(|d| d.kind == DiagnosticKind::Redundant));
}

#[test]
fn unsupported_rule_reports_unsupported() {
    let mut p = Problem::default();
    let a = var(&mut p, 0.0);
    let mut r = Rule::new(1, rules::fix(v(a), 1.0, L), Strength::Required);
    r.unsupported = Some("bezier tangency is not supported".into());
    p.rules.push(r);
    let sol = solve(&p, &opts());
    assert_eq!(sol.status, Status::Unsupported);
    assert_eq!(sol.diagnostics[0].kind, DiagnosticKind::Unsupported);
}

#[test]
fn invalid_problem_is_rejected_without_panic() {
    let mut p = Problem::default();
    let a = var(&mut p, f64::NAN);
    rule(&mut p, 1, rules::fix(v(a), 1.0, L));
    assert_eq!(solve(&p, &opts()).status, Status::Unsupported);
    let mut q = Problem::default();
    rule(&mut q, 1, rules::fix(Expr::Var(VarId(99)), 1.0, L));
    assert_eq!(solve(&q, &opts()).status, Status::Unsupported);
}

// ---------------------------------------------------------------------------
// Budgeted stepping and cancellation.

fn chain(n: usize) -> Problem {
    // A chain of n points with fixed distances, pinned at both ends: nonlinear.
    let mut p = Problem::default();
    let pts: Vec<PointExpr> = (0..n).map(|i| point(&mut p, i as f64 * 9.0, (i % 3) as f64).2).collect();
    rule(&mut p, 1, rules::fix_point(&pts[0], (0.0, 0.0), L));
    rule(&mut p, 2, rules::fix_point(&pts[n - 1], ((n - 1) as f64 * 8.0, 0.0), L));
    for i in 0..n - 1 {
        rule(&mut p, 10 + i as u64, rules::distance(&pts[i], &pts[i + 1], 10.0, L));
    }
    p
}

/// Chain whose middle point is dragged far away: needs several iterations.
fn dragged_chain() -> Problem {
    let mut p = chain(30);
    p.targets.push(Target { var: VarId(30), value: 100.0, strength: Strength::Strong });
    p.targets.push(Target { var: VarId(31), value: 150.0, strength: Strength::Strong });
    p
}

#[test]
fn stepping_matches_one_shot() {
    let p = dragged_chain();
    let one = solve(&p, &opts());
    assert!(one.accepted(), "{:?}", one.status);
    let mut job = SolveJob::new(p.clone(), opts());
    let mut steps = 0;
    while job.step(1) != Progress::Finished {
        steps += 1;
        assert!(steps < 10_000);
    }
    let stepped = job.into_solution();
    assert!(steps > 1, "the job must actually yield between iterations");
    assert_eq!(stepped.values, one.values);
}

#[test]
fn cancel_between_steps_keeps_values() {
    let p = dragged_chain();
    let mut job = SolveJob::new(p.clone(), opts());
    assert!(matches!(job.step(1), Progress::Running { .. }));
    job.cancel();
    assert_eq!(job.step(1), Progress::Finished);
    let sol = job.into_solution();
    assert_eq!(sol.status, Status::Cancelled);
    assert_eq!(sol.values, p.values());
}

#[test]
fn chain_distances_hold_independently() {
    let mut p = chain(40);
    p.targets.push(Target { var: VarId(40), value: 150.0, strength: Strength::Strong });
    p.targets.push(Target { var: VarId(41), value: 120.0, strength: Strength::Strong });
    let sol = solve(&p, &opts());
    assert!(sol.accepted());
    let s = &sol.values;
    for i in 0..39 {
        let d = (s[2 * i + 2] - s[2 * i]).hypot(s[2 * i + 3] - s[2 * i + 1]);
        assert!((d - 10.0).abs() < 1e-6, "segment {i}: {d}");
    }
}

// ---------------------------------------------------------------------------
// Convergence on curved manifolds (SQP curvature, approach phase)

fn bent_chain(p: &mut Problem, links: usize, link: f64) -> Vec<(VarId, VarId, PointExpr)> {
    let mut pts = Vec::new();
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for i in 0..=links {
        pts.push(point(p, x, y));
        let a = 0.9 * (i as f64 * 0.35).sin();
        x += link * a.cos();
        y += link * a.sin();
    }
    rule(p, 1, rules::fix_point(&pts[0].2, (0.0, 0.0), L));
    for (i, w) in pts.windows(2).enumerate() {
        rule(p, 100 + i as u64, rules::distance(&w[0].2, &w[1].2, link, L));
    }
    pts
}

#[test]
fn far_preference_on_a_circle_converges_in_few_iterations() {
    // A point on a circle of radius 100 pulled towards (300, −250): the optimum is
    // the radial projection. Without constraint curvature in the step model this
    // creeps along the circle for dozens of iterations.
    let mut p = Problem::default();
    let (_, _, c) = point(&mut p, 0.0, 0.0);
    let (px, py, pp) = point(&mut p, 0.0, 100.0);
    rule(&mut p, 1, rules::fix_point(&c, (0.0, 0.0), L));
    rule(&mut p, 2, rules::distance(&c, &pp, 100.0, L));
    p.targets.push(Target { var: px, value: 300.0, strength: Strength::Strong });
    p.targets.push(Target { var: py, value: -250.0, strength: Strength::Strong });
    let sol = solve(&p, &SolveOptions::default());
    assert!(sol.accepted(), "{sol:?}");
    // The stays (weight 1e-3) pull back by ~1e-5 mm: compare at 1e-4 mm.
    let k = 100.0 / 300f64.hypot(250.0);
    assert!((at(&sol.values, px) - 300.0 * k).abs() < 1e-4, "{}", at(&sol.values, px));
    assert!((at(&sol.values, py) + 250.0 * k).abs() < 1e-4, "{}", at(&sol.values, py));
    let r = at(&sol.values, px).hypot(at(&sol.values, py));
    assert!((r - 100.0).abs() < 1e-6, "{r}");
    assert!(sol.iterations <= 15, "iterations {}", sol.iterations);
}

#[test]
fn far_hard_edit_keeps_the_elbow_branch() {
    // Two-link arm, elbow up. The end is fixed far from where it is: the approach
    // phase follows the manifold, so the elbow stays up instead of flipping.
    let mut p = Problem::default();
    let (_, _, base) = point(&mut p, 0.0, 0.0);
    let (jx, jy, joint) = point(&mut p, 50.0, 100.0 * (0.75f64).sqrt());
    let (ex, ey, end) = point(&mut p, 100.0, 0.0);
    rule(&mut p, 1, rules::fix_point(&base, (0.0, 0.0), L));
    rule(&mut p, 2, rules::distance(&base, &joint, 100.0, L));
    rule(&mut p, 3, rules::distance(&joint, &end, 100.0, L));
    rule(&mut p, 4, rules::fix_point(&end, (60.0, 150.0), L));
    let sol = solve(&p, &SolveOptions::default());
    assert!(sol.accepted(), "{sol:?}");
    let s = &sol.values;
    assert_eq!((at(s, ex), at(s, ey)), (60.0, 150.0));
    let (x, y) = (at(s, jx), at(s, jy));
    assert!((x.hypot(y) - 100.0).abs() < 1e-6);
    assert!(((60.0 - x).hypot(150.0 - y) - 100.0).abs() < 1e-6);
    // The elbow stays left of the base→end direction, as it started:
    // cross((100, 0), (50, 86.6)) > 0 before, cross((60, 150), (x, y)) after.
    let before = 100.0 * (100.0 * 0.75f64.sqrt()) - 0.0 * 50.0;
    let after = 60.0 * y - 150.0 * x;
    assert!(before > 0.0 && after > 0.0, "elbow flipped: ({x}, {y})");
}

#[test]
fn bent_chain_typed_end_position_is_exact_and_fast() {
    // 40 links of 100 mm, bent; the end is typed 300 mm away (hard).
    let mut p = Problem::default();
    let pts = bent_chain(&mut p, 40, 100.0);
    let (ex, ey, end) = pts[40].clone();
    let (x0, y0) = (p.vars[ex.index()].value, p.vars[ey.index()].value);
    let target = (x0 - 180.0, y0 + 240.0);
    rule(&mut p, 2, rules::fix_point(&end, target, L));
    let sol = solve(&p, &SolveOptions::default());
    assert!(sol.accepted(), "{sol:?}");
    let s = &sol.values;
    assert_eq!((at(s, ex), at(s, ey)), target);
    for w in pts.windows(2) {
        let d = (at(s, w[1].0) - at(s, w[0].0)).hypot(at(s, w[1].1) - at(s, w[0].1));
        assert!((d - 100.0).abs() < 1e-6, "link length {d}");
    }
    assert_eq!((at(s, pts[0].0), at(s, pts[0].1)), (0.0, 0.0));
    assert!(sol.iterations <= 60, "iterations {}", sol.iterations);
}
