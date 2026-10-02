/**
 * The external plugin app, built outside the repository from the packed tarballs
 * (scripts/smoke-packages.mjs), running in a real browser — once at the server root
 * (relative base) and once built for and served under /dotloom/ (DL-SDK-10).
 */

import { expect, type Page, test } from '@playwright/test'

const sites = [
  { name: 'at the root', url: 'http://localhost:5201/' },
  { name: 'under /dotloom/', url: 'http://localhost:5202/dotloom/' },
]

type Handle = {
  engine: {
    documentJson(): Promise<{ entities: { type: string }[] }>
    apply(c: unknown[]): Promise<unknown>
    newDocument(): Promise<unknown>
  }
  canvas: {
    exportPng(o: { grid: boolean }): Promise<Blob>
    fit(): Promise<void>
    whenStable(): Promise<void>
  } | null
}

/** Pixels of an exported PNG that differ from its corner (background) pixel. */
async function inkPixels(page: Page): Promise<number> {
  return page.evaluate(async () => {
    const h = (window as unknown as { dotloom: Handle }).dotloom
    if (!h.canvas) return -1
    // A software WebGPU device can be lost right after start; the viewport then moves
    // to WebGL2. Export only once rendering runs on a stable backend.
    await h.canvas.whenStable()
    await h.canvas.fit()
    const bmp = await createImageBitmap(await h.canvas.exportPng({ grid: false }))
    const c = new OffscreenCanvas(bmp.width, bmp.height)
    const g = c.getContext('2d') as OffscreenCanvasRenderingContext2D
    g.drawImage(bmp, 0, 0)
    const d = g.getImageData(0, 0, bmp.width, bmp.height).data
    let ink = 0
    for (let i = 0; i < d.length; i += 4) {
      if (Math.abs((d[i] ?? 0) - (d[0] ?? 0)) + Math.abs((d[i + 1] ?? 0) - (d[1] ?? 0)) > 60) ink++
    }
    return ink
  })
}

for (const site of sites) {
  test(`the external app loads the packaged engine and uses the plugin tool ${site.name}`, async ({ page }) => {
    const failed: string[] = []
    const loaded: string[] = []
    page.on('response', (r) => {
      if (r.status() >= 400 && !r.url().includes('favicon')) failed.push(`${r.status()} ${r.url()}`)
      if (/\.(wasm|js)$/.test(new URL(r.url()).pathname)) loaded.push(new URL(r.url()).pathname)
    })
    await page.goto(site.url)
    await page.waitForFunction(() => 'dotloom' in window, null, { timeout: 45_000 })
    const tool = page.getByRole('button', { name: 'Table' })
    await expect(tool).toBeVisible()
    await tool.click()
    await page.locator('.dl-canvas canvas').click({ position: { x: 200, y: 200 } })
    await expect
      .poll(() =>
        page.evaluate(async () => {
          const h = (window as unknown as { dotloom: Handle }).dotloom
          return (await h.engine.documentJson()).entities.map((e) => e.type)
        }),
      )
      .toEqual(['acme.table'])
    expect(failed).toEqual([])
    // Worker, engine and renderer WASM came from this site's path.
    const prefix = new URL(site.url).pathname
    expect(loaded.filter((p) => p.endsWith('.wasm')).length).toBeGreaterThanOrEqual(2)
    expect(loaded.every((p) => p.startsWith(prefix))).toBe(true)

    // Text renders with the font embedded in the packaged renderer.
    await page.evaluate(async () => {
      const h = (window as unknown as { dotloom: Handle }).dotloom
      await h.engine.newDocument()
      await h.engine.apply([
        {
          op: 'createEntity',
          entity: { geometry: { type: 'text', position: [0, 0], content: 'Dotloom İğ', height: 50 } },
        },
      ])
    })
    await expect.poll(() => inkPixels(page)).toBeGreaterThan(200)
  })
}
