//! Stress probe: a chain of `n` points with fixed link lengths, pinned at both ends,
//! whose middle point is dragged towards an unreachable target. The straight, taut
//! chain at the optimum is a near-singular configuration.
//!
//! ```text
//! cargo run --release -p dotloom-constraints --example chain_stress -- 100
//! ```

use std::time::Instant;

use dotloom_constraints::{PointExpr, Problem, Rule, SolveOptions, Strength, Target, VarId, Variable, rules, solve};

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(100).max(3);
    let scale = 1000.0;
    let mut p = Problem::default();
    let pts: Vec<PointExpr> = (0..n)
        .map(|i| {
            let x = p.add_var(Variable::new(i as f64 * 9.0).scale(scale));
            let y = p.add_var(Variable::new((i % 3) as f64).scale(scale));
            PointExpr::vars(x, y)
        })
        .collect();
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return };
    p.rules.push(Rule::new(1, rules::fix_point(first, (0.0, 0.0), scale), Strength::Required));
    p.rules.push(Rule::new(2, rules::fix_point(last, ((n - 1) as f64 * 8.0, 0.0), scale), Strength::Required));
    for (i, w) in pts.windows(2).enumerate() {
        p.rules.push(Rule::new(10 + i as u64, rules::distance(&w[0], &w[1], 10.0, scale), Strength::Required));
    }
    let mid = u32::try_from(n / 2).unwrap_or(0);
    p.targets.push(Target { var: VarId(2 * mid), value: 100.0, strength: Strength::Strong });
    p.targets.push(Target { var: VarId(2 * mid + 1), value: 150.0, strength: Strength::Strong });
    let t = Instant::now();
    let s = solve(&p, &SolveOptions::default());
    println!("points={n} vars={} status={:?} iterations={} time={:?}", 2 * n, s.status, s.iterations, t.elapsed());
}
