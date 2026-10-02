import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

// DL-PERF-5…8 on the reference device: cold/warm start, opening a 10k-object .dotl,
// repeated open/close (leak check), a 100k-object stress run with a cancelled solve,
// and WebAssembly memory. Results: bench/files-<project>.json.

const out = resolve(dirname(fileURLToPath(import.meta.url)), '../../../bench')
const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)] ?? Number.NaN
const pct = (xs: number[], p: number) =>
  [...xs].sort((a, b) => a - b)[Math.min(xs.length - 1, Math.max(0, Math.ceil((p / 100) * xs.length) - 1))] ??
  Number.NaN
const r1 = (v: number) => Math.round(v * 10) / 10
const mib = (b: number) => Math.round((b / 1048576) * 10) / 10
const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / Math.max(1, xs.length)

test('open 10k .dotl, open/close leak check, 100k stress with cancel', async ({ page, browser }, info) => {
  test.setTimeout(900_000)
  const backend = info.project.metadata.backend as string
  await page.goto(`./bench.html?backend=${backend}`)
  const ready = await page.evaluate(() => window.bench.ready)
  test.skip(!ready.ok, `${backend} unavailable: ${ready.error}`)

  const warmStartMs = await page.evaluate(() => window.bench.warmStart())
  await page.evaluate(() => window.bench.load(10_000))
  const saved = await page.evaluate(() => window.bench.save())
  const opens = []
  for (let i = 0; i < 5; i++) opens.push(await page.evaluate(() => window.bench.open()))
  const cycles = await page.evaluate((n) => window.bench.cycles(n), 20)
  const stress = await page.evaluate((n) => window.bench.stress(n), 100_000)

  // Leak check: after a 5-cycle warm-up, compare cycles 6–10 with 16–20.
  const early = cycles.heap.slice(5, 10)
  const late = cycles.heap.slice(15, 20)
  const heapGrowth = mean(late) / mean(early) - 1
  const wasmGrowth = (cycles.rendererWasm[19] ?? 0) - (cycles.rendererWasm[5] ?? 0)
  const summary = {
    project: info.project.name,
    browser: `${browser.browserType().name()} ${browser.version()}`,
    backend,
    coldStartMs: r1(ready.coldStartMs ?? Number.NaN),
    warmStartMs: r1(warmStartMs),
    dotl10k: {
      bytes: saved.bytes,
      saveMs: r1(saved.ms),
      openMs: opens.map((o) => r1(o.ms)),
      openMedianMs: r1(median(opens.map((o) => o.ms))),
      openMaxMs: r1(Math.max(...opens.map((o) => o.ms))),
      loadMedianMs: r1(median(opens.map((o) => o.loadMs))),
      firstFrameMedianMs: r1(median(opens.map((o) => o.firstFrameMs))),
      engineWasmMiB: mib(opens[0]?.engineWasmBytes ?? 0),
      rendererWasmMiB: mib(opens[0]?.rendererWasmBytes ?? 0),
      gpuMiB: mib(opens[0]?.gpuBytes ?? 0),
    },
    leak: {
      cycles: cycles.heap.length,
      heapMiB: cycles.heap.map(mib),
      rendererWasmMiB: cycles.rendererWasm.map(mib),
      cycleMs: cycles.ms.map(r1),
      heapGrowthAfterWarmup: Math.round(heapGrowth * 1000) / 1000,
      rendererWasmGrowthBytes: wasmGrowth,
      heapMeasured: cycles.heap.every((h) => Number.isFinite(h)),
    },
    stress100k: {
      buildMs: r1(stress.buildMs),
      openMs: r1(stress.open.ms),
      loadMs: r1(stress.open.loadMs),
      firstFrameMs: r1(stress.open.firstFrameMs),
      engineWasmMiB: mib(stress.open.engineWasmBytes),
      rendererWasmMiB: mib(stress.open.rendererWasmBytes),
      gpuMiB: mib(stress.open.gpuBytes),
      frameP50: r1(median(stress.frames)),
      frameP95: r1(pct(stress.frames, 95)),
      errors: stress.errors,
      cancel: { ...stress.cancel, latencyMs: r1(stress.cancel.latencyMs) },
      queryAfterCancelMs: r1(stress.queryAfterCancelMs),
    },
  }
  mkdirSync(out, { recursive: true })
  writeFileSync(resolve(out, `files-${info.project.name}.json`), `${JSON.stringify(summary, null, 2)}\n`)
  console.log(JSON.stringify(summary, null, 2))

  expect(summary.dotl10k.openMaxMs, 'open 10k-object .dotl (excl. WASM init)').toBeLessThanOrEqual(2000)
  expect(summary.stress100k.errors, '100k stress: no crash, loss or renderer error').toEqual([])
  expect(stress.cancel.started, 'the long solve was still running when cancelled').toBe(true)
  expect(stress.cancel.cancelled, `cancel result: ${stress.cancel.code}`).toBe(true)
  expect(stress.cancel.revisionKept, 'cancelled transaction left the document unchanged').toBe(true)
  if (summary.leak.heapMeasured) {
    expect(summary.leak.heapGrowthAfterWarmup, 'JS heap growth after warm-up').toBeLessThan(0.05)
  }
  expect(summary.leak.rendererWasmGrowthBytes, 'renderer WASM memory growth after warm-up').toBe(0)
})
