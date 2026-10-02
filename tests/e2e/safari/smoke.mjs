#!/usr/bin/env node
// Real Safari smoke test (DL-TEST-13) over WebDriver (safaridriver), without
// dependencies. Playwright's WebKit is not Safari; this drives the installed Safari.
//
//   sudo safaridriver --enable && safaridriver -p 4444 &
//   node tests/e2e/safari/smoke.mjs --harness http://localhost:5199/ --playground http://localhost:5198/
//
// WebGL2 must work. WebGPU is checked too: if Safari does not expose it, the result is
// recorded as "not available" with the reason (never as passed). Writes
// results/safari.json and appends a Markdown summary to $GITHUB_STEP_SUMMARY.

import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`)
  return i >= 0 ? process.argv[i + 1] : fallback
}
const driver = arg('driver', 'http://localhost:4444')
const harness = arg('harness', 'http://localhost:5199/')
const playground = arg('playground', 'http://localhost:5198/')

async function wd(method, path, body) {
  const res = await fetch(`${driver}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const json = await res.json()
  if (!res.ok || json.value?.error) throw new Error(`${method} ${path}: ${JSON.stringify(json.value ?? json)}`)
  return json.value
}

/** Run an async page function; `arguments[0]` is the WebDriver callback. */
function script(fn) {
  return `const done = arguments[arguments.length - 1]; (${fn})().then(done, (e) => done({ ok: false, error: String(e && e.message || e) }))`
}

const session = await wd('POST', '/session', { capabilities: { alwaysMatch: { browserName: 'safari' } } })
const id = session.sessionId
const caps = session.capabilities ?? {}
const results = { browser: `Safari ${caps.browserVersion ?? '?'}`, platform: caps.platformName ?? '?', checks: [] }
let failed = false
try {
  await wd('POST', `/session/${id}/timeouts`, { script: 60_000, pageLoad: 60_000 })
  for (const backend of ['webgl2', 'webgpu']) {
    await wd('POST', `/session/${id}/url`, { url: `${harness}?backend=${backend}` })
    const r = await wd('POST', `/session/${id}/execute/async`, {
      script: script(`async () => {
        const r = await window.dl.ready
        if (!r.ok) return { ok: false, error: r.error }
        await window.dl.apply([
          { op: 'createEntity', entity: { geometry: { type: 'circle', center: [0, 0], radius: 40 } } },
          { op: 'createEntity', entity: { geometry: { type: 'line', a: [-60, 0], b: [60, 0] } } },
        ])
        const stats = await window.dl.nextFrame()
        const png = await window.dl.editor.viewport.exportPng()
        return { ok: true, backend: r.backend, items: stats.items, drawCalls: stats.drawCalls, png: png.size }
      }`),
      args: [],
    })
    const unavailable = backend === 'webgpu' && !r.ok && /webgpu|navigator\.gpu|getContext/i.test(r.error ?? '')
    const status = r.ok ? 'passed' : unavailable ? 'not available' : 'failed'
    if (status === 'failed' || (r.ok && (r.items !== 2 || !(r.png > 0)))) failed = true
    results.checks.push({ name: `harness ${backend}`, status, ...r })
  }
  // The React reference editor loads and its engine answers.
  await wd('POST', `/session/${id}/url`, { url: playground })
  const p = await wd('POST', `/session/${id}/execute/async`, {
    script: script(`async () => {
      for (let i = 0; i < 300 && !window.dotloom; i++) await new Promise((r) => setTimeout(r, 100))
      if (!window.dotloom) return { ok: false, error: 'playground did not start' }
      const doc = await window.dotloom.engine.documentJson()
      return { ok: true, entities: doc.entities.length, backend: window.dotloom.viewport?.backend ?? null }
    }`),
    args: [],
  })
  if (!p.ok) failed = true
  results.checks.push({ name: 'playground', status: p.ok ? 'passed' : 'failed', ...p })
} finally {
  await wd('DELETE', `/session/${id}`).catch(() => {})
}

const out = resolve(dirname(fileURLToPath(import.meta.url)), '../results')
mkdirSync(out, { recursive: true })
writeFileSync(resolve(out, 'safari.json'), `${JSON.stringify(results, null, 2)}\n`)
console.log(JSON.stringify(results, null, 2))
if (process.env.GITHUB_STEP_SUMMARY) {
  const rows = results.checks.map((c) => `| ${c.name} | ${c.status} | ${c.error ?? c.backend ?? ''} |`).join('\n')
  appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    `### ${results.browser} (${results.platform})\n\n| check | result | detail |\n|---|---|---|\n${rows}\n`,
  )
}
process.exit(failed ? 1 : 0)
