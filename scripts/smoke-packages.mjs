#!/usr/bin/env node
// Package consumer smoke test (DL-PLUGIN-6, DL-TEST-11).
//
// 1. Pack @dotloom/sdk and @dotloom/react exactly as they would be published.
// 2. Copy examples/external-plugin to a temporary directory OUTSIDE the repository.
// 3. Install the tarballs with npm (no workspace, no source aliases).
// 4. Check the tarball contents, run the plugin's Node tests, build its browser app.
// 5. Optionally (--serve <port>) serve the built app for the browser check.
//
//   node scripts/smoke-packages.mjs [--keep] [--work <dir outside the repo>] [--serve 5201]

import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const keep = process.argv.includes('--keep')
const serveAt = (() => {
  const i = process.argv.indexOf('--serve')
  return i >= 0 ? Number(process.argv[i + 1]) : null
})()
const win = process.platform === 'win32'

function run(cmd, args, cwd, env = process.env) {
  console.log(`\n▶ (${relative(root, cwd) || '.'}) ${cmd} ${args.join(' ')}`)
  const r = spawnSync(cmd, args, { stdio: 'inherit', cwd, env, shell: win })
  if (r.error || r.status !== 0) {
    console.error(`✖ ${cmd} ${args.join(' ')} failed`)
    process.exit(r.status ?? 1)
  }
}

const workArg = (() => {
  const i = process.argv.indexOf('--work')
  return i >= 0 ? process.argv[i + 1] : undefined
})()
const work = workArg ?? mkdtempSync(join(tmpdir(), 'dotloom-smoke-'))
if (workArg) {
  rmSync(work, { recursive: true, force: true })
  mkdirSync(work, { recursive: true })
}
if (!relative(root, work).startsWith('..')) {
  console.error('temporary directory must be outside the repository')
  process.exit(1)
}
const packs = join(work, 'packs')
mkdirSync(packs)

// 1. Pack.
for (const pkg of ['sdk', 'react']) {
  run('pnpm', ['pack', '--pack-destination', packs], join(root, 'packages', pkg))
}
const tarballs = Object.fromEntries(
  readdirSync(packs).map((f) => [f.startsWith('dotloom-react') ? '@dotloom/react' : '@dotloom/sdk', join(packs, f)]),
)
console.log('tarballs:', tarballs)

// Tarball contents: built JS, types, WASM, worker, licenses; no sources of tests.
// Relative name: GNU tar reads "C:..." as a remote host.
const sdkTgz = readdirSync(packs).find((f) => f.startsWith('dotloom-sdk')) ?? ''
const listing = spawnSync('tar', ['-tzf', sdkTgz], { encoding: 'utf8', cwd: packs }).stdout ?? ''
for (const required of [
  'package/dist/index.js',
  'package/dist/index.d.ts',
  'package/dist/worker.js',
  'package/dist/node.js',
  'package/dist/wasm/engine/dotloom_wasm_bg.wasm',
  'package/dist/wasm/render/dotloom_render_web_bg.wasm',
  'package/LICENSE-MIT',
  'package/LICENSE-APACHE',
  'package/THIRD-PARTY-NOTICES.md',
  'package/README.md',
]) {
  if (!listing.includes(required)) {
    console.error(`✖ @dotloom/sdk tarball lacks ${required}`)
    process.exit(1)
  }
}
if (/package\/test\//.test(listing)) {
  console.error('✖ @dotloom/sdk tarball contains tests')
  process.exit(1)
}

// 2. Copy the example out of the repository.
const app = join(work, 'external-plugin')
cpSync(join(root, 'examples', 'external-plugin'), app, {
  recursive: true,
  filter: (p) => !p.includes('node_modules') && !p.endsWith('dist'),
})

// 3. Point the dependencies at the tarballs (overrides also cover @dotloom/react's own dependency).
const pkgPath = join(app, 'package.json')
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'))
for (const [name, file] of Object.entries(tarballs)) pkg.dependencies[name] = `file:${file}`
pkg.overrides = { '@dotloom/sdk': `file:${tarballs['@dotloom/sdk']}` }
writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`)
run('npm', ['install', '--no-audit', '--no-fund', '--loglevel=error'], app)

// No workspace leaks: installed packages must come from the tarballs.
const installed = JSON.parse(readFileSync(join(app, 'node_modules', '@dotloom', 'sdk', 'package.json'), 'utf8'))
const expectedVersion = JSON.parse(readFileSync(join(root, 'packages', 'sdk', 'package.json'), 'utf8')).version
if (installed.version !== expectedVersion || existsSync(join(app, 'node_modules', '@dotloom', 'sdk', 'src', 'wasm'))) {
  console.error('✖ unexpected @dotloom/sdk installation')
  process.exit(1)
}

// 4. Test and build.
run('npm', ['test'], app)
run('npm', ['run', 'build'], app)
// The same app built for an absolute sub-path (GitHub Pages style): worker, WASM and
// font assets must resolve under /dotloom/ too (DL-SDK-10).
run('npx', ['vite', 'build', '--base', '/dotloom/', '--outDir', 'dist-subpath'], app)
console.log(`\n✓ external plugin built and tested from packed tarballs in ${app}`)

if (serveAt) {
  run(
    'node',
    [join(root, 'scripts', 'serve-site.mjs'), '--root', join(app, 'dist'), '--base', '/', '--port', String(serveAt)],
    app,
  )
} else if (!keep) {
  rmSync(work, { recursive: true, force: true })
}
