import { expect, test } from '@playwright/test'

// Diagnostic: WebGL2 calls per Dotloom frame (pan at the fitted view). Synchronous
// calls (getError, getParameter, getSyncParameter, clientWaitSync, readPixels, …)
// stall browsers that run WebGL in a separate GPU process.

test('WebGL2 calls per frame', async ({ page }, info) => {
  test.skip(info.project.metadata.backend !== 'webgl2', 'WebGL2 projects only')
  await page.addInitScript(() => {
    const counts: Record<string, number> = {}
    const proto = WebGL2RenderingContext.prototype as unknown as Record<string, unknown>
    for (const name of Object.getOwnPropertyNames(proto)) {
      const d = Object.getOwnPropertyDescriptor(proto, name)
      if (!d || typeof d.value !== 'function' || name === 'constructor') continue
      const orig = d.value as (...a: unknown[]) => unknown
      proto[name] = function (this: unknown, ...args: unknown[]) {
        counts[name] = (counts[name] ?? 0) + 1
        return orig.apply(this, args)
      }
    }
    ;(window as unknown as { glCounts: Record<string, number> }).glCounts = counts
  })
  await page.goto('./bench.html?backend=webgl2')
  await page.evaluate(() => window.bench.ready)
  await page.evaluate(() => window.bench.load(2000))
  const perFrame = await page.evaluate(async () => {
    const counts = (window as unknown as { glCounts: Record<string, number> }).glCounts
    for (const k of Object.keys(counts)) delete counts[k]
    await window.bench.rafFrames(120)
    const out: Record<string, number> = {}
    for (const [k, v] of Object.entries(counts)) out[k] = Math.round((v / 120) * 100) / 100
    return out
  })
  const sorted = Object.entries(perFrame).sort((a, b) => b[1] - a[1])
  console.log(info.project.name, JSON.stringify(Object.fromEntries(sorted)))
  // Calls that wait for the GPU process in browsers with out-of-process WebGL.
  const blocking = ['getError', 'readPixels', 'getBufferSubData', 'finish', 'clientWaitSync', 'getParameter']
  expect(blocking.filter((name) => (perFrame[name] ?? 0) > 0)).toEqual([])
})
