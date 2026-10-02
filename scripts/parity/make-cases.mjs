#!/usr/bin/env node
// Native/WASM parity cases (DL-TEST-6): documents and edits from the solver
// benchmark corpus plus hit-test, snap and export queries. The same cases run
// natively (crates/wasm/tests/parity.rs) and on the WebAssembly build
// (packages/sdk/test/parity.test.ts); both compare output digests with
// tests/fixtures/parity/expected.json.
//
//   node scripts/parity/make-cases.mjs   # rewrites tests/fixtures/parity/cases.json

import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { corpus } from '../bench/solver-corpus.mjs'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const cases = corpus().map((c) => ({
  name: c.name,
  types: c.types ?? [],
  setup: c.setup.map((commands) => ({ label: 'setup', commands })),
  edits: c.edits.slice(0, c.stress ? 3 : 6).map((commands, i) => ({ label: `edit ${i}`, commands })),
  // Model-space queries spread over the first entities' area.
  hit: [
    [0, 0, 20],
    [150, 40, 60],
    [400, 300, 25],
    [-500, 250, 80],
  ],
  snap: [
    { point: [10, 5], radius: 30 },
    { point: [205, 95], radius: 50, options: { grid: true, gridSpacing: 10 } },
  ],
}))
const out = resolve(root, 'tests/fixtures/parity/cases.json')
mkdirSync(dirname(out), { recursive: true })
writeFileSync(out, `${JSON.stringify(cases)}\n`)
console.log(`wrote ${cases.length} cases to ${out}`)
