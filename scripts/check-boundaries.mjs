#!/usr/bin/env node
// Dependency boundary check (DL-CORE-3): the core crates must build without any
// DOM, window or GPU dependency. Fails if a deny-listed crate appears in their
// normal dependency trees.
//
//   node scripts/check-boundaries.mjs

import { spawnSync } from 'node:child_process'

const CORE = ['dotloom-geometry', 'dotloom-constraints', 'dotloom-document', 'dotloom-scene', 'dotloom-engine', 'dotloom-io']
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

let failed = false
for (const crate of CORE) {
  const r = spawnSync('cargo', ['tree', '-p', crate, '-e', 'normal', '--prefix', 'none', '--format', '{p}', '--locked'], {
    encoding: 'utf8',
  })
  if (r.status !== 0) {
    console.error(r.stderr)
    process.exit(1)
  }
  const names = new Set(r.stdout.split('\n').map((l) => l.trim().split(' ')[0]).filter(Boolean))
  const bad = DENY.filter((d) => names.has(d))
  if (bad.length > 0) {
    failed = true
    console.error(`✖ ${crate} depends on ${bad.join(', ')}`)
  } else {
    console.log(`✓ ${crate}: ${names.size} crates, no DOM/window/GPU dependencies`)
  }
}
process.exit(failed ? 1 : 0)
