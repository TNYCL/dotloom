//! Incremental linear sessions (DL-SOLVE-4): every result must equal a full solve
//! of the same problem, and anything the session cannot decide falls back to it.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dotloom_constraints::{
    Expr, LinearSession, PointExpr, Problem, Rule, SolveOptions, Status, Strength, VarId, Variable,
    rules::{self, Cmp},
    solve,
};
use proptest::prelude::*;

const L: f64 = 1000.0;

fn v(id: VarId) -> Expr {
    Expr::Var(id)
}

fn preview() -> SolveOptions {
    SolveOptions { analyze: false, ..SolveOptions::default() }
}

/// Shelf: total = w1 + w2 + w3, w1 = 600, w2 = w3 ≥ 400; the dragged rule fixes
/// `total` (index of that rule returned).
fn shelf(total: f64, strength: Strength) -> (Problem, usize) {
    let mut p = Problem::default();
    let t = p.add_var(Variable::new(1800.0).scale(L));
    let w1 = p.add_var(Variable::new(600.0).scale(L));
    let w2 = p.add_var(Variable::new(600.0).scale(L));
    let w3 = p.add_var(Variable::new(600.0).scale(L));
    let req = |p: &mut Problem, id, rows| p.rules.push(Rule::new(id, rows, Strength::Required));
    req(&mut p, 1, rules::linear(&[(1.0, v(w1)), (1.0, v(w2)), (1.0, v(w3)), (-1.0, v(t))], Cmp::Eq, 0.0, L));
    req(&mut p, 2, rules::equal(v(w2), v(w3), L));
    req(&mut p, 3, rules::at_least(v(w2), 400.0, L));
    req(&mut p, 4, rules::at_least(v(w3), 400.0, L));
    req(&mut p, 5, rules::fix(v(w1), 600.0, L));
    p.rules.push(Rule::new(6, rules::fix(v(t), total, L), strength).label("drag total"));
    let moving = p.rules.len() - 1;
    (p, moving)
}

/// A session result must equal the full solve of the same problem. `None` is always
/// allowed (the caller then runs the full solve); the tests below count fallbacks.
fn assert_same(session: Option<dotloom_constraints::Solution>, p: &Problem, what: &str) {
    let Some(s) = session else { return };
    let full = solve(p, &preview());
    assert!(full.accepted(), "{what}: session accepted but the full solve says {:?}", full.status);
    assert_eq!(s.status, full.status, "{what}");
    for (i, (a, b)) in s.values.iter().zip(&full.values).enumerate() {
        assert!((a - b).abs() <= 1e-7, "{what}: var {i}: session {a} vs full {b}");
    }
}

#[test]
fn hard_drag_target_matches_full_solves_and_rejects_infeasible_positions() {
    let (p0, moving) = shelf(1800.0, Strength::Required);
    let mut session = LinearSession::new(&p0, &[moving], preview()).expect("linear problem");
    let mut accepted = 0;
    let mut rejected = 0;
    // Down past the 1400 mm lower bound (w2 = w3 ≥ 400) and back up.
    let path: Vec<f64> =
        (0..=16).map(|i| 1800.0 - 50.0 * f64::from(i)).chain((0..=16).map(|i| 1000.0 + 50.0 * f64::from(i))).collect();
    for total in path {
        let (p, _) = shelf(total, Strength::Required);
        let full = solve(&p, &preview());
        let s = session.resolve(&p);
        if full.accepted() {
            let s = s.unwrap_or_else(|| panic!("total {total}: session fell back on a feasible position"));
            accepted += 1;
            assert!((s.values[2] - (total - 600.0) / 2.0).abs() < 1e-7, "total {total}: w2 = {}", s.values[2]);
            assert_same(Some(s), &p, &format!("total {total}"));
        } else {
            assert!(s.is_none(), "total {total}: session accepted an infeasible hard target");
            assert_eq!(full.status, Status::Conflicting);
            rejected += 1;
        }
        assert!(!session.is_broken(), "edit-variable moves never break the session");
    }
    assert_eq!((accepted, rejected), (18, 16));
}

#[test]
fn soft_drag_target_stops_at_the_nearest_feasible_value() {
    let (p0, moving) = shelf(1800.0, Strength::Strong);
    let mut session = LinearSession::new(&p0, &[moving], preview()).expect("linear problem");
    for total in [1700.0, 1500.0, 1300.0, 900.0, 1400.0, 2400.0] {
        let (p, _) = shelf(total, Strength::Strong);
        let s = session.resolve(&p).expect("a soft target is always feasible");
        assert!((s.values[0] - total.max(1400.0)).abs() < 1e-7, "total {total}: {}", s.values[0]);
        assert_same(Some(s), &p, &format!("soft total {total}"));
    }
}

#[test]
fn changed_constants_outside_the_target_are_re_added_incrementally() {
    // A moved fixed value (w1 locked at a new width) changes a non-moving row.
    let (p0, moving) = shelf(1800.0, Strength::Strong);
    let mut session = LinearSession::new(&p0, &[moving], preview()).expect("linear problem");
    for w1 in [600.0, 700.0, 500.0, 900.0] {
        let (mut p, _) = shelf(1800.0, Strength::Strong);
        p.rules[4] = Rule::new(5, rules::fix(v(VarId(1)), w1, L), Strength::Required);
        let s = session.resolve(&p).expect("feasible");
        assert!((s.values[1] - w1).abs() < 1e-7);
        assert!((s.values[2] - (1800.0 - w1) / 2.0).abs() < 1e-7);
        assert_same(Some(s), &p, &format!("w1 {w1}"));
    }
}

#[test]
fn structural_changes_and_nonlinear_problems_fall_back() {
    let (p0, moving) = shelf(1800.0, Strength::Required);
    let mut session = LinearSession::new(&p0, &[moving], preview()).expect("linear problem");
    // An extra rule: different structure.
    let (mut p, _) = shelf(1700.0, Strength::Required);
    p.rules.push(Rule::new(7, rules::at_most(v(VarId(2)), 700.0, L), Strength::Required));
    assert!(session.resolve(&p).is_none());
    // A newly fixed variable: different structure.
    let (mut p, _) = shelf(1700.0, Strength::Required);
    p.vars[2].fixed = true;
    assert!(session.resolve(&p).is_none());
    // The original structure still works afterwards.
    let (p, _) = shelf(1700.0, Strength::Required);
    assert!(session.resolve(&p).is_some());

    // Distance is nonlinear: no session.
    let mut q = Problem::default();
    let a = PointExpr::vars(q.add_var(Variable::new(0.0).scale(L)), q.add_var(Variable::new(0.0).scale(L)));
    let b = PointExpr::vars(q.add_var(Variable::new(100.0).scale(L)), q.add_var(Variable::new(0.0).scale(L)));
    q.rules.push(Rule::new(1, rules::distance(&a, &b, 100.0, L), Strength::Required));
    assert!(LinearSession::new(&q, &[], preview()).is_none());

    // Required rules that already contradict each other: no session.
    let (mut bad, moving) = shelf(1800.0, Strength::Required);
    bad.rules.push(Rule::new(8, rules::fix(v(VarId(2)), 100.0, L), Strength::Required));
    assert!(LinearSession::new(&bad, &[moving], preview()).is_none());
}

/// A chain of `n` boxes: x₀ fixed, xᵢ₊₁ = xᵢ + wᵢ, 50 ≤ wᵢ ≤ 400, w₁ = w₃, a weak
/// preference w₀ = 120, and the dragged end x_n.
fn chain(n: usize, end: f64, strength: Strength, origin: f64, stays: &[f64]) -> (Problem, usize) {
    let mut p = Problem::default();
    let x0 = p.add_var(Variable::new(origin).scale(L).fixed(true));
    let mut xs = vec![x0];
    let mut ws = Vec::new();
    for i in 0..n {
        let stay = stays.get(i).copied().unwrap_or(1.0);
        ws.push(p.add_var(Variable::new(100.0).scale(L).stay(stay)));
        xs.push(p.add_var(Variable::new(origin + 100.0 * (i + 1) as f64).scale(L)));
    }
    let mut id = 0;
    let mut req = |p: &mut Problem, rows| {
        id += 1;
        p.rules.push(Rule::new(id, rows, Strength::Required));
    };
    for i in 0..n {
        req(&mut p, rules::linear(&[(1.0, v(xs[i])), (1.0, v(ws[i])), (-1.0, v(xs[i + 1]))], Cmp::Eq, 0.0, L));
        req(&mut p, rules::at_least(v(ws[i]), 50.0, L));
        req(&mut p, rules::at_most(v(ws[i]), 400.0, L));
    }
    if n > 3 {
        req(&mut p, rules::equal(v(ws[1]), v(ws[3]), L));
    }
    p.rules.push(Rule::new(1000, rules::fix(v(ws[0]), 120.0, L), Strength::Weak));
    p.rules.push(Rule::new(1001, rules::fix(v(xs[n]), end, L), strength));
    let moving = p.rules.len() - 1;
    (p, moving)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn chain_drags_match_full_solves(
        n in 2usize..12,
        hard in any::<bool>(),
        stays in proptest::collection::vec(0.1f64..5.0, 12),
        path in proptest::collection::vec((-300.0f64..5000.0, -50.0f64..50.0), 1..12),
    ) {
        let strength = if hard { Strength::Required } else { Strength::Strong };
        let (p0, moving) = chain(n, 100.0 * n as f64, strength, 0.0, &stays);
        let mut session = LinearSession::new(&p0, &[moving], preview()).expect("linear chain");
        let mut fell_back = 0;
        let mut feasible = 0;
        for (end, origin) in path {
            let (p, _) = chain(n, end, strength, origin, &stays);
            let full = solve(&p, &preview());
            let s = session.resolve(&p);
            if full.accepted() {
                feasible += 1;
                if s.is_none() { fell_back += 1; }
            } else {
                prop_assert!(s.is_none(), "session accepted what the full solve rejects ({:?})", full.status);
            }
            assert_same(s, &p, &format!("n {n}, end {end}, origin {origin}, hard {hard}"));
            if session.is_broken() {
                session = LinearSession::new(&p, &[moving], preview()).expect("rebuild");
            }
        }
        // The session answers every feasible position itself.
        prop_assert_eq!(fell_back, 0, "{} of {} feasible positions fell back", fell_back, feasible);
    }
}
