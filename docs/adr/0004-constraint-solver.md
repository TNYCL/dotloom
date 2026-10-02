# ADR-0004: Constraint solver architecture

Status: accepted (2026-10-02), revised 2026-10-02 after implementation measurements
and again after the DL-PERF-4 solver benchmark (sparse algebra, curvature, approach
phase; see `docs/performance.md`)

## Decision

1. **Variables** are scalar parameters: entity geometry parameters (line endpoints,
   circle radius, ...) and numeric entity properties (shelf widths, block start times).
   Anchors are expressions of variables through the entity transform.
2. **Rows** are scalar `Expr` residuals (`= 0` or `≤ 0`) with exact derivatives
   (forward-mode automatic differentiation on sparse gradients). Built-in geometric
   rules and plugin expressions compile to the same representation; Jacobians are
   checked against central differences in `crates/constraints/tests/jacobian.rs`.
3. **Graph and components.** Rules and free variables form a bipartite graph. Fixed
   variables do not connect rules. Each connected component is classified once per
   solve:
   - *linear*: every row is affine in the free variables → **kasuari** (maintained
     Cassowary implementation, MIT/Apache-2.0). Required rules are hard; preferences and
     targets use strong/medium/weak. Variables and rows are scaled to O(1) because
     kasuari's internal zero test is absolute (1e-8). kasuari's hash maps are randomly
     seeded, so every variable gets a stay preference whose weight differs slightly by
     stable column order — ties are broken deterministically (tested 50× in-process).
   - *nonlinear/mixed*: the whole component goes to the numeric backend; its linear
     rows are ordinary rows. A variable is owned by exactly one backend per solve.
4. **Numeric backend** (`crates/constraints/src/numeric.rs`):
   - *presolve* eliminates unknowns fixed by hard equalities that are linear in a single
     unknown (`fix`, `fixPoint`, propagated chains); contradictions found here are
     certain conflicts;
   - *approach phase*: single-unknown hard equalities that the starting point violates
     (typed edits, hard drag targets) are first followed as strong preferences from the
     current, feasible configuration and only then enforced exactly (presolved). It
     ends when they are within 1e-6 (scaled) or a step gains < 0.1 %. Jumping straight
     to the new value starts the restoration far from the manifold, where Newton steps
     on chains overshoot and crawl (measured: up to 100 iterations); following the
     manifold also keeps the previous branch (elbow side) instead of flipping;
   - *restoration phase* (start point violates hard rows): damped minimum-norm
     Gauss–Newton steps `Δ = Aᵀ(AAᵀ)⁻¹(−r)` on the hard rows only, with an Armijo test
     on `Σ violation²` (the maximum violation as merit rejected good steps that trade
     error between rows). Levenberg–Marquardt damping and a non-monotone acceptance
     were measured and rejected: damping relative to the largest diagonal suppresses
     the low-frequency "move the whole chain" mode of path-like Jacobians;
   - *optimization phase* (feasible point): equality-constrained quadratic step —
     hard rows are exact constraints of the step, never penalty terms; preferences
     (strong 1, medium 0.1, weak 0.01) and stays towards the previous valid values
     (0.001) form the objective. The bounded weight ladder keeps the system well
     conditioned (an earlier variant with a 1e-5…1e3 range lost the hard rows of cheap
     variables to rounding and was rejected). From the second step on, the model
     includes the **constraint curvature** `Σ λᵢ∇²cᵢ` (row Hessians by forward
     differences of the exact gradients, weighted with the previous multipliers): an
     SQP step. Without it the method is a projected Gauss–Newton step that converges
     linearly on curved manifolds (measured: 101-iteration budget exhausted on a
     40-gon and a tangent-circle chain; 14–18 iterations with curvature). When the
     curvature makes the model non-convex on the feasible directions (inertia check;
     `ρ·AᵀA` augmentation removes indefiniteness in the range of `Aᵀ` without changing
     the step), the step falls back to the Gauss–Newton model. Each trial step is
     projected back onto the hard manifold with damped Newton steps and accepted only
     if all hard rows hold and the objective decreases;
   - *linear algebra* (`crates/constraints/src/sparse.rs`): sparse, deterministic.
     Minimum-norm corrections and diagonal-Hessian steps factor the normal equations
     `A·D·Aᵀ` (reverse Cuthill–McKee order, envelope Cholesky, escalating diagonal
     regularization for redundant rows); curvature steps factor the quasi-definite KKT
     matrix `[H Aᵀ; A −εI]` with `L·D·Lᵀ` (duals ordered after their primal
     neighbours, inertia check, two steps of iterative refinement). The dense
     predecessor spent 75 % of a 200-variable solve in `A·Aᵀ` and its Cholesky;
   - inequalities use a primal active set: the most violated linearized inequality is
     added, active rows with negative multipliers are released;
   - trust region on the scaled step, explicit stop criteria (hard rows within
     `tolerance · row.scale`, relative objective gain ≤ 1e-9 or step ≤ 1e-12, iteration
     budget). A feasible point reached when the budget runs out is still valid;
   - line-search trial points are evaluated without gradients (values only).
5. **Status**: `solved` (no remaining DOF), `underconstrained { dof }` (valid and
   usable; DOF counts equality rows only — an inequality at its bound limits motion in
   one direction but removes no freedom), `conflicting` (proven: kasuari
   required-failure with a deletion-filter minimal set, presolve contradiction, rules
   depending only on fixed values, or a rank-deficient *linear* row set violated by the
   residual), `notConverged { suspectedConflict }` (local evidence only — never shown as
   proof), `cancelled`, `unsupported`.
6. **Commit rule**: the engine re-evaluates every hard rule with an independent
   geometric evaluator after solving; if any exceeds tolerance the transaction is
   rejected and the document stays at the previous revision.
7. **Explanation**: on linear conflicts the engine re-solves with the user's edit
   demoted to a strong target to report the nearest feasible value (shelf total
   130 cm → 140 cm). Minimal conflict sets are computed only on the failure path and
   are bounded by `conflict_search_limit`.
8. **Cancellation**: `SolveJob::step(budget)` runs at most `budget` iterations (a
   linear component counts as one); `cancel()` between steps finishes the job as
   `cancelled` with the input values.
9. **Branches**: minimum-norm restoration steps and stays keep the configuration of
   the previous valid state (distance orientation, tangency side). Rules whose sign is
   part of the intent use signed residuals (signed angle, signed point–line distance,
   tangency side, internal tangency orientation) fixed at rule creation.
10. **Interactive previews** may set `analyze: false` to skip the final rank analysis
    (DOF/redundancy); commits keep it on.
11. **User intent and priority** (engine, `crates/engine/src/solve.rs`):
    - typed values are *exact* edits (hard rules of that transaction); drags and
      "prefer" edits are strong targets;
    - every variable has a stay multiplier: plugin properties declare
      `stay: low | normal | high` (a door offset slides before a door width changes),
      parameters of directly edited entities keep their values 5× more strongly;
    - weighted stays alone share a forced change between variables, so each solve
      first runs a **pinned attempt** in which the non-edited parameters of edited
      entities and `high`-stay properties are held fixed. The pinned result is used
      when it satisfies every hard rule *and* meets every target; otherwise a relaxed
      attempt (nothing pinned) runs, falling back to the pinned result if the relaxed
      one fails. This gives lexicographic priority without extreme weights.
    - locked entities (UI lock) and `solve: false` properties are constants.
