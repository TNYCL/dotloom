/**
 * Reference-device benchmarks (DL-PERF-2…8), built from the packaged @dotloomjs/sdk
 * like an application.
 *
 * Fixture: `count` simple shapes (40 % lines, 20 % rectangles, 20 % circles, 10 %
 * arcs, 10 % five-point polylines; every fifth shape filled) on a square grid of
 * 100 mm cells, so every shape is visible when the view fits the scene.
 *
 * URL parameters: `backend=webgpu|webgl2` (exactly this backend), `grid=0|1`.
 */

import {
  type Backend,
  type Camera,
  type Command,
  createEditor,
  type DotloomEditor,
  DotloomError,
  type Point,
} from '@dotloomjs/sdk'

interface FrameSample {
  /** Input (camera change) → frame encoded, submitted and finished on the GPU. */
  ms: number
  /** CPU time of the render call. */
  cpuMs: number
  /** Camera scale relative to the fitted view. */
  zoom: number
  items: number
  chunksRebuilt: number
  itemsTessellated: number
  chunksDrawn: number
  drawCalls: number
  lineSegments: number
  triangles: number
}

interface OpenResult {
  /** `.dotl` bytes → fitted first frame finished on the GPU. */
  ms: number
  loadMs: number
  firstFrameMs: number
  engineWasmBytes: number
  rendererWasmBytes: number
  gpuBytes: number
}

interface Bench {
  ready: Promise<{ ok: boolean; backend?: string; info?: unknown; error?: string; coldStartMs?: number }>
  load(count: number): Promise<{ ms: number; batches: number }>
  /** Camera path driven frame by frame, each frame awaited until the GPU is done. */
  frames(warmup: number, count: number): Promise<FrameSample[]>
  /** The same path driven by requestAnimationFrame: presented-frame intervals. */
  rafFrames(count: number): Promise<number[]>
  /** Worker round trips of hit-test and snap queries at seeded points. */
  queries(count: number): Promise<{ hit: number[]; snap: number[]; hitMeanMs: number; snapMeanMs: number }>
  /** Save the loaded document as `.dotl` (kept for `open`/`cycles`). */
  save(): Promise<{ bytes: number; ms: number }>
  /** Second editor on the page (modules cached): engine worker + renderer. */
  warmStart(): Promise<number>
  /** Open the saved `.dotl` in a fresh editor (WASM already initialized). */
  open(): Promise<OpenResult>
  /** Create, open, draw and dispose an editor `n` times; memory after each cycle. */
  cycles(n: number): Promise<{ heap: number[]; rendererWasm: number[]; ms: number[] }>
  /** 100k-shape stress: build, open, navigate, then cancel a long solve. */
  stress(count: number): Promise<StressResult>
  /** DL-DOC-9: single geometry edits in the loaded document. */
  editOne(count: number): Promise<EditResult>
  memory(): { usedJSHeapSize: number } | null
}

interface EditResult {
  /** Commit round trip of one exact edit. */
  ms: number[]
  /** Encoded scene delta per edit, and the full scene for comparison. */
  deltaBytes: number[]
  fullSceneBytes: number
  /** Renderer work for the frame after each edit. */
  itemsTessellated: number[]
  chunksRebuilt: number[]
}

interface StressResult {
  buildMs: number
  open: OpenResult
  frames: number[]
  errors: string[]
  cancel: { started: boolean; cancelled: boolean; code: string; latencyMs: number; revisionKept: boolean }
  queryAfterCancelMs: number
}

declare global {
  interface Window {
    bench: Bench
    gc?: () => void
  }
}

const params = new URLSearchParams(location.search)
const backend = (params.get('backend') ?? 'webgpu') as Backend
const t0Page = performance.now()
let editor: DotloomEditor | null = null
let fitted: Camera | null = null
let dotl: Uint8Array | null = null
/** IDs created by `load`, in fixture order (index % 10 < 4 are lines). */
let created: number[] = []

function fixture(count: number, first: number, batch = 1000): Command[] {
  const cols = Math.ceil(Math.sqrt(count))
  const out: Command[] = []
  for (let i = first; i < Math.min(count, first + batch); i++) {
    const x = (i % cols) * 100
    const y = Math.floor(i / cols) * 100
    const kind = i % 10
    const p = (dx: number, dy: number): Point => [x + dx, y + dy]
    const geometry =
      kind < 4
        ? { type: 'line' as const, a: p(10, 10), b: p(90, 70 + (i % 7) * 3) }
        : kind < 6
          ? { type: 'rect' as const, origin: p(15, 15), width: 60, height: 45 }
          : kind < 8
            ? { type: 'circle' as const, center: p(50, 50), radius: 30 + (i % 5) * 3 }
            : kind === 8
              ? { type: 'arc' as const, center: p(50, 50), radius: 35, start: 0.3, sweep: 4.2 }
              : { type: 'polyline' as const, points: [p(10, 20), p(30, 80), p(50, 20), p(70, 80), p(90, 20)] }
    const style = i % 5 === 4 ? { fill: '#4f8ad433' } : undefined
    out.push({ op: 'createEntity', entity: { geometry, ...(style ? { style } : {}) } })
  }
  return out
}

/** Deterministic camera path: pan at the fitted scale, zoom in to 10×, out to 0.25×. */
function cameraAt(base: Camera, k: number, count: number): { cam: Camera; zoom: number } {
  const [cx, cy] = base.center
  const extent = 1920 / base.scale
  const third = count / 3
  const t = (k % third) / third
  if (k < third) {
    const a = t * 2 * Math.PI
    const r = 0.15 * extent
    return { cam: { center: [cx + r * Math.cos(a), cy + r * Math.sin(a)], scale: base.scale }, zoom: 1 }
  }
  const zoom = k < 2 * third ? 10 ** Math.sin(Math.PI * t) : 10 ** (-0.6 * Math.sin(Math.PI * t))
  const drift = 0.1 * extent * t
  return { cam: { center: [cx + drift, cy - drift], scale: base.scale * zoom }, zoom }
}

function seeded(seed: number): () => number {
  let s = seed >>> 0
  return () => {
    s ^= s << 13
    s ^= s >>> 17
    s ^= s << 5
    return (s >>> 0) / 4294967296
  }
}

/** A full-size stage for an additional editor. */
function stage(): HTMLDivElement {
  const div = document.createElement('div')
  div.style.cssText = 'position:absolute;inset:0'
  document.body.append(div)
  return div
}

function makeEditor(el: HTMLElement): Promise<DotloomEditor> {
  return createEditor(el, { viewport: { backends: [backend], grid: { visible: params.get('grid') !== '0' } } })
}

async function openIn(ed: DotloomEditor, bytes: Uint8Array): Promise<OpenResult> {
  const t0 = performance.now()
  await ed.engine.load(bytes)
  const t1 = performance.now()
  await ed.viewport.fit(null, 16)
  ed.viewport.frame()
  await ed.viewport.gpuIdle()
  const t2 = performance.now()
  const mem = ed.viewport.memoryStats()
  return {
    ms: t2 - t0,
    loadMs: t1 - t0,
    firstFrameMs: t2 - t1,
    engineWasmBytes: (await ed.engine.memory()).wasmBytes,
    rendererWasmBytes: mem.wasmBytes,
    gpuBytes: mem.gpuBytes,
  }
}

async function build(ed: DotloomEditor, count: number, batch: number): Promise<number> {
  const t0 = performance.now()
  for (let first = 0; first < count; first += batch) await ed.engine.apply(fixture(count, first, batch))
  return performance.now() - t0
}

const bench: Bench = {
  ready: (async () => {
    try {
      editor = await makeEditor(document.getElementById('stage') as HTMLDivElement)
      return {
        ok: true,
        backend: editor.viewport.backend ?? '',
        info: editor.viewport.info,
        // Page script start → editor ready: worker spawn, engine and renderer WASM
        // fetch + compile + instantiate, GPU adapter and device.
        coldStartMs: performance.now() - t0Page,
      }
    } catch (e) {
      return { ok: false, error: String((e as Error).message) }
    }
  })(),

  async load(count) {
    if (!editor) throw new Error('not ready')
    const t0 = performance.now()
    let batches = 0
    created = []
    for (let first = 0; first < count; first += 1000) {
      const r = await editor.engine.apply(fixture(count, first))
      created.push(...r.created)
      batches += 1
    }
    await editor.viewport.fit(null, 16)
    fitted = editor.viewport.camera
    // Let the scene deltas reach the renderer before measuring.
    editor.viewport.frame()
    await editor.viewport.gpuIdle()
    return { ms: performance.now() - t0, batches }
  },

  async frames(warmup, count) {
    if (!editor || !fitted) throw new Error('not loaded')
    const vp = editor.viewport
    const base = fitted
    const out: FrameSample[] = []
    for (let k = -warmup; k < count; k++) {
      const { cam, zoom } = cameraAt(base, ((k % count) + count) % count, count)
      const t0 = performance.now()
      vp.setCamera(cam)
      const stats = vp.frame()
      await vp.gpuIdle()
      const ms = performance.now() - t0
      if (k >= 0 && stats) {
        out.push({
          ms,
          cpuMs: stats.cpuMs,
          zoom,
          items: stats.items,
          chunksRebuilt: stats.chunksRebuilt,
          itemsTessellated: stats.itemsTessellated,
          chunksDrawn: stats.chunksDrawn,
          drawCalls: stats.drawCalls,
          lineSegments: stats.lineSegments,
          triangles: stats.triangles,
        })
      }
    }
    return out
  },

  rafFrames(count) {
    const vp = editor?.viewport
    const base = fitted
    if (!vp || !base) throw new Error('not loaded')
    return new Promise((resolve) => {
      const stamps: number[] = []
      let k = 0
      const step = (now: number) => {
        stamps.push(now)
        if (k >= count) {
          resolve(stamps.slice(1).map((s, i) => s - (stamps[i] ?? s)))
          return
        }
        vp.setCamera(cameraAt(base, k, count).cam)
        vp.frame()
        k += 1
        requestAnimationFrame(step)
      }
      requestAnimationFrame(step)
    })
  },

  async queries(count) {
    if (!editor || !fitted) throw new Error('not loaded')
    const rnd = seeded(0x5eed)
    const hit: number[] = []
    const snap: number[] = []
    const extent = 1920 / fitted.scale
    let hitTotal = 0
    let snapTotal = 0
    for (let i = 0; i < count; i++) {
      // Half of the queries at the fitted view, half zoomed in 5×.
      const zoom = i % 2 === 0 ? 1 : 5
      const radius = 6 / (fitted.scale * zoom)
      const point: Point = [
        fitted.center[0] + (rnd() - 0.5) * extent * 0.9,
        fitted.center[1] + (rnd() - 0.5) * extent * 0.5,
      ]
      let t0 = performance.now()
      await editor.engine.hitTest(point, radius)
      let dt = performance.now() - t0
      hit.push(dt)
      hitTotal += dt
      t0 = performance.now()
      await editor.engine.snap({ point, radius })
      dt = performance.now() - t0
      snap.push(dt)
      snapTotal += dt
    }
    // performance.now() is coarsened (0.1 ms in Chromium, 1 ms in Firefox without
    // cross-origin isolation); the means over all queries are more precise.
    return { hit, snap, hitMeanMs: hitTotal / count, snapMeanMs: snapTotal / count }
  },

  async save() {
    if (!editor) throw new Error('not ready')
    const t0 = performance.now()
    dotl = await editor.engine.save()
    return { bytes: dotl.byteLength, ms: performance.now() - t0 }
  },

  async warmStart() {
    const el = stage()
    const t0 = performance.now()
    const ed = await makeEditor(el)
    const ms = performance.now() - t0
    ed.dispose()
    el.remove()
    return ms
  },

  async open() {
    if (!dotl) throw new Error('save first')
    const el = stage()
    const ed = await makeEditor(el)
    try {
      return await openIn(ed, dotl)
    } finally {
      ed.dispose()
      el.remove()
    }
  },

  async cycles(n) {
    if (!dotl) throw new Error('save first')
    const heap: number[] = []
    const rendererWasm: number[] = []
    const ms: number[] = []
    for (let i = 0; i < n; i++) {
      const t0 = performance.now()
      const el = stage()
      const ed = await makeEditor(el)
      await openIn(ed, dotl)
      ed.dispose()
      el.remove()
      ms.push(performance.now() - t0)
      // Let disposal callbacks run, then collect garbage when exposed.
      await new Promise((r) => setTimeout(r, 50))
      window.gc?.()
      heap.push(bench.memory()?.usedJSHeapSize ?? Number.NaN)
      rendererWasm.push(editor?.viewport.memoryStats().wasmBytes ?? Number.NaN)
    }
    return { heap, rendererWasm, ms }
  },

  async stress(count) {
    const errors: string[] = []
    // Build in a dedicated editor, keep only the file.
    let el = stage()
    let ed = await makeEditor(el)
    const buildMs = await build(ed, count, 5000)
    const bytes = await ed.engine.save()
    ed.dispose()
    el.remove()

    el = stage()
    ed = await makeEditor(el)
    for (const ev of ['lost', 'error'] as const) ed.viewport.on(ev, (v) => errors.push(`${ev}: ${JSON.stringify(v)}`))
    ed.engine.on('crash', (v) => errors.push(`crash: ${JSON.stringify(v)}`))
    const open = await openIn(ed, bytes)
    const base = ed.viewport.camera
    const frames: number[] = []
    for (let k = 0; k < 120; k++) {
      const t0 = performance.now()
      ed.viewport.setCamera(cameraAt(base, k * 5, 600).cam)
      ed.viewport.frame()
      await ed.viewport.gpuIdle()
      frames.push(performance.now() - t0)
    }

    // A long solve: a 4000-link chain whose end is typed far away, in one
    // transaction. Cancel it after 100 ms.
    const ids = await ed.engine.reserveIds(4001)
    const cmds: Command[] = []
    for (let i = 0; i < 4000; i++) {
      const a: Point = [i * 100, -2000 + 3 * Math.sin(i)]
      const b: Point = [(i + 1) * 100, -2000 + 3 * Math.sin(i + 1)]
      cmds.push({ op: 'createEntity', id: ids[i] as number, entity: { geometry: { type: 'line', a, b } } })
    }
    for (let i = 0; i < 4000; i++) {
      const line = {
        from: { entity: ids[i] as number, anchor: 'start' },
        to: { entity: ids[i] as number, anchor: 'end' },
      }
      cmds.push({ op: 'addConstraint', constraint: { rule: { kind: 'length', line, value: 100 } } })
      if (i > 0) {
        const a = { entity: ids[i - 1] as number, anchor: 'end' }
        const b = { entity: ids[i] as number, anchor: 'start' }
        cmds.push({ op: 'addConstraint', constraint: { rule: { kind: 'coincident', a, b } } })
      }
    }
    cmds.push({
      op: 'addConstraint',
      constraint: { rule: { kind: 'fixPoint', a: { entity: ids[0] as number, anchor: 'start' }, at: [0, -2000] } },
    })
    cmds.push({
      op: 'addConstraint',
      constraint: {
        rule: { kind: 'fixPoint', a: { entity: ids[3999] as number, anchor: 'end' }, at: [150_000, 60_000] },
      },
    })
    const revision = ed.engine.revision
    const ctl = new AbortController()
    let code = 'completed'
    let abortedAt = 0
    const started = performance.now()
    const run = ed.engine.apply(cmds, { signal: ctl.signal }).then(
      () => 'completed',
      (e: unknown) => (e instanceof DotloomError ? e.code : String(e)),
    )
    const timer = setTimeout(() => {
      abortedAt = performance.now()
      ctl.abort()
    }, 100)
    code = await run
    const settled = performance.now()
    clearTimeout(timer)
    const t0 = performance.now()
    await ed.engine.hitTest(base.center, 10)
    const queryAfterCancelMs = performance.now() - t0
    const result: StressResult = {
      buildMs,
      open,
      frames,
      errors,
      cancel: {
        started: abortedAt > 0 && abortedAt - started >= 99,
        cancelled: code === 'cancelled',
        code,
        latencyMs: abortedAt > 0 ? settled - abortedAt : Number.NaN,
        revisionKept: ed.engine.revision === revision,
      },
      queryAfterCancelMs,
    }
    ed.dispose()
    el.remove()
    return result
  },

  async editOne(count) {
    if (!editor) throw new Error('not ready')
    const { engine, viewport } = editor
    let last = 0
    const off = engine.on('scene', (m) => {
      last = m.delta.byteLength
    })
    try {
      await engine.requestFullScene()
      const fullSceneBytes = last
      viewport.frame()
      const out: EditResult = { ms: [], deltaBytes: [], fullSceneBytes, itemsTessellated: [], chunksRebuilt: [] }
      for (let k = 0; k < count; k++) {
        const id = created[(k * 37 * 10) % created.length] ?? created[0]
        if (id === undefined) break
        const t0 = performance.now()
        await engine.apply([
          { op: 'setParams', values: [{ entity: id, param: 'b.x', value: 5000 + k }], mode: 'exact' },
        ])
        out.ms.push(performance.now() - t0)
        out.deltaBytes.push(last)
        viewport.requestRender()
        const stats = viewport.frame()
        out.itemsTessellated.push(stats?.itemsTessellated ?? -1)
        out.chunksRebuilt.push(stats?.chunksRebuilt ?? -1)
      }
      return out
    } finally {
      off()
    }
  },

  memory() {
    const m = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory
    return m ? { usedJSHeapSize: m.usedJSHeapSize } : null
  },
}

window.bench = bench
