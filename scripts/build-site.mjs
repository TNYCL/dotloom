#!/usr/bin/env node
// Build the GitHub Pages site into site/ exactly as it is served under a sub-path:
//
//   site/                 docs (VitePress)
//   site/api/ts/          TypeScript API (TypeDoc)
//   site/api/rust/        Rust API (rustdoc)
//   site/playground/      reference React editor
//   site/examples/<name>/ example apps
//
//   node scripts/build-site.mjs [--base /dotloom/] [--skip-rustdoc]
//
// Requires the WASM modules (node scripts/build-wasm.mjs) and built packages (pnpm build).

import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const arg = (name, fallback) => {
  const i = process.argv.indexOf(name)
  return i >= 0 ? process.argv[i + 1] : fallback
}
let base = arg('--base', '/dotloom/')
if (!base.endsWith('/')) base += '/'
const skipRustdoc = process.argv.includes('--skip-rustdoc')
const site = join(root, 'site')
const docs = join(root, 'apps', 'docs')
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'

function run(cmd, args, opts = {}) {
  console.log(`\n▶ ${cmd} ${args.join(' ')}`)
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd: root, shell: process.platform === 'win32', ...opts })
  if (r.error || r.status !== 0) {
    console.error(`✖ ${cmd} failed`)
    process.exit(r.status ?? 1)
  }
}

for (const f of ['packages/sdk/dist/index.js', 'packages/react/dist/index.js', 'packages/sdk/src/wasm/engine']) {
  if (!existsSync(join(root, f))) {
    console.error(`missing ${f}: run node scripts/build-wasm.mjs && pnpm build first`)
    process.exit(1)
  }
}

rmSync(site, { recursive: true, force: true })
const publicApi = join(docs, 'public', 'api')
rmSync(publicApi, { recursive: true, force: true })

// TypeScript API.
run(pnpm, ['--filter', '@dotloom/docs', 'exec', 'typedoc'])

// Rust API (published crates only).
if (!skipRustdoc) {
  const crates = ['geometry', 'constraints', 'document', 'scene', 'engine', 'io', 'render']
  run('cargo', ['doc', '--no-deps', '--locked', '-j', '4', ...crates.flatMap((c) => ['-p', `dotloom-${c}`])])
  mkdirSync(join(publicApi, 'rust'), { recursive: true })
  cpSync(join(root, 'target', 'doc'), join(publicApi, 'rust'), { recursive: true })
}

// Docs.
run(pnpm, ['--filter', '@dotloom/docs', 'run', 'build'], { env: { ...process.env, DOTLOOM_BASE: base } })
cpSync(join(docs, '.vitepress', 'dist'), site, { recursive: true })

// Playground and examples.
const apps = [
  ['@dotloom/playground', 'playground'],
  ['@dotloom/example-vanilla', 'examples/vanilla'],
  ['@dotloom/example-shelf-configurator', 'examples/shelf-configurator'],
  ['@dotloom/example-floorplan', 'examples/floorplan'],
  ['@dotloom/example-timeline', 'examples/timeline'],
]
for (const [pkg, path] of apps) {
  const out = join(site, ...path.split('/'))
  run(pnpm, ['--filter', pkg, 'exec', 'vite', 'build', '--outDir', out, '--emptyOutDir'], {
    env: { ...process.env, DOTLOOM_BASE: `${base}${path}/` },
  })
}

writeFileSync(join(site, '.nojekyll'), '')
console.log(`\nsite built in ${site} (base ${base})`)
