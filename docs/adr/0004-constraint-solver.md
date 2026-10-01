# ADR-0004: Constraint solver architecture

Status: accepted (2026-10-02)

## Decision

1. **Variables** are scalar parameters: entity geometry parameters (line endpoints,
   circle radius, ...) and numeric entity properties (shelf widths, block start times).
   Anchors are expressions of variables through the entity transform.
2. **Graph and components.** Constraints and variables form a bipartite graph. Each
   connected component is classified once per solve:
   - *linear*: every enabled constraint is linear → **kasuari** (maintained Cassowary
     implementation, MIT/Apache-2.0). Required constraints are hard; preferences and
     drag/stay targets use `strong/medium/weak`. Edit variables make drags incremental.
   - *nonlinear/mixed*: the whole component goes to the numeric backend; its linear
     constraints become exact linear rows. A variable is owned by exactly one backend
     per solve, so backends never fight over it.
3. **Numeric backend** = hierarchical (lexicographic) damped Gauss–Newton:
   - level 0: hard equalities + active hard inequalities, solved as a minimum-norm
     step `J₀ Δ = −r₀` (SVD via nalgebra);
   - lower levels (drag target, preferences, stay-near-previous) are least-squares
     objectives solved in the **null space** of the higher levels, so a soft objective
     never trades off a hard residual;
   - residuals are scaled per rule; analytic Jacobians are implemented per rule and
     checked against central differences in tests;
   - inequalities: active set (violated or binding inequalities become rows at their
     bound; released when inactive);
   - warm start from the previous valid solution, step-length limiting, explicit stop
     criteria (`‖r_hard‖∞ ≤ tol`, small step, iteration budget).
4. **Fixed variables** (locked parameters, `fix` rules) are eliminated from the
   unknown vector.
5. **Status**: `solved` (no remaining DOF), `underconstrained { dof }` (valid and
   usable), `conflicting { evidence }` (proven: kasuari required-failure for linear
   components, or redundant inconsistent rows detected by rank analysis),
   `notConverged { suspectedConflict }` (local minimum of the hard residual or budget
   exhausted — never presented as proof), `cancelled`, `unsupported { rule }`.
6. **Commit rule**: after solving, the engine re-evaluates every hard rule with an
   independent evaluator; if any exceeds tolerance the transaction is rejected and the
   document stays at the previous revision.
7. **Explanation**: on linear conflicts the engine computes the nearest feasible value
   of the edited variable by re-solving with the user's value demoted to `strong`
   (shelf total 130 cm → feasible bound 140 cm). Conflict sets are computed only on the
   failure path with a budget, never on every drag.
8. **Cancellation**: solves are resumable state machines (`SolveJob::step(budget)`).
9. **Branches**: geometric rules with two solutions (tangency side, angle direction,
   distance orientation) keep the branch of the starting configuration because the
   solver takes minimum-norm steps from the previous valid state; signed residuals
   (e.g. signed angle) are used where the sign is part of the user's intent.
