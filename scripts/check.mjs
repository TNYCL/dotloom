#!/usr/bin/env node
// One entry point for formatting, linting and type checking — used locally and in CI.
//
//   pnpm run check            # everything
//   pnpm run check -- rust    # only Rust (fmt, dependency boundaries, clippy, wasm32)
//   pnpm run check -- ts      # only TypeScript (biome + tsc)

import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const which = process.argv.slice(2).filter((a) => a !== '--')
const all = which.length === 0
const require = createRequire(join(root, 'package.json'))
const CORE_CRATES = [
  'dotloom-geometry',
  'dotloom-constraints',
  'dotloom-document',
  'dotloom-scene',
  'dotloom-engine',
  'dotloom-io',
]

let failed = false
function run(label, cmd, args, cwd = root) {
  console.log(`\n▶ ${label}`)
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd })
  if (r.error || r.status !== 0) {
    console.error(`✖ ${label} failed`)
    failed = true
  }
}

if (all || which.includes('rust')) {
  run('cargo fmt --check', 'cargo', ['fmt', '--all', '--', '--check'])
  run('dependency boundaries', process.execPath, [join(root, 'scripts', 'check-boundaries.mjs')])
  run('third-party notices', process.execPath, [join(root, 'scripts', 'third-party-notices.mjs'), '--check'])
  // Published crates carry the license texts (crates.io packages cannot reach the root).
  console.log('
▶ crate license files')
  for (const name of readdirSync(join(root, 'crates'))) {
    const manifest = readFileSync(join(root, 'crates', name, 'Cargo.toml'), 'utf8')
    if (/^publish\s*=\s*false/m.test(manifest)) continue
    for (const lic of ['LICENSE-MIT', 'LICENSE-APACHE']) {
      const copy = join(root, 'crates', name, lic)
      if (!existsSync(copy) || readFileSync(copy, 'utf8') !== readFileSync(join(root, lic), 'utf8')) {
        console.error(`✖ crates/${name}/${lic} is missing or differs from the root copy`)
        failed = true
      }
    }
  }
  run('cargo clippy', 'cargo', [
    'clippy',
    '--workspace',
    '--all-targets',
    '--all-features',
    '--locked',
    '--',
    '-D',
    'warnings',
  ])
  run('cargo clippy (wasm32 bindings)', 'cargo', [
    'clippy',
    '-p',
    'dotloom-wasm',
    '-p',
    'dotloom-render-web',
    '--target',
    'wasm32-unknown-unknown',
    '--locked',
    '--',
    '-D',
    'warnings',
  ])
  // The core crates also build for a target without OS, window or GPU (DL-CORE-1).
  run('cargo check (core crates, wasm32 without bindings)', 'cargo', [
    'check',
    ...CORE_CRATES.flatMap((c) => ['-p', c]),
    '--target',
    'wasm32-unknown-unknown',
    '--locked',
  ])
}

if (all || which.includes('ts')) {
  const biome = require.resolve('@biomejs/biome/bin/biome')
  run('biome check', process.execPath, [biome, 'check', '.'])
  const tsc = require.resolve('typescript/bin/tsc')
  for (const dir of ['packages', 'apps', 'examples', 'tests']) {
    const base = join(root, dir)
    if (!existsSync(base)) continue
    for (const name of readdirSync(base)) {
      // The test config maps workspace packages to their sources, so the check
      // does not depend on build order; build configs are exercised by `pnpm build`.
      const test = join(base, name, 'tsconfig.test.json')
      const main = join(base, name, 'tsconfig.json')
      const cfg = existsSync(test) ? test : existsSync(main) ? main : null
      if (cfg)
        run(`tsc ${dir}/${name}/${cfg === test ? 'tsconfig.test.json' : 'tsconfig.json'}`, process.execPath, [
          tsc,
          '-p',
          cfg,
          '--noEmit',
        ])
    }
  }
}

process.exit(failed ? 1 : 0)
