/**
 * Renderer in real browsers, per backend (DL-RENDER-1/3/4/7/8/9).
 */

import { expect, type Page, test } from '@playwright/test'
import { decodePng, type Rgba } from './png.js'

type Ready = { ok: boolean; backend?: string; error?: string; info?: Record<string, unknown>; attempts?: unknown }

const BACKENDS = ['webgpu', 'webgl2'] as const

async function open(page: Page, backend: string, browserName: string): Promise<Ready> {
  await page.goto(`./?backend=${backend}`)
  const r = (await page.evaluate(() => window.dl.ready)) as Ready
  if (r.ok && backend === 'webgpu') {
    // Environment check: does plain WebGPU keep a device and show a frame here?
    const probe = await rawWebgpu(page)
    test.info().annotations.push({ type: 'webgpu-probe', description: probe.ok ? 'ok' : probe.reason })
    test.skip(!probe.ok, `${browserName}: ${probe.reason} (environment, reproduced without Dotloom)`)
  }
  if (!r.ok) {
    // WebGPU is optional outside Chromium: report as skipped with the reason.
    test.skip(backend === 'webgpu' && browserName !== 'chromium', `WebGPU unavailable in ${browserName}: ${r.error}`)
    throw new Error(`${backend} renderer failed in ${browserName}: ${r.error}`)
  }
  test.info().annotations.push({ type: 'adapter', description: JSON.stringify(r.info) })
  return r
}

async function drawSample(page: Page): Promise<void> {
  await page.evaluate(async () => {
    await window.dl.apply([
      {
        op: 'createEntity',
        entity: { geometry: { type: 'line', a: [-200, 0], b: [200, 0] }, style: { stroke: '#000000', strokeWidth: 3 } },
      },
      {
        op: 'createEntity',
        entity: { geometry: { type: 'rect', origin: [50, 50], width: 100, height: 60 }, style: { fill: '#ff0000' } },
      },
      {
        op: 'createEntity',
        entity: { geometry: { type: 'text', position: [-250, -100], content: 'Ölçü ğüşıİç', height: 24 } },
      },
    ])
    await window.dl.nextFrame()
  })
}

/**
 * Whether the browser composites a *resized* WebGL2 canvas at all (plain WebGL,
 * no Dotloom code). Some WebKit builds do not; tests depending on it are skipped
 * with this reason instead of being reported as passing.
 */
async function rawWebglResizeComposites(page: Page): Promise<boolean> {
  await page.evaluate(async () => {
    const c = document.createElement('canvas')
    c.id = 'probe'
    c.width = 40
    c.height = 40
    c.style.cssText = 'position:absolute;left:700px;top:300px;width:20px;height:20px'
    document.body.appendChild(c)
    const gl = c.getContext('webgl2', { antialias: true })
    if (!gl) return
    gl.clear(gl.COLOR_BUFFER_BIT)
    await new Promise((r) => setTimeout(r, 50))
    c.width = 20
    c.height = 20
    await new Promise<void>((r) =>
      requestAnimationFrame(() => {
        gl.viewport(0, 0, 20, 20)
        gl.clearColor(0, 0, 0, 1)
        gl.clear(gl.COLOR_BUFFER_BIT)
        r()
      }),
    )
    await new Promise((r) => setTimeout(r, 100))
  })
  const img = decodePng(await page.locator('#probe').screenshot())
  await page.evaluate(() => document.getElementById('probe')?.remove())
  return isDark(img.at(10, 10))
}

// Minimal WebGPU shapes for the probe (no @webgpu/types dependency).
interface ProbeEncoder {
  beginRenderPass(desc: unknown): { end(): void }
  finish(): unknown
}
interface ProbeDevice {
  lost: Promise<{ message: string }>
  createCommandEncoder(): ProbeEncoder
  queue: { submit(buffers: unknown[]): void }
}
interface ProbeGpu {
  requestAdapter(): Promise<{ requestDevice(): Promise<ProbeDevice> } | null>
  getPreferredCanvasFormat(): string
}
interface ProbeContext {
  configure(config: unknown): void
  getCurrentTexture(): { createView(): unknown }
}

/**
 * Plain WebGPU (no Dotloom code): request a device, clear a canvas to red, wait,
 * and report device loss or a non-displayed frame. Used to tell environment
 * problems (e.g. software adapters losing devices) from renderer bugs.
 */
async function rawWebgpu(page: Page): Promise<{ ok: boolean; reason: string }> {
  const r = await page.evaluate(async () => {
    const gpu = (navigator as unknown as { gpu?: ProbeGpu }).gpu
    if (!gpu) return { lost: 'no navigator.gpu' }
    const adapter = await gpu.requestAdapter()
    if (!adapter) return { lost: 'no adapter' }
    const device = await adapter.requestDevice()
    let lost: string | null = null
    device.lost.then((i) => {
      lost = i.message
    })
    const c = document.createElement('canvas')
    c.id = 'probe-gpu'
    c.width = 20
    c.height = 20
    c.style.cssText = 'position:absolute;left:700px;top:340px;width:20px;height:20px'
    document.body.appendChild(c)
    const ctx = c.getContext('webgpu') as unknown as ProbeContext
    ctx.configure({ device, format: gpu.getPreferredCanvasFormat(), alphaMode: 'opaque' })
    const draw = (): void => {
      const enc = device.createCommandEncoder()
      const pass = enc.beginRenderPass({
        colorAttachments: [
          {
            view: ctx.getCurrentTexture().createView(),
            loadOp: 'clear',
            storeOp: 'store',
            clearValue: { r: 1, g: 0, b: 0, a: 1 },
          },
        ],
      })
      pass.end()
      device.queue.submit([enc.finish()])
    }
    draw()
    await new Promise((res) => setTimeout(res, 300))
    if (!lost) await new Promise<void>((res) => requestAnimationFrame(() => (draw(), res())))
    await new Promise((res) => setTimeout(res, 100))
    return { lost }
  })
  if (r.lost) return { ok: false, reason: `plain WebGPU device lost: ${r.lost}` }
  const img = decodePng(await page.locator('#probe-gpu').screenshot())
  await page.evaluate(() => document.getElementById('probe-gpu')?.remove())
  return isRed(img.at(10, 10)) ? { ok: true, reason: '' } : { ok: false, reason: 'plain WebGPU frame not displayed' }
}

async function shot(page: Page): Promise<Rgba> {
  const buf = await page.locator('#stage canvas').screenshot()
  // Keep the evidence: the composited screenshot and the canvas contents read in
  // the same task as a fresh frame (helps tell compositor issues from rendering).
  await test.info().attach('canvas-screenshot.png', { body: buf, contentType: 'image/png' })
  const dataUrl = await page.evaluate(() => {
    const v = window.dl.editor?.viewport
    if (!v) return ''
    v.requestRender()
    ;(v as unknown as { dirty: boolean }).dirty = true
    v.frame()
    return v.canvas.toDataURL('image/png')
  })
  const diag = await page.evaluate(() => ({
    events: window.dl.events,
    stats: window.dl.editor?.viewport.lastStats,
    info: window.dl.editor?.viewport.info,
  }))
  await test.info().attach('viewport.json', { body: JSON.stringify(diag, null, 2), contentType: 'application/json' })
  if (dataUrl.startsWith('data:image/png;base64,')) {
    await test.info().attach('canvas-readback.png', {
      body: Buffer.from(dataUrl.slice('data:image/png;base64,'.length), 'base64'),
      contentType: 'image/png',
    })
  }
  return decodePng(buf)
}

const isDark = (p: number[]): boolean => (p[0] ?? 255) < 90 && (p[1] ?? 255) < 90 && (p[2] ?? 255) < 90
const isWhite = (p: number[]): boolean => (p[0] ?? 0) > 240 && (p[1] ?? 0) > 240 && (p[2] ?? 0) > 240
const isRed = (p: number[]): boolean => (p[0] ?? 0) > 220 && (p[1] ?? 255) < 40 && (p[2] ?? 255) < 40

function inkIn(img: Rgba, x0: number, y0: number, x1: number, y1: number): number {
  let n = 0
  for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) if (!isWhite(img.at(x, y))) n++
  return n
}

for (const backend of BACKENDS) {
  test.describe(backend, () => {
    // Pixel positions below assume CSS px = device px (some device profiles default to 2).
    test.use({ deviceScaleFactor: 1 })
    test('draws lines, fills and Turkish text at the right pixels', async ({ page, browserName }) => {
      const r = await open(page, backend, browserName)
      expect(r.backend).toBe(backend)
      await drawSample(page)
      const img = await shot(page)
      expect(img.width).toBe(640)
      // World (x, y) → CSS (320 + x, 200 − y) at scale 1.
      expect(isDark(img.at(320, 200)), 'line').toBe(true)
      expect(isWhite(img.at(320, 190)), 'line is thin').toBe(true)
      expect(isRed(img.at(420, 120)), 'rect fill').toBe(true)
      expect(isWhite(img.at(20, 20)), 'background').toBe(true)
      expect(inkIn(img, 70, 270, 330, 310), 'text ink').toBeGreaterThan(150)
      const stats = await page.evaluate(() => window.dl.editor?.viewport.lastStats)
      expect(stats?.glyphs).toBeGreaterThanOrEqual(10)
    })

    test.describe('hidpi', () => {
      test.use({ deviceScaleFactor: 2 })
      test('device pixel ratio 2 renders at 2x without double scaling', async ({ page, browserName }) => {
        await open(page, backend, browserName)
        await drawSample(page)
        const size = await page.evaluate(() => {
          const c = document.querySelector('#stage canvas') as HTMLCanvasElement
          return [c.width, c.height, window.dl.editor?.viewport.size.dpr]
        })
        expect(size).toEqual([1280, 800, 2])
        const img = await shot(page)
        expect(img.width).toBe(1280)
        expect(isDark(img.at(640, 400)), 'line at 2x').toBe(true)
        expect(isRed(img.at(840, 240)), 'rect at 2x').toBe(true)
        // 3 CSS px stroke = ~6 device px.
        let dark = 0
        for (let y = 380; y < 420; y++) if (isDark(img.at(640, y))) dark++
        expect(dark).toBeGreaterThanOrEqual(5)
        expect(dark).toBeLessThanOrEqual(8)
      })
    })

    test('pan, zoom and resize keep coordinates consistent', async ({ page, browserName }) => {
      await open(page, backend, browserName)
      if (backend === 'webgl2') {
        const ok = await rawWebglResizeComposites(page)
        test.skip(!ok, `${browserName} does not display resized WebGL2 canvases (reproduced with plain WebGL)`)
      }
      await drawSample(page)
      await page.evaluate(async () => {
        const v = window.dl.editor?.viewport
        v?.zoomAt(320, 200, 2)
        await window.dl.nextFrame()
      })
      let img = await shot(page)
      // Rect corner (50, 50) moves to CSS (420, 100) at scale 2 around the center.
      expect(isRed(img.at(430, 90)), 'zoomed rect').toBe(true)
      await page.evaluate(async () => {
        const stage = document.getElementById('stage') as HTMLDivElement
        stage.style.width = '320px'
        stage.style.height = '200px'
        await new Promise((r) => setTimeout(r, 100))
        await window.dl.nextFrame()
      })
      const size = await page.evaluate(() => {
        const c = document.querySelector('#stage canvas') as HTMLCanvasElement
        return [c.width, c.height]
      })
      expect(size).toEqual([320, 200])
      img = await shot(page)
      expect(isDark(img.at(160, 100)), 'line stays at the center').toBe(true)
    })

    test('recovers from GPU device/context loss', async ({ page, browserName }) => {
      await open(page, backend, browserName)
      await drawSample(page)
      await page.evaluate(async () => {
        const v = window.dl.editor?.viewport
        const restored = new Promise((r) => v?.on('restored', r))
        v?.simulateContextLoss()
        await restored
        await window.dl.nextFrame()
      })
      const events = await page.evaluate(() => window.dl.events)
      expect(events.some((e) => e.startsWith('lost:'))).toBe(true)
      expect(events.some((e) => e.startsWith('restored:'))).toBe(true)
      const img = await shot(page)
      expect(isDark(img.at(320, 200)), 'scene redrawn after recovery').toBe(true)
      expect(isRed(img.at(420, 120))).toBe(true)
    })

    test('dispose releases the canvas and stops rendering', async ({ page, browserName }) => {
      await open(page, backend, browserName)
      await drawSample(page)
      const left = await page.evaluate(() => {
        window.dl.editor?.dispose()
        return document.querySelectorAll('#stage canvas').length
      })
      expect(left).toBe(0)
    })
  })
}
