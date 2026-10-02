//! Incremental vs full linear solves during a drag (DL-SOLVE-4).
//!
//! A chain of `n` boxes (x₀ fixed, xᵢ₊₁ = xᵢ + wᵢ, 50 ≤ wᵢ ≤ 400, a weak width
//! preference) whose end is dragged along 200 pointer positions. Each position is
//! solved from scratch and by one [`LinearSession`]; the results must agree.
//!
//! ```text
//! cargo run --release -p dotloom-constraints --example drag_session -- 50 100 200
//! ```

use std::time::Instant;

use dotloom_constraints::{
    Expr, LinearSession, Problem, Rule, SolveOptions, Strength, VarId, Variable,
    rules::{self, Cmp},
    solve,
};

const L: f64 = 1000.0;

fn chain(n: usize, end: f64) -> (Problem, usize) {
    let v = Expr::Var;
    let mut p = Problem::default();
    let mut xs: Vec<VarId> = vec![p.add_var(Variable::new(0.0).scale(L).fixed(true))];
    let mut ws = Vec::new();
    for i in 0..n {
        ws.push(p.add_var(Variable::new(100.0).scale(L)));
        xs.push(p.add_var(Variable::new(100.0 * (i + 1) as f64).scale(L)));
    }
    let mut id = 0;
    for i in 0..n {
        for rows in [
            rules::linear(&[(1.0, v(xs[i])), (1.0, v(ws[i])), (-1.0, v(xs[i + 1]))], Cmp::Eq, 0.0, L),
            rules::at_least(v(ws[i]), 50.0, L),
            rules::at_most(v(ws[i]), 400.0, L),
        ] {
            id += 1;
            p.rules.push(Rule::new(id, rows, Strength::Required));
        }
    }
    p.rules.push(Rule::new(id + 1, rules::fix(v(ws[0]), 120.0, L), Strength::Weak));
    p.rules.push(Rule::new(id + 2, rules::fix(v(xs[n]), end, L), Strength::Strong));
    let moving = p.rules.len() - 1;
    (p, moving)
}

fn pct(xs: &mut [f64], q: f64) -> f64 {
    xs.sort_by(f64::total_cmp);
    let i = ((q * xs.len() as f64).ceil() as usize).clamp(1, xs.len()) - 1;
    xs.get(i).copied().unwrap_or(f64::NAN)
}

fn main() {
    let sizes: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let sizes = if sizes.is_empty() { vec![50, 100, 200] } else { sizes };
    let opts = SolveOptions { analyze: false, ..SolveOptions::default() };
    println!("boxes  vars  full p50/p95 ms  incremental p50/p95 ms  speed-up (p50)  max |Δ| mm");
    for n in sizes {
        let path: Vec<f64> =
            (0..200).map(|k| 100.0 * n as f64 * (1.0 + 0.8 * libm::sin(f64::from(k) * 0.05))).collect();
        let (p0, moving) = chain(n, path[0]);
        let Some(mut session) = LinearSession::new(&p0, &[moving], opts) else {
            eprintln!("no session for n = {n}");
            continue;
        };
        let (mut full_ms, mut inc_ms, mut max_diff) = (Vec::new(), Vec::new(), 0.0f64);
        for end in &path {
            let (p, _) = chain(n, *end);
            let t = Instant::now();
            let full = solve(&p, &opts);
            full_ms.push(t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            let inc = session.resolve(&p);
            inc_ms.push(t.elapsed().as_secs_f64() * 1e3);
            match inc {
                Some(s) => {
                    for (a, b) in s.values.iter().zip(&full.values) {
                        max_diff = max_diff.max((a - b).abs());
                    }
                }
                None => eprintln!("n = {n}, end = {end}: session fell back"),
            }
        }
        let (f50, f95) = (pct(&mut full_ms, 0.5), pct(&mut full_ms, 0.95));
        let (i50, i95) = (pct(&mut inc_ms, 0.5), pct(&mut inc_ms, 0.95));
        println!(
            "{n:>5}  {:>4}  {f50:>7.3} / {f95:<7.3}  {i50:>9.3} / {i95:<9.3}   {:>6.1}×        {max_diff:.1e}",
            p0.vars.len(),
            f50 / i50.max(1e-9)
        );
    }
}
