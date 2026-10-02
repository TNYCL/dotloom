/**
 * `Viewport`: a canvas drawn by the wgpu renderer (WebAssembly, main thread).
 *
 * * Backends are tried in the given order (`['webgpu', 'webgl2']` by default);
 *   every attempt and its error is reported in `attempts` — nothing falls back
 *   silently, and there is no Canvas2D renderer.
 * * The canvas follows its container (ResizeObserver, exact device-pixel size)
 *   and the device-pixel ratio. DOM coordinates are CSS pixels everywhere.
 * * Frames are drawn on demand (scene change, camera change, overlay change).
 * * WebGPU device loss and WebGL context loss are recovered by creating a new
 *   renderer and requesting a full scene from the engine.
 */

import {
  type Camera,
  DEFAULT_CAMERA,
  fitBounds,
  isValidCamera,
  panBy,
  screenToWorld,
  type ViewSize,
  worldToScreen,
  zoomAt,
} from './camera.js'
import { type ThemeInput, themeToWire } from './color.js'
import type { DotloomEngine } from './engine.js'
import { DotloomError } from './protocol.js'
import type { Aabb, EntityId, Point, Shape, SnapKind } from './types.js'

export type Backend = 'webgpu' | 'webgl2'

export type MarkerKind = 'handle' | 'endpoint' | 'midpoint' | 'center' | 'intersection' | 'nearest' | 'grid' | 'anchor'

/** Interaction overlay (not part of the document). */
export interface OverlayState {
  markers?: { at: Point; kind: MarkerKind }[]
  marquee?: Aabb | null
  crossing?: boolean
  guides?: { a: Point; b: Point }[]
  sketch?: Shape[]
}

/** Grid settings (model units). */
export interface GridSettings {
  visible: boolean
  spacing: number
  majorEvery: number
  minPx: number
}

export const DEFAULT_GRID: GridSettings = { visible: true, spacing: 10, majorEvery: 10, minPx: 8 }

export interface RendererInfo {
  backend: Backend
  wgpuBackend: string
  adapter: string
  vendor: number
  deviceType: string
  driver: string
  format: string
  alphaMode: string
  sampleCount: number
  maxTextureDimension2D: number
  protocol: number
}

export interface FrameStats {
  items: number
  chunks: number
  chunksDrawn: number
  chunksRebuilt: number
  itemsTessellated: number
  drawCalls: number
  lineSegments: number
  triangles: number
  glyphs: number
  gpuBytes: number
  /** CPU time of the render call in milliseconds. */
  cpuMs: number
}

export interface BackendAttempt {
  backend: Backend
  ok: boolean
  error?: string
}

export interface ViewportOptions {
  /** Backends to try, in order. Default `['webgpu', 'webgl2']`. */
  backends?: Backend[]
  /** Override the renderer `.wasm` URL. */
  renderWasmUrl?: string | URL
  theme?: ThemeInput
  grid?: Partial<GridSettings>
  camera?: Camera
  /** Accessible label of the canvas. */
  label?: string
  /** 4× multisampling for smooth fill edges when supported (default true). */
  msaa?: boolean
}

/** What the editor needs from a viewport (lets tests use a fake). */
export interface ViewportLike {
  readonly size: ViewSize
  readonly camera: Camera
  setCamera(c: Camera): void
  screenToWorld(x: number, y: number): Point
  worldToScreen(p: Point): Point
  setOverlay(o: OverlayState): void
  setHover(id: EntityId | null): void
  setGrid(g: Partial<GridSettings>): void
  readonly grid: GridSettings
  fit(bounds?: Aabb | null, margin?: number): Promise<void>
  requestRender(): void
}

type RenderModule = typeof import('./wasm/render/dotloom_render_web.js')
type WebRenderer = import('./wasm/render/dotloom_render_web.js').WebRenderer

export const RENDER_PROTOCOL = 1

let modulePromise: Promise<RenderModule> | null = null

/** Load (once) the renderer WebAssembly module. */
export function loadRenderer(url?: string | URL): Promise<RenderModule> {
  modulePromise ??= (async () => {
    const m = await import('./wasm/render/dotloom_render_web.js')
    await m.default(url === undefined ? undefined : { module_or_path: url })
    if (m.renderProtocol() !== RENDER_PROTOCOL) {
      throw new DotloomError({
        code: 'protocol',
        message: `renderer protocol ${m.renderProtocol()} != ${RENDER_PROTOCOL}`,
      })
    }
    return m
  })()
  modulePromise.catch(() => {
    modulePromise = null
  })
  return modulePromise
}

function parseErr(e: unknown): { code: string; message: string } {
  if (typeof e === 'string') {
    try {
      const v = JSON.parse(e) as { code?: string; message?: string }
      if (typeof v.code === 'string') return { code: v.code, message: v.message ?? '' }
    } catch {
      // plain message
    }
    return { code: 'render', message: e }
  }
  return { code: 'render', message: e instanceof Error ? e.message : String(e) }
}

type ViewportEvents = {
  frame: FrameStats
  backend: { info: RendererInfo; attempts: BackendAttempt[] }
  lost: { backend: Backend; reason: string }
  restored: { backend: Backend; recoveries: number }
  error: { code: string; message: string }
  camera: Camera
}

const MAX_RECOVERIES = 3
const RECOVERY_WINDOW_MS = 30_000
const LOSS_RESTORE_TIMEOUT_MS = 1500
const LOSS_WATCHDOG_MS = 500

export class Viewport implements ViewportLike {
  readonly container: HTMLElement
  canvas: HTMLCanvasElement
  private renderer: WebRenderer | null = null
  private mod: RenderModule
  private readonly engine: DotloomEngine
  private readonly backends: Backend[]
  private cam: Camera
  private sz: ViewSize = { width: 1, height: 1, dpr: 1 }
  private devicePx: [number, number] = [1, 1]
  private gridSettings: GridSettings
  private themeWire: Record<string, number>
  private overlayJson = '{}'
  private hover: EntityId | null = null
  private raf: number | null = null
  private dirty = true
  private disposed = false
  private lostState: string | null = null
  private recoveries: number[] = []
  private readonly listeners = new Map<keyof ViewportEvents, Set<(v: never) => void>>()
  private readonly cleanup: (() => void)[] = []
  private readonly label: string
  private readonly msaa: boolean
  info: RendererInfo | null = null
  attempts: BackendAttempt[] = []
  lastStats: FrameStats | null = null

  private constructor(container: HTMLElement, engine: DotloomEngine, mod: RenderModule, opts: ViewportOptions) {
    this.container = container
    this.engine = engine
    this.mod = mod
    this.backends = opts.backends ?? ['webgpu', 'webgl2']
    this.cam = opts.camera && isValidCamera(opts.camera) ? opts.camera : DEFAULT_CAMERA
    this.gridSettings = { ...DEFAULT_GRID, ...opts.grid }
    this.label = opts.label ?? 'Drawing canvas'
    this.msaa = opts.msaa ?? true
    this.themeWire = this.resolveTheme(opts.theme)
    this.canvas = this.makeCanvas()
  }

  /** Create a viewport inside `container` (which must have a size). */
  static async create(container: HTMLElement, engine: DotloomEngine, opts: ViewportOptions = {}): Promise<Viewport> {
    const mod = await loadRenderer(opts.renderWasmUrl)
    const v = new Viewport(container, engine, mod, opts)
    v.measure()
    await v.start(v.backends)
    v.observe()
    return v
  }

  private resolveTheme(t?: ThemeInput): Record<string, number> {
    const preset = this.mod.themePreset(t?.preset ?? 'light')
    if (!preset) throw new DotloomError({ code: 'invalid', message: `unknown theme preset ${t?.preset}` })
    return themeToWire(JSON.parse(preset) as Record<string, number>, t?.colors)
  }

  private makeCanvas(): HTMLCanvasElement {
    const c = document.createElement('canvas')
    c.style.display = 'block'
    c.style.width = '100%'
    c.style.height = '100%'
    c.style.touchAction = 'none'
    c.style.outline = 'none'
    c.setAttribute('role', 'img')
    c.setAttribute('aria-label', this.label)
    c.dataset.dotloom = 'canvas'
    this.container.appendChild(c)
    return c
  }

  private replaceCanvas(): void {
    const old = this.canvas
    this.canvas = this.makeCanvas()
    old.remove()
    this.canvas.width = this.devicePx[0]
    this.canvas.height = this.devicePx[1]
    this.attachCanvasListeners()
  }

  on<K extends keyof ViewportEvents>(name: K, cb: (v: ViewportEvents[K]) => void): () => void {
    let set = this.listeners.get(name)
    if (!set) {
      set = new Set()
      this.listeners.set(name, set)
    }
    set.add(cb as (v: never) => void)
    return () => set?.delete(cb as (v: never) => void)
  }

  private emit<K extends keyof ViewportEvents>(name: K, v: ViewportEvents[K]): void {
    for (const cb of [...(this.listeners.get(name) ?? [])]) (cb as (x: ViewportEvents[K]) => void)(v)
  }

  private measure(): void {
    const r = this.container.getBoundingClientRect()
    const dpr = globalThis.devicePixelRatio || 1
    this.setSize(r.width, r.height, Math.round(r.width * dpr), Math.round(r.height * dpr))
  }

  private setSize(cssW: number, cssH: number, devW: number, devH: number): void {
    const w = Math.max(1, cssW)
    const h = Math.max(1, cssH)
    const dw = Math.max(1, Math.min(16384, devW))
    const dh = Math.max(1, Math.min(16384, devH))
    // The ratio actually used by the canvas, not window.devicePixelRatio (zoom,
    // fractional scaling and rounding all end up in this one number).
    this.sz = { width: w, height: h, dpr: dw / w }
    this.devicePx = [dw, dh]
    this.canvas.width = dw
    this.canvas.height = dh
    try {
      this.renderer?.resize(dw, dh)
    } catch (e) {
      this.emit('error', parseErr(e))
    }
    this.requestRender()
  }

  private observe(): void {
    if (typeof ResizeObserver === 'function') {
      const ro = new ResizeObserver((entries) => {
        const e = entries[0]
        if (!e) return
        const cssW = e.contentRect.width
        const cssH = e.contentRect.height
        const dev = e.devicePixelContentBoxSize?.[0]
        const dpr = globalThis.devicePixelRatio || 1
        const est = [Math.round(cssW * dpr), Math.round(cssH * dpr)] as const
        // The exact device-pixel box is preferred, but some environments (e.g.
        // emulated device scale factors) report CSS pixels there: only trust it
        // when it agrees with devicePixelRatio to within a pixel.
        const exact =
          dev && Math.abs(dev.inlineSize - est[0]) <= 1 && Math.abs(dev.blockSize - est[1]) <= 1 ? dev : null
        this.setSize(cssW, cssH, exact ? exact.inlineSize : est[0], exact ? exact.blockSize : est[1])
      })
      try {
        ro.observe(this.container, { box: 'device-pixel-content-box' })
      } catch {
        ro.observe(this.container)
      }
      this.cleanup.push(() => ro.disconnect())
    }
    // DPR changes (moving between monitors, browser zoom) without a size change.
    const watchDpr = (): void => {
      if (typeof matchMedia !== 'function') return
      const mq = matchMedia(`(resolution: ${globalThis.devicePixelRatio || 1}dppx)`)
      const onChange = (): void => {
        this.measure()
        watchDpr()
      }
      mq.addEventListener('change', onChange, { once: true })
      this.cleanup.push(() => mq.removeEventListener('change', onChange))
    }
    watchDpr()
    // Device loss is reported asynchronously; notice it even when no frame is drawn.
    const watchdog = setInterval(() => {
      const r = this.renderer
      if (!r || this.lostState || this.disposed) return
      const lost = this.lostReason(r)
      if (lost) {
        this.markLost(this.info?.backend ?? 'webgpu', lost)
        void this.recover()
      }
    }, LOSS_WATCHDOG_MS)
    this.cleanup.push(() => clearInterval(watchdog))
    this.cleanup.push(
      this.engine.on('scene', (s) => {
        if (!this.renderer || this.lostState) return
        try {
          this.renderer.applyDelta(new Uint8Array(s.delta))
          this.requestRender()
        } catch (e) {
          this.emit('error', parseErr(e))
        }
      }),
    )
  }

  private canvasListeners: (() => void)[] = []

  private attachCanvasListeners(): void {
    for (const f of this.canvasListeners.splice(0)) f()
    const lost = (ev: Event): void => {
      ev.preventDefault()
      this.markLost('webgl2', 'WebGL context lost')
    }
    const restored = (): void => {
      void this.recover()
    }
    this.canvas.addEventListener('webglcontextlost', lost)
    this.canvas.addEventListener('webglcontextrestored', restored)
    this.canvasListeners.push(
      () => this.canvas.removeEventListener('webglcontextlost', lost),
      () => this.canvas.removeEventListener('webglcontextrestored', restored),
    )
  }

  private async start(order: Backend[]): Promise<void> {
    const attempts: BackendAttempt[] = []
    for (const backend of order) {
      if (attempts.length > 0) this.replaceCanvas()
      else this.attachCanvasListeners()
      try {
        const r = await this.mod.WebRenderer.create(
          this.canvas,
          backend,
          this.devicePx[0],
          this.devicePx[1],
          JSON.stringify({ msaa: this.msaa }),
        )
        attempts.push({ backend, ok: true })
        this.renderer = r
        this.info = JSON.parse(r.info()) as RendererInfo
        this.attempts = attempts
        this.applySettings()
        this.lostState = null
        this.emit('backend', { info: this.info, attempts })
        await this.engine.requestFullScene()
        this.requestRender()
        return
      } catch (e) {
        attempts.push({ backend, ok: false, error: parseErr(e).message })
      }
    }
    this.attempts = attempts
    throw new DotloomError({
      code: 'init',
      message: `no renderer backend available (${attempts.map((a) => `${a.backend}: ${a.error}`).join('; ')})`,
      details: { attempts },
    })
  }

  private applySettings(): void {
    const r = this.renderer
    if (!r) return
    r.setTheme(JSON.stringify(this.themeWire))
    r.setGrid(JSON.stringify(this.gridSettings))
    r.setOverlay(this.overlayJson)
    r.setHover(this.hover ?? -1)
  }

  private markLost(backend: Backend, reason: string): void {
    if (this.lostState) return
    this.lostState = reason
    this.emit('lost', { backend, reason })
    // If the browser never restores the context, recover on a fresh canvas.
    setTimeout(() => {
      if (this.lostState === reason) void this.recover()
    }, LOSS_RESTORE_TIMEOUT_MS)
  }

  private recovering = false

  private async recover(): Promise<void> {
    if (this.disposed || this.recovering || !this.lostState) return
    this.recovering = true
    try {
      await this.recoverNow()
    } finally {
      this.recovering = false
    }
  }

  private async recoverNow(): Promise<void> {
    const now = Date.now()
    this.recoveries = this.recoveries.filter((t) => now - t < RECOVERY_WINDOW_MS)
    if (this.recoveries.length >= MAX_RECOVERIES) {
      this.emit('error', { code: 'lost', message: 'the GPU device was lost repeatedly; rendering stopped' })
      return
    }
    this.recoveries.push(now)
    const backend = this.info?.backend ?? this.backends[0] ?? 'webgpu'
    try {
      this.renderer?.dispose()
    } catch {
      // already unusable
    }
    this.renderer = null
    this.replaceCanvas()
    try {
      const rest = this.backends.filter((b) => b !== backend)
      await this.start([backend, ...rest])
      this.emit('restored', { backend: this.info?.backend ?? backend, recoveries: this.recoveries.length })
    } catch (e) {
      this.emit('error', parseErr(e))
    }
  }

  private lostReason(r: WebRenderer): string | null {
    try {
      return r.lost() ?? null
    } catch {
      return 'renderer unavailable'
    }
  }

  /** Simulate GPU device/context loss (tests). */
  simulateContextLoss(): void {
    if (!this.renderer) return
    if (this.info?.backend === 'webgl2') {
      // Get the extension before losing the context: a lost context returns null.
      const ext = this.canvas.getContext('webgl2')?.getExtension('WEBGL_lose_context')
      ext?.loseContext()
      // Restore shortly after, as a real driver reset would.
      setTimeout(() => ext?.restoreContext(), 50)
    } else {
      this.renderer.loseDeviceForTesting()
      this.requestRender()
    }
  }

  // --- camera ------------------------------------------------------------

  get size(): ViewSize {
    return this.sz
  }

  get camera(): Camera {
    return this.cam
  }

  get grid(): GridSettings {
    return this.gridSettings
  }

  get backend(): Backend | null {
    return this.info?.backend ?? null
  }

  setCamera(c: Camera): void {
    if (!isValidCamera(c)) return
    this.cam = c
    this.emit('camera', c)
    this.requestRender()
  }

  panBy(dx: number, dy: number): void {
    this.setCamera(panBy(this.cam, dx, dy))
  }

  zoomAt(x: number, y: number, factor: number): void {
    this.setCamera(zoomAt(this.cam, this.sz, x, y, factor))
  }

  screenToWorld(x: number, y: number): Point {
    return screenToWorld(this.cam, this.sz, x, y)
  }

  worldToScreen(p: Point): Point {
    return worldToScreen(this.cam, this.sz, p)
  }

  /** Scene bounds known to the renderer, or `null` when empty. */
  sceneBounds(): Aabb | null {
    if (!this.renderer) return null
    return JSON.parse(this.renderer.sceneBounds()) as Aabb | null
  }

  /** Fit `bounds` (default: the whole scene). */
  async fit(bounds?: Aabb | null, margin = 32): Promise<void> {
    const b = bounds ?? this.sceneBounds()
    if (b) this.setCamera(fitBounds(this.sz, b, margin))
  }

  // --- appearance --------------------------------------------------------

  setTheme(t: ThemeInput): void {
    this.themeWire = this.resolveTheme(t)
    this.renderer?.setTheme(JSON.stringify(this.themeWire))
    this.requestRender()
  }

  setGrid(g: Partial<GridSettings>): void {
    this.gridSettings = { ...this.gridSettings, ...g }
    this.renderer?.setGrid(JSON.stringify(this.gridSettings))
    this.requestRender()
  }

  setOverlay(o: OverlayState): void {
    const json = JSON.stringify({
      markers: o.markers ?? [],
      marquee: o.marquee ?? null,
      crossing: o.crossing ?? false,
      guides: o.guides ?? [],
      sketch: o.sketch ?? [],
    })
    if (json === this.overlayJson) return
    this.overlayJson = json
    try {
      this.renderer?.setOverlay(json)
    } catch (e) {
      this.emit('error', parseErr(e))
    }
    this.requestRender()
  }

  setHover(id: EntityId | null): void {
    if (id === this.hover) return
    this.hover = id
    this.renderer?.setHover(id ?? -1)
    this.requestRender()
  }

  // --- frames ------------------------------------------------------------

  requestRender(): void {
    this.dirty = true
    if (this.raf !== null || this.disposed) return
    if (typeof requestAnimationFrame !== 'function') return
    this.raf = requestAnimationFrame(() => {
      this.raf = null
      this.frame()
    })
  }

  /** Draw now (normally driven by `requestRender`). */
  frame(): FrameStats | null {
    if (!this.dirty || !this.renderer || this.lostState || this.disposed) return null
    this.dirty = false
    const r = this.renderer
    try {
      r.setView(this.cam.center[0], this.cam.center[1], this.cam.scale, this.sz.width, this.sz.height, this.sz.dpr)
      const t0 = performance.now()
      const json = r.render()
      const cpuMs = performance.now() - t0
      if (json === 'null') {
        // Surface reconfigured or occluded; try again next frame.
        this.requestRender()
        return null
      }
      const stats = { ...(JSON.parse(json) as Omit<FrameStats, 'cpuMs'>), cpuMs }
      this.lastStats = stats
      this.emit('frame', stats)
      return stats
    } catch (e) {
      const err = parseErr(e)
      const lost = err.code === 'lost' ? err.message : this.lostReason(r)
      if (lost) {
        this.markLost(this.info?.backend ?? 'webgpu', lost)
        void this.recover()
      } else {
        this.emit('error', err)
        // The device-lost notification can arrive after the frame that failed.
        setTimeout(() => {
          const late = r === this.renderer ? this.lostReason(r) : null
          if (late) {
            this.markLost(this.info?.backend ?? 'webgpu', late)
            void this.recover()
          }
        }, 100)
      }
      return null
    }
  }

  /** Remove the canvas and release GPU resources and listeners. */
  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    if (this.raf !== null && typeof cancelAnimationFrame === 'function') cancelAnimationFrame(this.raf)
    for (const f of this.cleanup.splice(0)) f()
    for (const f of this.canvasListeners.splice(0)) f()
    try {
      this.renderer?.dispose()
      this.renderer?.free()
    } catch {
      // already released
    }
    this.renderer = null
    this.canvas.remove()
    this.listeners.clear()
  }

  get isDisposed(): boolean {
    return this.disposed
  }
}

export type { SnapKind }
