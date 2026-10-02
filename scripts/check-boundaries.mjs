#!/usr/bin/env node
// Dependency boundary check (DL-CORE-3): the core crates must build without any
// DOM, window or GPU dependency. Fails if a deny-listed crate appears in their
// normal dependency trees for any target and with all features enabled.
//
// The renderer is checked as a canary: it must be flagged (it depends on wgpu),
// which proves the check detects a violation instead of passing vacuously.
//
//   node scripts/check-boundaries.mjs

import { spawnSync } from 'node:child_process'

const CORE = [
  'dotloom-geometry',
  'dotloom-constraints',
  'dotloom-document',
  'dotloom-scene',
  'dotloom-engine',
  'dotloom-io',
]
const CANARY = 'dotloom-render'
const DENY = [
  'wgpu',
  'wgpu-core',
  'wgpu-hal',
  'naga',
  'web-sys',
  'js-sys',
  'wasm-bindgen',
  'winit',
  'raw-window-handle',
  'glow',
  'ash',
  'metal',
  'windows',
  'objc2',
]

function denied(crate) {
  const r = spawnSync(
    'cargo',
    [
      'tree',
      '-p',
      crate,
      '-e',
      'normal',
      '--all-features',
      '--target',
      'all',
      '--prefix',
      'none',
      '--format',
      '{p}',
      '--locked',
    ],
    { encoding: 'utf8' },
  )
  if (r.status !== 0) {
    console.error(r.stderr)
    process.exit(1)
  }
  const names = new Set(
    r.stdout
      .split('\n')
      .map((l) => l.trim().split(' ')[0])
      .filter(Boolean),
  )
  return { size: names.size, bad: DENY.filter((d) => names.has(d)) }
}

let failed = false
for (const crate of CORE) {
  const { size, bad } = denied(crate)
  if (bad.length > 0) {
    failed = true
    console.error(`✖ ${crate} depends on ${bad.join(', ')}`)
  } else {
    console.log(`✓ ${crate}: ${size} crates (all targets, all features), no DOM/window/GPU dependencies`)
  }
}
const canary = denied(CANARY)
if (canary.bad.length === 0) {
  failed = true
  console.error(`✖ canary ${CANARY} was not flagged: the check would miss a GPU dependency`)
} else {
  console.log(`✓ canary ${CANARY} is flagged (${canary.bad.join(', ')})`)
}
process.exit(failed ? 1 : 0)
