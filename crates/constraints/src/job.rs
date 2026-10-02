//! Budgeted, cancellable solve jobs.
//!
//! A [`SolveJob`] is a resumable state machine. Hosts call [`SolveJob::step`] with a
//! work budget (Gauss–Newton iterations; a linear component counts as one unit),
//! yield to their event loop, and call it again. [`SolveJob::cancel`] between steps
//! makes the job finish as `cancelled` with the input values untouched — this is
//! how the Worker implements real cancellation without threads.

use crate::{
    Backend, Certainty, ComponentReport, Diagnostic, DiagnosticKind, Problem, Solution, SolveOptions, Status, Strength,
    analysis::{Columns, eval_rows, max_violation},
    graph::{Component, components},
    linear::{conflict_diagnostic, solve_linear},
    numeric::NumericSolver,
};

/// Progress of a job after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// More work remains.
    Running {
        /// Components finished so far.
        completed: usize,
        /// Total components.
        total: usize,
    },
    /// The job is finished (solved, failed or cancelled).
    Finished,
}

/// A resumable solve.
#[derive(Debug)]
pub struct SolveJob {
    problem: Problem,
    opts: SolveOptions,
    x: Vec<f64>,
    x_in: Vec<f64>,
    comps: Vec<Component>,
    next: usize,
    current: Option<NumericSolverBox>,
    reports: Vec<ComponentReport>,
    diagnostics: Vec<Diagnostic>,
    cancelled: bool,
    invalid: Option<String>,
    iterations: u32,
}

// Wrapper so `SolveJob` can derive Debug without exposing solver internals.
struct NumericSolverBox(NumericSolver);

impl core::fmt::Debug for NumericSolverBox {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("NumericSolver")
    }
}

fn validate(p: &Problem) -> Result<(), String> {
    let n = p.vars.len();
    if u32::try_from(n).is_err() {
        return Err("too many variables".into());
    }
    for (i, v) in p.vars.iter().enumerate() {
        if !v.value.is_finite() {
            return Err(format!("variable {i} ({}) is not finite", v.label));
        }
    }
    for r in &p.rules {
        for v in r.vars() {
            if v.index() >= n {
                return Err(format!("rule {} references unknown variable {}", r.id, v.0));
            }
        }
        for row in &r.rows {
            if row.expr.size() > 100_000 {
                return Err(format!("rule {} expression too large", r.id));
            }
        }
    }
    for t in &p.targets {
        if t.var.index() >= n || !t.value.is_finite() {
            return Err("invalid target".into());
        }
    }
    Ok(())
}

impl SolveJob {
    /// Prepare a job. Values of `problem.vars` are the starting point and the
    /// "stay near" reference.
    #[must_use]
    pub fn new(problem: Problem, opts: SolveOptions) -> Self {
        let x = problem.values();
        let invalid = validate(&problem).err();
        let comps = if invalid.is_some() { Vec::new() } else { components(&problem) };
        Self {
            x_in: x.clone(),
            x,
            comps,
            next: 0,
            current: None,
            reports: Vec::new(),
            diagnostics: Vec::new(),
            cancelled: false,
            invalid,
            iterations: 0,
            problem,
            opts,
        }
    }

    /// Request cancellation. The next [`SolveJob::step`] returns `Finished`.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// Whether all work is done.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.cancelled || self.invalid.is_some() || self.next >= self.comps.len()
    }

    /// Aggregate status of the components finished so far (meaningful once
    /// [`SolveJob::is_finished`] is true).
    #[must_use]
    pub fn peek_status(&self) -> Status {
        if self.cancelled {
            return Status::Cancelled;
        }
        if self.invalid.is_some() {
            return Status::Unsupported;
        }
        self.reports.iter().fold(Status::Solved, |acc, r| acc.combine(r.status))
    }

    /// Run at most `budget` work units.
    pub fn step(&mut self, budget: u32) -> Progress {
        let mut left = budget.max(1);
        while left > 0 && !self.is_finished() {
            let Some(comp) = self.comps.get(self.next).cloned() else { break };
            match comp.backend {
                Backend::Trivial => {
                    // An unsupported rule has no rows, so it always lands here.
                    if let Some(unsupported) = self.unsupported(&comp) {
                        self.push_unsupported(&comp, unsupported);
                    } else {
                        self.solve_trivial(&comp);
                    }
                    self.next += 1;
                }
                Backend::Linear => {
                    if let Some(unsupported) = self.unsupported(&comp) {
                        self.push_unsupported(&comp, unsupported);
                    } else {
                        let out = solve_linear(&self.problem, &comp, &mut self.x, &self.x_in, &self.opts);
                        self.iterations += 1;
                        self.reports.push(self.report(&comp, Backend::Linear, out.status, 1, out.max_hard));
                        self.diagnostics.extend(out.diagnostics);
                    }
                    self.next += 1;
                    left -= 1;
                    continue;
                }
                Backend::Numeric => {
                    if let Some(unsupported) = self.unsupported(&comp) {
                        self.push_unsupported(&comp, unsupported);
                        self.next += 1;
                        continue;
                    }
                    let solver = self
                        .current
                        .get_or_insert_with(|| NumericSolverBox(NumericSolver::new(&self.problem, &comp, &self.x_in)));
                    let finished = solver.0.iterate(&self.problem, &mut self.x, &self.opts);
                    self.iterations += 1;
                    left -= 1;
                    if finished && let Some(NumericSolverBox(s)) = self.current.take() {
                        self.finish_numeric(&comp, &s);
                        self.next += 1;
                    }
                    continue;
                }
            }
        }
        if self.is_finished() {
            Progress::Finished
        } else {
            Progress::Running { completed: self.next, total: self.comps.len() }
        }
    }

    fn unsupported(&self, comp: &Component) -> Option<(usize, String)> {
        comp.rules
            .iter()
            .find_map(|ri| self.problem.rules.get(*ri).and_then(|r| r.unsupported.clone().map(|u| (*ri, u))))
    }

    fn push_unsupported(&mut self, comp: &Component, (ri, why): (usize, String)) {
        let mut d = conflict_diagnostic(&self.problem, &[ri], Certainty::Certain, None, why);
        d.kind = DiagnosticKind::Unsupported;
        self.diagnostics.push(d);
        self.reports.push(self.report(comp, comp.backend, Status::Unsupported, 0, f64::NAN));
    }

    fn report(
        &self,
        comp: &Component,
        backend: Backend,
        status: Status,
        iterations: u32,
        max_hard: f64,
    ) -> ComponentReport {
        ComponentReport {
            vars: comp.vars.clone(),
            rules: comp.rules.iter().filter_map(|ri| self.problem.rules.get(*ri).map(|r| r.id)).collect(),
            backend,
            status,
            iterations,
            max_hard_residual: max_hard,
        }
    }

    fn solve_trivial(&mut self, comp: &Component) {
        let (status, diagnostics, max_hard) = solve_trivial(&self.problem, comp, &mut self.x, &self.opts);
        self.diagnostics.extend(diagnostics);
        self.reports.push(self.report(comp, Backend::Trivial, status, 0, max_hard));
    }

    fn finish_numeric(&mut self, comp: &Component, s: &NumericSolver) {
        let mut status = s.done.unwrap_or(Status::NotConverged { suspected_conflict: false });
        if status == Status::Conflicting {
            let evidence = s.inconsistent_rules(&self.problem, &self.x, &self.opts);
            self.diagnostics.push(conflict_diagnostic(
                &self.problem,
                &evidence,
                Certainty::Certain,
                None,
                "hard rules fix the same value to different numbers".into(),
            ));
        } else if let Status::NotConverged { suspected_conflict } = status {
            let evidence = s.inconsistent_rules(&self.problem, &self.x, &self.opts);
            let fixed = |v| self.problem.is_fixed(v);
            let all_linear = !evidence.is_empty()
                && evidence.iter().all(|ri| {
                    self.problem
                        .rules
                        .get(*ri)
                        .is_some_and(|r| r.rows.iter().all(|row| row.expr.linear_form(&self.x, &fixed).is_some()))
                });
            let residual = Some(s.max_hard);
            if all_linear {
                status = Status::Conflicting;
                self.diagnostics.push(conflict_diagnostic(
                    &self.problem,
                    &evidence,
                    Certainty::Certain,
                    residual,
                    "linear rules in a mixed component contradict each other".into(),
                ));
            } else if suspected_conflict || !evidence.is_empty() {
                status = Status::NotConverged { suspected_conflict: true };
                let rules = if evidence.is_empty() { comp.rules.clone() } else { evidence };
                self.diagnostics.push(conflict_diagnostic(
                    &self.problem,
                    &rules,
                    Certainty::Suspected,
                    residual,
                    "hard rules could not be satisfied; the residual stopped decreasing".into(),
                ));
            } else {
                let mut d = conflict_diagnostic(
                    &self.problem,
                    &comp.rules,
                    Certainty::Suspected,
                    residual,
                    format!("iteration budget of {} exhausted", self.opts.max_iterations),
                );
                d.kind = DiagnosticKind::NotConverged;
                self.diagnostics.push(d);
            }
        } else {
            let redundant = s.redundant_rules(&self.problem, &self.x, &self.opts);
            if !redundant.is_empty() {
                let mut d = conflict_diagnostic(
                    &self.problem,
                    &redundant,
                    Certainty::Certain,
                    None,
                    "redundant rules (consistent)".into(),
                );
                d.kind = DiagnosticKind::Redundant;
                self.diagnostics.push(d);
            }
        }
        self.reports.push(self.report(comp, Backend::Numeric, status, s.iterations, s.max_hard));
    }

    /// Finish the job (running remaining work unless cancelled) and return the result.
    #[must_use]
    pub fn into_solution(mut self) -> Solution {
        while !self.is_finished() {
            self.step(u32::MAX);
        }
        if let Some(why) = self.invalid.take() {
            return Solution {
                values: self.x_in,
                status: Status::Unsupported,
                components: Vec::new(),
                diagnostics: vec![Diagnostic {
                    kind: DiagnosticKind::Unsupported,
                    certainty: Certainty::Certain,
                    rules: Vec::new(),
                    labels: Vec::new(),
                    sources: Vec::new(),
                    entities: Vec::new(),
                    residual: None,
                    message: why,
                }],
                iterations: 0,
            };
        }
        if self.cancelled {
            let mut diagnostics = self.diagnostics;
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Cancelled,
                certainty: Certainty::Certain,
                rules: Vec::new(),
                labels: Vec::new(),
                sources: Vec::new(),
                entities: Vec::new(),
                residual: None,
                message: "solve cancelled".into(),
            });
            return Solution {
                values: self.x_in,
                status: Status::Cancelled,
                components: self.reports,
                diagnostics,
                iterations: self.iterations,
            };
        }
        let status = self.reports.iter().fold(Status::Solved, |acc, r| acc.combine(r.status));
        let values = if status.is_acceptable() { self.x } else { self.x_in };
        Solution {
            values,
            status,
            components: self.reports,
            diagnostics: self.diagnostics,
            iterations: self.iterations,
        }
    }
}

/// Solve to completion.
#[must_use]
pub fn solve(problem: &Problem, opts: &SolveOptions) -> Solution {
    SolveJob::new(problem.clone(), *opts).into_solution()
}

/// Solve a [`Backend::Trivial`] component: free variables without rules follow their
/// strongest target; rules without free variables are only verified.
pub(crate) fn solve_trivial(
    p: &Problem,
    comp: &Component,
    x: &mut [f64],
    opts: &SolveOptions,
) -> (Status, Vec<Diagnostic>, f64) {
    if comp.rules.is_empty() {
        for v in &comp.vars {
            let best = p
                .targets
                .iter()
                .filter(|t| t.var == *v)
                .max_by_key(|t| if t.strength == Strength::Required { Strength::Strong } else { t.strength });
            if let (Some(t), Some(slot)) = (best, x.get_mut(v.index())) {
                *slot = t.value;
            }
        }
        let dof = comp.vars.len();
        let status = if dof == 0 { Status::Solved } else { Status::Underconstrained { dof } };
        return (status, Vec::new(), 0.0);
    }
    let cols = Columns::new(p, &[]);
    let rows = eval_rows(p, &comp.rules, &cols, x);
    let hard: Vec<_> = rows.iter().filter(|r| p.rules.get(r.rule).is_some_and(crate::Rule::is_hard)).cloned().collect();
    let max_hard = max_violation(&hard);
    if max_hard > opts.tolerance {
        let msg = "rule depends only on fixed values and is violated".to_owned();
        let residual = hard.iter().map(|r| r.value.abs()).fold(0.0, f64::max);
        let d = conflict_diagnostic(p, &comp.rules, Certainty::Certain, Some(residual), msg);
        return (Status::Conflicting, vec![d], max_hard);
    }
    let soft_unmet =
        rows.iter().any(|r| p.rules.get(r.rule).is_some_and(|x| !x.is_hard()) && r.violation() > opts.tolerance);
    let mut diagnostics = Vec::new();
    if soft_unmet {
        let mut d = conflict_diagnostic(
            p,
            &comp.rules,
            Certainty::Certain,
            None,
            "preference cannot be met: all its variables are fixed".into(),
        );
        d.kind = DiagnosticKind::PreferenceUnmet;
        diagnostics.push(d);
    }
    (Status::Solved, diagnostics, max_hard)
}
