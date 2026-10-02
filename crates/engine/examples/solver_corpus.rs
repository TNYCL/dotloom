//! Native run of the solver benchmark corpus (`scripts/bench/solver-corpus.mjs`).
//!
//! ```text
//! node scripts/bench/solver-corpus.mjs --out bench/solver-corpus.json
//! cargo run --release -p dotloom-engine --example solver_corpus -- bench/solver-corpus.json [case]
//! ```
//!
//! `BENCH_VERBOSE=1` prints every edit; `BENCH_NO_ANALYZE=1` skips the final rank
//! analysis (as interactive previews do).

use std::time::Instant;

use dotloom_engine::{ApplyOptions, Command, Engine, EntityTypeDef, Transaction};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    #[serde(default)]
    stress: bool,
    #[serde(default)]
    types: Vec<EntityTypeDef>,
    setup: Vec<Vec<Command>>,
    edits: Vec<Vec<Command>>,
}

fn percentile(xs: &[f64], p: f64) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    let i = ((p / 100.0) * s.len() as f64).ceil() as usize;
    s.get(i.saturating_sub(1).min(s.len().saturating_sub(1))).copied().unwrap_or(f64::NAN)
}

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: solver_corpus <corpus.json>");
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return;
        }
    };
    let cases: Vec<Case> = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return;
        }
    };
    let mut gated = Vec::new();
    let mut stress = Vec::new();
    let only = std::env::args().nth(2);
    for c in cases {
        if only.as_ref().is_some_and(|o| *o != c.name) {
            continue;
        }
        let mut opts = dotloom_engine::EngineOptions::default();
        opts.solve.analyze = std::env::var("BENCH_NO_ANALYZE").is_err();
        let mut e = Engine::new(opts);
        for t in c.types {
            if let Err(err) = e.register_type(t, "bench") {
                eprintln!("{}: {err}", c.name);
            }
        }
        for s in c.setup {
            if let Err(err) = e.apply(Transaction::new("setup", s), ApplyOptions::default()) {
                eprintln!("{}: setup failed: {err}", c.name);
            }
        }
        let mut times = Vec::new();
        let (mut iters, mut attempts, mut rejected) = (Vec::new(), Vec::new(), 0);
        for edit in c.edits {
            let t = Instant::now();
            match e.apply(Transaction::new("edit", edit), ApplyOptions::default()) {
                Ok(r) => {
                    if let Some(s) = r.solver {
                        iters.push(s.iterations);
                        attempts.push(s.attempts);
                        if std::env::var("BENCH_VERBOSE").is_ok() {
                            println!(
                                "  edit {} iterations {} attempts {} status {:?} {:.2} ms",
                                times.len(),
                                s.iterations,
                                s.attempts,
                                r.status,
                                t.elapsed().as_secs_f64() * 1000.0
                            );
                        }
                    }
                }
                Err(_) => rejected += 1,
            }
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let violations = e.verify().len();

        println!(
            "{:<16} p50 {:>9.2} ms  p95 {:>9.2} ms  max {:>9.2} ms  rejected {rejected}  violations {violations}  iterations {:?}  attempts {:?}",
            c.name,
            percentile(&times, 50.0),
            percentile(&times, 95.0),
            times.iter().copied().fold(0.0, f64::max),
            iters.iter().max(),
            attempts.iter().max(),
        );
        if c.stress { stress.extend(times) } else { gated.extend(times) }
    }
    println!(
        "gated   p50 {:.2} ms  p95 {:.2} ms  ({} edits)",
        percentile(&gated, 50.0),
        percentile(&gated, 95.0),
        gated.len()
    );
    if !stress.is_empty() {
        println!(
            "stress  p50 {:.2} ms  p95 {:.2} ms  ({} edits)",
            percentile(&stress, 50.0),
            percentile(&stress, 95.0),
            stress.len()
        );
    }
}
