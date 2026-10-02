#!/usr/bin/env node
// Real-browser smoke test over W3C WebDriver (DL-TEST-13), without dependencies.
// Playwright's builds are engine-family tests; this drives the real, released
// browsers: Safari (safaridriver), Chrome (chromedriver), Edge (msedgedriver) and
// Firefox (geckodriver).
//
//   # Safari: sudo safaridriver --enable && safaridriver -p 4444 &
//   node tests/e2e/webdriver/smoke.mjs --browser safari
//   # Chrome/Edge/Firefox from install.mjs (spawns the driver itself):
//   node tests/e2e/webdriver/smoke.mjs --info browsers/chrome-previous/browser.json [--headless]
//
// Options: --harness <url> (default http://localhost:5199/), --playground <url>
// (default http://localhost:5198/), --driver <url> (default http://localhost:4444),
// --arg <browser argument> (repeatable), --webgpu required|optional (default optional),
// --canvas-readback required|optional (default required).
//
// WebGL2 must work. WebGPU is checked too; with `optional` a failure is recorded as
// "not available" with the reason (never as passed).
//
// PNG export reads the canvas back. A control canvas (plain 2D, no Dotloom) shows
// whether the browser environment can read back canvases at all; some virtual
// machines cannot (Safari 27 on the macOS 27 runner). With `--canvas-readback
// optional` and a failed control, the drawn pixels are read with WebGL `readPixels`
// instead and PNG export is recorded as "not verifiable here" — never as passed. Writes
// results/webdriver-<browser>-<major>.json and appends a Markdown summary to
// $GITHUB_STEP_SUMMARY.

import { spawn } from 'node:child_process'
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const argv = process.argv.slice(2)
const arg = (name, fallback) => {
  const i = argv.indexOf(`--${name}`)
  return i >= 0 ? argv[i + 1] : fallback
}
const many = (name) => argv.flatMap((a, i) => (a === `--${name}` && argv[i + 1] ? [argv[i + 1]] : []))
const info = arg('info') ? JSON.parse(readFileSync(arg('info'), 'utf8')) : null
const browser = info?.browser ?? arg('browser', 'safari')
const port = Number(arg('port', '4444'))
const driverUrl = arg('driver', `http://localhost:${port}`)
const harness = arg('harness', 'http://localhost:5199/')
const playground = arg('playground', 'http://localhost:5198/')
const webgpuPolicy = arg('webgpu', 'optional')
const readbackPolicy = arg('canvas-readback', 'required')
const headless = argv.includes('--headless')
const extraArgs = many('arg')

async function wd(method, path, body) {
  const res = await fetch(`${driverUrl}${path}`, {
    method,
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  const json = await res.json()
  // WebDriver errors come with a non-2xx status; page results may carry an `error` field.
  if (!res.ok) throw new Error(`${method} ${path}: ${JSON.stringify(json.value ?? json)}`)
  return json.value
}

/** Run an async page function; `arguments[0]` is the WebDriver callback. */
function script(fn) {
  return `const done = arguments[arguments.length - 1]; (${fn})().then(done, (e) => done({ ok: false, error: String(e && e.message || e) }))`
}

function capabilities() {
  switch (browser) {
    case 'safari':
      return { browserName: 'safari' }
    case 'chrome':
    case 'msedge': {
      const args = [
        ...(headless ? ['--headless=new'] : []),
        '--no-first-run',
        '--no-default-browser-check',
        ...extraArgs,
      ]
      const opts = { args, ...(info?.binary ? { binary: info.binary } : {}) }
      return browser === 'chrome'
        ? { browserName: 'chrome', 'goog:chromeOptions': opts }
        : { browserName: 'MicrosoftEdge', 'ms:edgeOptions': opts }
    }
    case 'firefox':
      return {
        browserName: 'firefox',
        'moz:firefoxOptions': {
          ...(info?.binary ? { binary: info.binary } : {}),
          args: [...(headless ? ['-headless'] : []), ...extraArgs],
          // Allow software WebGL on GPU-less runners; WebGPU as shipped by the release.
          prefs: { 'webgl.force-enabled': true, 'webgl.disabled': false },
        },
      }
    default:
      throw new Error(`unknown browser ${browser}`)
  }
}

// Start the WebDriver server when its binary is known.
let driverProc = null
if (info?.driver) {
  const portArgs = browser === 'firefox' ? ['--port', String(port)] : [`--port=${port}`]
  driverProc = spawn(info.driver, portArgs, { stdio: ['ignore', 'inherit', 'inherit'] })
  for (let i = 0; ; i++) {
    const ready = await fetch(`${driverUrl}/status`)
      .then((r) => r.json())
      .then((j) => j.value?.ready === true)
      .catch(() => false)
    if (ready) break
    if (i > 100) throw new Error(`${info.driver} did not start`)
    await new Promise((r) => setTimeout(r, 200))
  }
}

const session = await wd('POST', '/session', { capabilities: { alwaysMatch: capabilities() } })
const id = session.sessionId
const caps = session.capabilities ?? {}
const version = String(caps.browserVersion ?? info?.version ?? '?')
const label = { safari: 'Safari', chrome: 'Chrome', msedge: 'Edge', firefox: 'Firefox' }[browser]
const results = {
  browser: `${label} ${version}`,
  channel: info?.channel ?? null,
  platform: String(caps.platformName ?? process.platform),
  checks: [],
}
let failed = false
try {
  await wd('POST', `/session/${id}/timeouts`, { script: 90_000, pageLoad: 60_000 })
  await wd('POST', `/session/${id}/url`, { url: harness })
  const control = await wd('POST', `/session/${id}/execute/async`, {
    script: script(`async () => {
      const c = document.createElement('canvas')
      c.width = 16
      c.height = 16
      const g = c.getContext('2d')
      g.fillStyle = '#ff0000'
      g.fillRect(0, 0, 16, 16)
      try {
        const d = g.getImageData(8, 8, 1, 1).data
        return d[0] === 255 && d[3] === 255 ? { ok: true } : { ok: false, error: 'pixel ' + Array.from(d).join(',') }
      } catch (e) {
        return { ok: false, error: e.name + ': ' + e.message }
      }
    }`),
    args: [],
  })
  const readbackWorks = control.ok === true
  const fallback = !readbackWorks && readbackPolicy === 'optional'
  if (!readbackWorks && !fallback) failed = true
  results.checks.push({
    name: 'canvas readback (control, no Dotloom)',
    status: readbackWorks ? 'passed' : fallback ? 'unavailable in this environment' : 'failed',
    ...control,
  })
  for (const backend of ['webgl2', 'webgpu']) {
    await wd('POST', `/session/${id}/url`, { url: `${harness}?backend=${backend}` })
    const r = await wd('POST', `/session/${id}/execute/async`, {
      script: script(`async () => {
        for (let i = 0; i < 300 && !window.dl; i++) await new Promise((r) => setTimeout(r, 100))
        const r = await window.dl.ready
        if (!r.ok) return { ok: false, error: r.error }
        await window.dl.apply([
          { op: 'createEntity', entity: { geometry: { type: 'circle', center: [0, 0], radius: 40 } } },
          { op: 'createEntity', entity: { geometry: { type: 'line', a: [-60, 0], b: [60, 0] } } },
          { op: 'createEntity', entity: { geometry: { type: 'text', position: [-50, 50], content: 'Ölçü İğ', height: 12 } } },
        ])
        const stats = await window.dl.nextFrame()
        const base = { ok: true, backend: r.backend, items: stats.items, drawCalls: stats.drawCalls }
        if (!${fallback}) return { ...base, png: (await window.dl.editor.viewport.exportPng()).size }
        // The environment cannot read canvases back: count the pixels of the frame that
        // exportPng renders (grid hidden) with WebGL readPixels, in the same task.
        const vp = window.dl.editor.viewport
        await vp.fit()
        const canvas = vp.canvas
        const gl = r.backend === 'webgl2' ? canvas.getContext('webgl2') : null
        let ink = null
        const toBlob = HTMLCanvasElement.prototype.toBlob
        canvas.toBlob = function (cb, ...rest) {
          if (gl) {
            const prevRead = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING)
            const prevPack = gl.getParameter(gl.PIXEL_PACK_BUFFER_BINDING)
            gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null)
            gl.bindBuffer(gl.PIXEL_PACK_BUFFER, null)
            const w = gl.drawingBufferWidth
            const h = gl.drawingBufferHeight
            const px = new Uint8Array(w * h * 4)
            gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px)
            gl.bindFramebuffer(gl.READ_FRAMEBUFFER, prevRead)
            gl.bindBuffer(gl.PIXEL_PACK_BUFFER, prevPack)
            ink = 0
            for (let i = 0; i < px.length; i += 4) {
              if (Math.abs(px[i] - px[0]) + Math.abs(px[i + 1] - px[1]) + Math.abs(px[i + 2] - px[2]) > 60) ink++
            }
          }
          return toBlob.call(this, cb, ...rest)
        }
        let pngError = null
        try {
          await vp.exportPng({ grid: false })
        } catch (e) {
          pngError = String(e && e.message || e)
        } finally {
          delete canvas.toBlob
        }
        const pngExport = pngError
          ? 'not verifiable here: ' + pngError
          : 'returned a file despite the failed control (not counted)'
        return { ...base, readback: 'webgl readPixels', ink, pngExport }
      }`),
      args: [],
    })
    const drawn = fallback ? r.ink > 200 : r.png > 0
    const good = r.ok && r.items === 3 && drawn && r.backend === backend
    const status = good ? 'passed' : backend === 'webgpu' && webgpuPolicy === 'optional' ? 'not available' : 'failed'
    if (status === 'failed') failed = true
    results.checks.push({ name: `harness ${backend}`, status, ...r })
  }
  // The React reference editor loads, its engine answers, and it renders.
  await wd('POST', `/session/${id}/url`, { url: playground })
  const p = await wd('POST', `/session/${id}/execute/async`, {
    script: script(`async () => {
      for (let i = 0; i < 300 && !window.dotloom; i++) await new Promise((r) => setTimeout(r, 100))
      if (!window.dotloom) return { ok: false, error: 'playground did not start' }
      const h = window.dotloom
      await h.engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [100, 50] } } }])
      const doc = await h.engine.documentJson()
      return { ok: true, entities: doc.entities.length, backend: h.canvas?.info?.backend ?? null }
    }`),
    args: [],
  })
  if (!p.ok || !p.backend) failed = true
  results.checks.push({ name: 'playground', status: p.ok && p.backend ? 'passed' : 'failed', ...p })
} finally {
  await wd('DELETE', `/session/${id}`).catch(() => {})
  driverProc?.kill()
}

const out = resolve(dirname(fileURLToPath(import.meta.url)), '../results')
mkdirSync(out, { recursive: true })
const majorVersion = version.split('.')[0]
writeFileSync(resolve(out, `webdriver-${browser}-${majorVersion}.json`), `${JSON.stringify(results, null, 2)}\n`)
console.log(JSON.stringify(results, null, 2))
if (process.env.GITHUB_STEP_SUMMARY) {
  const detail = (c) =>
    c.error ??
    [c.backend, c.readback && `${c.readback}: ${c.ink} px`, c.pngExport && `PNG export ${c.pngExport}`]
      .filter(Boolean)
      .join('; ')
  const rows = results.checks.map((c) => `| ${c.name} | ${c.status} | ${detail(c)} |`).join('\n')
  appendFileSync(
    process.env.GITHUB_STEP_SUMMARY,
    `### ${results.browser}${results.channel ? ` (${results.channel} major)` : ''} — ${results.platform}\n\n| check | result | detail |\n|---|---|---|\n${rows}\n`,
  )
}
process.exit(failed ? 1 : 0)
