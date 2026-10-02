#!/usr/bin/env node
// Build a TypeScript package: tsc → dist/, then copy generated WASM bindings and
// license files so the published tarball is self-contained.
//
//   node scripts/build-package.mjs sdk|react

import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const name = process.argv[2]
if (!name) {
  console.error('usage: build-package.mjs <package-dir-name>')
  process.exit(2)
}
const pkg = join(root, 'packages', name)
const dist = join(pkg, 'dist')
rmSync(dist, { recursive: true, force: true })

const require = createRequire(join(pkg, 'package.json'))
const tsc = require.resolve('typescript/bin/tsc')
const r = spawnSync(process.execPath, [tsc, '-p', join(pkg, 'tsconfig.json')], { stdio: 'inherit', cwd: pkg })
if (r.status !== 0) process.exit(r.status ?? 1)

const wasm = join(pkg, 'src', 'wasm')
if (existsSync(wasm)) {
  mkdirSync(join(dist, 'wasm'), { recursive: true })
  cpSync(wasm, join(dist, 'wasm'), { recursive: true, filter: (p) => !p.endsWith('.gitignore') })
}
const css = join(pkg, 'src', 'styles.css')
if (existsSync(css)) cpSync(css, join(dist, 'styles.css'))
for (const f of ['LICENSE-MIT', 'LICENSE-APACHE']) cpSync(join(root, f), join(pkg, f))
console.log(`built packages/${name}`)
