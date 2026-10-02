#!/usr/bin/env node
// Solver benchmark (DL-PERF-4) on the engine's WebAssembly build through the SDK
// (Node, in-thread transport: includes protocol and JSON costs).
//
//   node scripts/bench/solver.mjs [--json out.json]
//
// Requires packages/sdk/dist (pnpm build).

import { writeFileSync } from 'node:fs'
import { performance } from 'node:perf_hooks'
import { createNodeEngine } from '../../packages/sdk/dist/node.js'
import { corpus } from './solver-corpus.mjs'

const PARAMS = { line: 4, circle: 3, arc: 5, rect: 4 }

function percentile(xs, p) {
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))] ?? Number.NaN
}

const rows = []
const gated = []
const stress = []
for (const c of corpus()) {
  const e = await createNodeEngine()
  if (c.types) await e.registerTypes(c.types, 'bench')
  const t0 = performance.now()
  for (const s of c.setup) await e.apply(s)
  const setupMs = performance.now() - t0
  const doc = await e.documentJson()
  let vars = 0
  for (const ent of doc.entities) {
    if (ent.geometry) vars += PARAMS[ent.geometry.type] ?? 0
    else vars += Object.values(ent.props ?? {}).filter((v) => typeof v === 'number').length
  }
  const times = []
  let rejected = 0
  let violations = (await e.verify()).length
  for (const edit of c.edits) {
    const t = performance.now()
    try {
      await e.apply(edit)
    } catch {
      rejected++
    }
    times.push(performance.now() - t)
    violations += (await e.verify()).length
  }
  ;(c.stress ? stress : gated).push(...times)
  rows.push({
    name: c.stress ? `${c.name} (stress)` : c.name,
    vars,
    rules: doc.constraints.length,
    edits: times.length,
    rejected,
    violations,
    setupMs: Math.round(setupMs),
    p50: +percentile(times, 50).toFixed(2),
    p95: +percentile(times, 95).toFixed(2),
    max: +Math.max(...times).toFixed(2),
  })
  e.dispose()
}
console.table(rows)
const summarize = (xs) => ({
  p50: +percentile(xs, 50).toFixed(2),
  p95: +percentile(xs, 95).toFixed(2),
  edits: xs.length,
})
const summary = { gated: summarize(gated), stress: summarize(stress) }
console.log('gated', summary.gated)
console.log('stress', summary.stress)
const out = (() => {
  const i = process.argv.indexOf('--json')
  return i >= 0 ? process.argv[i + 1] : null
})()
if (out)
  writeFileSync(
    out,
    `${JSON.stringify({ rows, summary, node: process.version, date: new Date().toISOString() }, null, 2)}\n`,
  )
