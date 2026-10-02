#!/usr/bin/env node
// Build the WebAssembly modules and their JS bindings for the SDK.
// Works on Windows (PowerShell/cmd), Linux and macOS: only Node, cargo and
// wasm-bindgen-cli are required.
//
//   node scripts/build-wasm.mjs           # optimized (profile wasm-release)
//   node scripts/build-wasm.mjs --dev     # fast debug build

import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const dev = process.argv.includes('--dev')
const profile = dev ? 'dev' : 'wasm-release'
const profileDir = dev ? 'debug' : 'wasm-release'
const only = process.argv.find((a) => a.startsWith('--only='))?.slice('--only='.length)

const targets = [
  { crate: 'dotloom-wasm', lib: 'dotloom_wasm', out: 'packages/sdk/src/wasm/engine' },
  { crate: 'dotloom-render-web', lib: 'dotloom_render_web', out: 'packages/sdk/src/wasm/render' },
]
  .filter((t) => existsSync(join(root, 'crates', t.crate === 'dotloom-wasm' ? 'wasm' : 'render-web')))
  .filter((t) => !only || t.crate === only)

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: root, ...opts })
  if (r.error) {
    console.error(`failed to start ${cmd}: ${r.error.message}`)
    process.exit(1)
  }
  if (r.status !== 0) process.exit(r.status ?? 1)
}

function lockedVersion(name) {
  const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8')
  const m = lock.match(new RegExp(`name = "${name}"\\r?\\nversion = "([^"]+)"`))
  return m ? m[1] : undefined
}

const want = lockedVersion('wasm-bindgen')
const have = spawnSync('wasm-bindgen', ['--version'], { encoding: 'utf8' })
if (have.error || have.status !== 0) {
  console.error(
    `wasm-bindgen CLI not found. Install it with: cargo install wasm-bindgen-cli --version ${want} --locked`,
  )
  process.exit(1)
}
const haveVersion = have.stdout.trim().split(/\s+/)[1]
if (haveVersion !== want) {
  console.error(`wasm-bindgen CLI ${haveVersion} does not match the crate version ${want} in Cargo.lock.`)
  console.error(`Install the matching CLI: cargo install wasm-bindgen-cli --version ${want} --locked`)
  process.exit(1)
}

for (const t of targets) {
  console.log(`\n▶ building ${t.crate} (${profile})`)
  run('cargo', ['build', '-p', t.crate, '--target', 'wasm32-unknown-unknown', '--profile', profile, '--locked'])
  const wasm = join(root, 'target', 'wasm32-unknown-unknown', profileDir, `${t.lib}.wasm`)
  const out = join(root, t.out)
  mkdirSync(out, { recursive: true })
  run('wasm-bindgen', ['--target', 'web', '--out-dir', out, '--out-name', t.lib, wasm])
  const size = statSync(join(out, `${t.lib}_bg.wasm`)).size
  console.log(`  ${t.lib}_bg.wasm: ${(size / 1024).toFixed(0)} KiB`)
}
