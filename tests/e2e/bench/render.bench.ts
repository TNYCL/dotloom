import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

// DL-PERF-2 / DL-PERF-3 on the reference device. Results are written to
// bench/render-<project>.json (repository root) and summarized in docs/performance.md.

const SHAPES = Number(process.env.DOTLOOM_BENCH_SHAPES ?? 10_000)
const FRAMES = 600
const WARMUP = 60
const QUERIES = 500
const out = resolve(dirname(fileURLToPath(import.meta.url)), '../../../bench')

function pct(xs: number[], p: number): number {
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1))] ?? Number.NaN
}

const r2 = (v: number) => Math.round(v * 100) / 100

test('10k shapes at 1080p: pan/zoom frame time, hit-test and snap latency', async ({ page, browser }, info) => {
  const backend = info.project.metadata.backend as string
  await page.goto(`./bench.html?backend=${backend}`)
  const ready = await page.evaluate(() => window.bench.ready)
  if (!ready.ok) {
    // Recorded as not run (never as passed): the browser cannot provide the backend.
    mkdirSync(out, { recursive: true })
    const record = { project: info.project.name, browser: `${browser.browserType().name()} ${browser.version()}` }
    writeFileSync(
      resolve(out, `render-${info.project.name}.json`),
      `${JSON.stringify({ ...record, backend, notRun: ready.error }, null, 2)}\n`,
    )
  }
  test.skip(!ready.ok, `${backend} unavailable: ${ready.error}`)

  const load = await page.evaluate((n) => window.bench.load(n), SHAPES)
  // Presented frames and queries first: the GPU-synchronized loop below leaves
  // Firefox's event loop throttled for a while (measured: 0 dropped frames alone,
  // 30 right after; query outliers up to 1.4 s right after).
  await page.evaluate((n) => window.bench.rafFrames(n), WARMUP)
  const raf = await page.evaluate((n) => window.bench.rafFrames(n), FRAMES)
  const q = await page.evaluate((n) => window.bench.queries(n), QUERIES)
  const frames = await page.evaluate(([w, n]) => window.bench.frames(w, n), [WARMUP, FRAMES] as const)
  const env = await page.evaluate(() => ({
    userAgent: navigator.userAgent,
    devicePixelRatio,
    viewport: [innerWidth, innerHeight],
    screen: [screen.width, screen.height],
  }))

  const ms = frames.map((f) => f.ms)
  // Firefox reports WebGL fence completion late (≈40 ms after submission even with
  // 100 shapes, while presenting every 120 Hz frame): there the presented-frame
  // intervals are the frame-time measure.
  const gpuSyncReliable = browser.browserType().name() !== 'firefox'
  const phases = [
    ['pan (fitted, all shapes visible)', frames.slice(0, FRAMES / 3)],
    ['zoom in to 10×', frames.slice(FRAMES / 3, (2 * FRAMES) / 3)],
    ['zoom out to 0.25×', frames.slice((2 * FRAMES) / 3)],
  ] as const
  const summary = {
    project: info.project.name,
    browser: `${browser.browserType().name()} ${browser.version()}`,
    backend,
    adapter: ready.info,
    env,
    fixture: { shapes: SHAPES, frames: FRAMES, warmup: WARMUP, queries: QUERIES },
    loadMs: r2(load.ms),
    frame: {
      method: gpuSyncReliable ? 'input → GPU done' : 'input → GPU done (fence reported late: not a frame time)',
      p50: r2(pct(ms, 50)),
      p95: r2(pct(ms, 95)),
      max: r2(Math.max(...ms)),
      cpuP95: r2(
        pct(
          frames.map((f) => f.cpuMs),
          95,
        ),
      ),
    },
    phases: phases.map(([name, fs]) => ({
      name,
      p95: r2(
        pct(
          fs.map((f) => f.ms),
          95,
        ),
      ),
      chunksDrawn: [Math.min(...fs.map((f) => f.chunksDrawn)), Math.max(...fs.map((f) => f.chunksDrawn))],
      lineSegments: [Math.min(...fs.map((f) => f.lineSegments)), Math.max(...fs.map((f) => f.lineSegments))],
      triangles: [Math.min(...fs.map((f) => f.triangles)), Math.max(...fs.map((f) => f.triangles))],
    })),
    items: frames[0]?.items,
    slowest: frames
      .map((f, i) => ({ i, ms: r2(f.ms), zoom: r2(f.zoom), rebuilt: f.chunksRebuilt, tessellated: f.itemsTessellated }))
      .sort((a, b) => b.ms - a.ms)
      .slice(0, 5),
    raf: {
      p50: r2(pct(raf, 50)),
      p95: r2(pct(raf, 95)),
      // Intervals longer than 1.5 refresh periods (60 Hz) are dropped frames.
      dropped: raf.filter((d) => d > 25).length,
    },
    hit: { p50: r2(pct(q.hit, 50)), p95: r2(pct(q.hit, 95)), max: r2(Math.max(...q.hit)), mean: q.hitMeanMs },
    snap: { p50: r2(pct(q.snap, 50)), p95: r2(pct(q.snap, 95)), max: r2(Math.max(...q.snap)), mean: q.snapMeanMs },
  }
  mkdirSync(out, { recursive: true })
  const suffix = SHAPES === 10_000 ? '' : `-${SHAPES}`
  writeFileSync(resolve(out, `render-${info.project.name}${suffix}.json`), `${JSON.stringify(summary, null, 2)}\n`)
  console.log(JSON.stringify(summary, null, 2))

  expect(summary.raf.p95, 'p95 presented-frame interval').toBeLessThanOrEqual(16.7)
  if (gpuSyncReliable) expect(summary.frame.p95, 'p95 frame time (input → GPU done)').toBeLessThanOrEqual(16.7)
  expect(summary.hit.p95, 'p95 hit-test round trip').toBeLessThanOrEqual(8)
  expect(summary.snap.p95, 'p95 snap round trip').toBeLessThanOrEqual(8)
})
