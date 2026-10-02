/** Environment probes with plain WebGL/WebGPU (no Dotloom code) and pixel helpers. */

import type { Page } from '@playwright/test'
import { decodePng } from './png.js'

export const isDark = (p: number[]): boolean => (p[0] ?? 255) < 90 && (p[1] ?? 255) < 90 && (p[2] ?? 255) < 90
export const isWhite = (p: number[]): boolean => (p[0] ?? 0) > 240 && (p[1] ?? 0) > 240 && (p[2] ?? 0) > 240
export const isRed = (p: number[]): boolean => (p[0] ?? 0) > 220 && (p[1] ?? 255) < 40 && (p[2] ?? 255) < 40

/**
 * Whether the browser composites a *resized* WebGL2 canvas at all (plain WebGL,
 * no Dotloom code). Some WebKit builds do not; tests depending on it are skipped
 * with this reason instead of being reported as passing.
 */
export async function rawWebglResizeComposites(page: Page): Promise<boolean> {
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
export async function rawWebgpu(page: Page): Promise<{ ok: boolean; reason: string }> {
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

/**
 * Whether a WebGL2 context created with `antialias: false` (as wgpu does) is
 * displayed. Some WebKit builds never composite such contexts.
 */
export async function rawWebglNoAntialiasComposites(page: Page): Promise<boolean> {
  await page.evaluate(() => {
    const c = document.createElement('canvas')
    c.id = 'probe-noaa'
    c.width = 20
    c.height = 20
    c.style.cssText = 'position:absolute;left:700px;top:380px;width:20px;height:20px'
    document.body.appendChild(c)
    const gl = c.getContext('webgl2', { antialias: false })
    if (!gl) return
    gl.clearColor(0, 0, 0, 1)
    gl.clear(gl.COLOR_BUFFER_BIT)
  })
  await page.waitForTimeout(100)
  const img = decodePng(await page.locator('#probe-noaa').screenshot())
  await page.evaluate(() => document.getElementById('probe-noaa')?.remove())
  return isDark(img.at(10, 10))
}

/** Skip with a reason when plain WebGL2 shows the compositor problems wgpu would hit. */
export async function webgl2Environment(page: Page): Promise<string | null> {
  if (!(await rawWebglNoAntialiasComposites(page))) return 'WebGL2 contexts without antialiasing are not displayed'
  if (!(await rawWebglResizeComposites(page))) return 'resized WebGL2 canvases are not displayed'
  return null
}
