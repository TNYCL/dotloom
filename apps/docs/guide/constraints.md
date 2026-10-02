# Constraints

Rules relate numeric **parameters** (`{ entity, prop }` for plugin properties,
`{ entity, geom }` for built-in geometry such as `a.x` or `radius`) and **anchors**
(`{ entity, anchor }`: `start`, `end`, `mid`, `center`, plugin anchors).

## Rule kinds

| Kind | Meaning |
|---|---|
| `fix`, `fixPoint` | a parameter or anchor has a value (a "lock") |
| `equal`, `allEqual`, `ratio`, `equalSpacing` | relations between parameters |
| `linear` | `Σ coef·param (= | <= | >=) rhs` — sums, differences, minimum gaps |
| `coincident`, `horizontal`, `vertical`, `distance` | anchors |
| `pointOnLine`, `pointLineDistance`, `pointOnCircle` | incidence |
| `length`, `equalLength`, `parallel`, `perpendicular`, `angle` | lines |
| `concentric`, `radius`, `equalRadius`, `tangentLineCircle`, `tangentCircles` | circles and arcs |
| `expression` | a typed expression on one entity (`lhs op rhs`) |

Plugin types add their own rules (templates) that apply to every instance.
Unsupported equation classes are reported as `unsupported` — never silently ignored.

## Strengths and priorities

`required` rules are hard: a commit never violates them. `strong`, `medium` and
`weak` rules are preferences, satisfied in that order as far as the hard rules
allow. Parameters also have a **stay** priority (`low`, `normal`, `high`) that says
which values should change first when something must give — a door's offset (low)
slides before its width (high).

## How solving works

The engine builds a graph of variables and rules and solves only the connected
component affected by a change:

- purely linear components use an incremental Cassowary solver (`kasuari`) with
  scaled rows and deterministic tie-breaking;
- nonlinear components use a sequential quadratic programming (SQP) solver with
  exact gradients, constraint curvature, sparse linear algebra, presolve elimination
  and an active set for inequalities. A typed value or hard drag target far from the
  current geometry is first approached along the constraint manifold and then
  enforced exactly, so linkages move continuously instead of flipping to another
  branch.

Every commit reports what the solver did in `CommitReport.solver` (iterations,
attempts, variables, rules, components).

Hard rules are constraints, not penalties. After solving, an independent checker
re-evaluates every hard rule on the values that would be stored; only then is the
change committed.

## Results

| Status | Meaning |
|---|---|
| `solved` | every rule holds; the component is fully determined |
| `underconstrained` | every rule holds; `dof` degrees of freedom remain |
| `conflicting` | the hard rules cannot hold together (certain or suspected) |
| `notConverged` | the numeric solver did not reach the tolerance in its budget |
| `cancelled` | the request was cancelled |
| `unsupported` | a rule class is not supported |

Diagnostics name the rules (IDs, labels, plugin templates), the edited parameters
and the entities involved. User locks are never removed automatically. For linear
conflicts, `nearest` gives the closest feasible value of each requested edit — the
shelf example explains that 130 cm is impossible and offers 140 cm.

## Tolerances

Four tolerances are kept apart: the **model** tolerance (1 nm absolute, for geometric
comparisons), the **solver** tolerance (rule residuals, scaled per dimension), the
**flatten** tolerance (¼ device pixel for drawing curves) and the **screen**
tolerance (pick and snap radii in CSS pixels, converted with the current zoom).

## Cancellation

```ts
const ctrl = new AbortController()
const p = engine.apply(tx, { signal: ctrl.signal })
ctrl.abort() // takes effect between solver steps; the document stays unchanged
```
