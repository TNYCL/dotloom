#!/usr/bin/env node
// Verify a published GitHub Release from its public URLs (DL-OSS-5, DL-OSS-7):
// checksums, the npm tarballs installed by URL into a fresh project outside the
// repository and used from Node, and this platform's CLI archive.
//
//   node scripts/verify-release.mjs v1.0.0 [--work <dir outside the repo>]

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const tag = process.argv[2]
if (!tag?.startsWith('v')) {
  console.error('usage: verify-release.mjs vX.Y.Z [--work <dir>]')
  process.exit(2)
}
const version = tag.slice(1)
const i = process.argv.indexOf('--work')
const work = i >= 0 ? resolve(process.argv[i + 1]) : mkdtempSync(join(tmpdir(), 'dotloom-release-'))
if (!relative(root, work).startsWith('..')) {
  console.error(`✖ the work directory must be outside the repository: ${work}`)
  process.exit(1)
}
rmSync(work, { recursive: true, force: true })
mkdirSync(work, { recursive: true })
const base = `https://github.com/TNYCL/dotloom/releases/download/${tag}`

function run(cmd, args, cwd, opts = {}) {
  console.log(`\n▶ ${cmd} ${args.join(' ')}`)
  const r = spawnSync(cmd, args, {
    cwd,
    stdio: opts.capture ? 'pipe' : 'inherit',
    encoding: 'utf8',
    // npm is a .cmd shim on Windows; everything else runs without a shell (paths
    // with spaces stay intact).
    shell: process.platform === 'win32' && cmd === 'npm',
  })
  if (r.status !== 0) {
    console.error(r.stderr ?? '')
    console.error(`✖ ${cmd} failed`)
    process.exit(1)
  }
  return r.stdout ?? ''
}
async function get(name) {
  const r = await fetch(`${base}/${name}`)
  if (!r.ok) throw new Error(`${name}: HTTP ${r.status}`)
  const bytes = Buffer.from(await r.arrayBuffer())
  writeFileSync(join(work, name), bytes)
  return bytes
}

// 1. Checksums of every listed asset we use.
const sums = Object.fromEntries(
  (await get('SHA256SUMS'))
    .toString('utf8')
    .trim()
    .split('\n')
    .map((l) => l.trim().split(/\s+\*?/))
    .map(([h, f]) => [f, h]),
)
const cli =
  process.platform === 'win32'
    ? `dotloom-${version}-x86_64-pc-windows-msvc.zip`
    : process.platform === 'darwin'
      ? `dotloom-${version}-aarch64-apple-darwin.tar.gz`
      : `dotloom-${version}-x86_64-unknown-linux-gnu.tar.gz`
for (const name of [`dotloom-sdk-${version}.tgz`, `dotloom-react-${version}.tgz`, cli]) {
  const bytes = await get(name)
  const h = createHash('sha256').update(bytes).digest('hex')
  if (sums[name] !== h) {
    console.error(`✖ ${name}: SHA-256 ${h} does not match SHA256SUMS (${sums[name]})`)
    process.exit(1)
  }
  console.log(`✓ ${name} ${h}`)
}
// The manifest is written after SHA256SUMS; it must name the tag's version.
await get('release-manifest.json')
const manifest = JSON.parse(readFileSync(join(work, 'release-manifest.json'), 'utf8'))
console.log('release manifest:', JSON.stringify({ version: manifest.version, commit: manifest.commit }))
if (manifest.version !== version) {
  console.error('✖ the manifest version does not match the tag')
  process.exit(1)
}

// 2. npm: install the tarballs by URL into a fresh project and use the engine.
const app = join(work, 'consumer')
mkdirSync(app)
writeFileSync(
  join(app, 'package.json'),
  `${JSON.stringify({ name: 'dotloom-release-consumer', private: true, type: 'module' }, null, 2)}\n`,
)
run(
  'npm',
  [
    'install',
    '--no-audit',
    '--no-fund',
    '--loglevel=error',
    `${base}/dotloom-sdk-${version}.tgz`,
    `${base}/dotloom-react-${version}.tgz`,
    'react@19',
    'react-dom@19',
  ],
  app,
)
writeFileSync(
  join(app, 'check.mjs'),
  `import { SDK_VERSION } from '@dotloom/sdk'
import { createNodeEngine } from '@dotloom/sdk/node'
import * as react from '@dotloom/react'
if (SDK_VERSION !== ${JSON.stringify(version)}) throw new Error('SDK_VERSION ' + SDK_VERSION)
if (typeof react.DotloomEditor !== 'function') throw new Error('no DotloomEditor export')
const e = await createNodeEngine()
const r = await e.apply([{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [90, 0] } } }])
const id = r.created[0]
await e.apply([{ op: 'addConstraint', constraint: { rule: { kind: 'length', line: { from: { entity: id, anchor: 'start' }, to: { entity: id, anchor: 'end' } }, value: 100 } } }])
const bytes = await e.save()
const e2 = await createNodeEngine()
await e2.load(bytes)
const info = await e2.entityInfo(id)
const [a, b] = ['start', 'end'].map((n) => info.anchors.find((x) => x.name === n).point)
const len = Math.hypot(b[0] - a[0], b[1] - a[1])
if (Math.abs(len - 100) > 1e-6) throw new Error('length ' + len)
console.log('npm consumer ok: engine ' + e.capabilities.engineVersion + ', .dotl ' + bytes.length + ' bytes, length ' + len)
e.dispose(); e2.dispose()
`,
)
run(process.execPath, ['check.mjs'], app)

// 3. CLI archive for this platform.
const cliDir = join(work, 'cli')
mkdirSync(cliDir)
if (cli.endsWith('.zip')) {
  run('powershell', ['-NoProfile', '-Command', `Expand-Archive -Force '${join(work, cli)}' '${cliDir}'`], work)
} else {
  run('tar', ['-xzf', join(work, cli), '-C', cliDir], work)
}
const folder = join(cliDir, readdirSync(cliDir)[0] ?? '')
const files = readdirSync(folder)
for (const f of ['LICENSE-MIT', 'LICENSE-APACHE', 'THIRD-PARTY-NOTICES.md', 'README.md']) {
  if (!files.includes(f)) {
    console.error(`✖ the CLI archive lacks ${f}`)
    process.exit(1)
  }
}
const exe = join(folder, process.platform === 'win32' ? 'dotloom.exe' : 'dotloom')
const out = run(exe, ['--version'], folder, { capture: true })
if (!out.includes(version)) {
  console.error(`✖ dotloom --version printed ${out}`)
  process.exit(1)
}
const inspect = run(exe, ['--json', 'inspect', join(root, 'tests/fixtures/dotl/drawing-0.1.0.dotl')], folder, {
  capture: true,
})
const report = JSON.parse(inspect)
console.log(`CLI ok: ${out.trim()}; inspect: ${JSON.stringify(report).slice(0, 160)}…`)
console.log(`\n✓ ${tag} verified from its public URLs in ${work}`)
