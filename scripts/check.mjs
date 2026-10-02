#!/usr/bin/env node
// One entry point for formatting, linting and type checking — used locally and in CI.
//
//   pnpm run check            # everything
//   pnpm run check -- rust    # only Rust (fmt + clippy)
//   pnpm run check -- ts      # only TypeScript (biome + tsc)

import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const which = process.argv.slice(2).filter((a) => a !== '--')
const all = which.length === 0
const require = createRequire(join(root, 'package.json'))

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
  run('cargo clippy', 'cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'])
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
