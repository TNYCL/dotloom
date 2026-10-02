/**
 * The published site layout (docs, API references, playground, examples) under the
 * /dotloom/ sub-path — locally (scripts/serve-site.mjs) or against the deployed URL
 * (DOTLOOM_SITE_URL). Checks that WASM, Worker and font assets load from the
 * sub-path, not from the repository root.
 */

import { readFileSync } from 'node:fs'
import { expect, type Page, test } from '@playwright/test'

async function editorReady(page: Page): Promise<void> {
  await page.waitForFunction(() => 'dotloom' in window, null, { timeout: 45_000 })
}

test('docs home and guide pages', async ({ page }) => {
  const res = await page.goto('./')
  expect(res?.status()).toBe(200)
  await expect(page.getByRole('heading', { name: 'Dotloom', level: 1 })).toBeVisible()
  await page.goto('./guide/getting-started.html')
  await expect(page.getByRole('heading', { name: 'Getting started' })).toBeVisible()
  await page.goto('./guide/file-format.html')
  await expect(page.getByRole('heading', { name: /file format/ })).toBeVisible()
})

test('API references are published', async ({ page }) => {
  expect((await page.goto('./api/ts/index.html'))?.status()).toBe(200)
  await expect(page.locator('body')).toContainText('DotloomEngine')
  expect((await page.goto('./api/rust/dotloom_engine/index.html'))?.status()).toBe(200)
  await expect(page.locator('body')).toContainText('dotloom_engine')
})

test('playground loads its worker, WASM and renders from the sub-path', async ({ page }) => {
  const failed: string[] = []
  page.on('response', (r) => {
    if (r.status() >= 400) failed.push(`${r.status()} ${r.url()}`)
  })
  await page.goto('./playground/?example=floorplan')
  await editorReady(page)
  const info = await page.evaluate(() => {
    const h = (window as unknown as { dotloom: { canvas: { info: unknown } | null } }).dotloom
    return h.canvas?.info ?? null
  })
  expect(info, 'a renderer backend started').not.toBeNull()
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const h = (window as unknown as { dotloom: { engine: { documentJson(): Promise<{ entities: unknown[] }> } } })
          .dotloom
        return (await h.engine.documentJson()).entities.length
      }),
    )
    .toBe(6)
  expect(failed.filter((f) => !f.includes('favicon'))).toEqual([])
})

for (const name of ['vanilla', 'shelf-configurator', 'floorplan', 'timeline']) {
  test(`example ${name} starts`, async ({ page }) => {
    const failed: string[] = []
    page.on('response', (r) => {
      if (r.status() >= 400 && !r.url().includes('favicon')) failed.push(`${r.status()} ${r.url()}`)
    })
    await page.goto(`./examples/${name}/`)
    await editorReady(page)
    expect(failed).toEqual([])
  })
}

test('vanilla example: toolbar, drawing, undo/redo, save, SVG export and open without React', async ({ page }) => {
  await page.goto('./examples/vanilla/')
  await editorReady(page)
  const count = () =>
    page.evaluate(async () => {
      const h = (window as unknown as { dotloom: { engine: { documentJson(): Promise<{ entities: unknown[] }> } } })
        .dotloom
      return (await h.engine.documentJson()).entities.length
    })
  // Tool labels and prompts are shown as text, never as raw message keys.
  const status = page.getByRole('status')
  await expect(status).toHaveText(/^Click to select/)
  const line = page.getByRole('button', { name: 'Line', exact: true })
  await expect(line).toHaveAttribute('data-tool', 'line')
  await line.click()
  await expect(line).toHaveAttribute('aria-pressed', 'true')
  await expect(status).toHaveText('Line: click the start point.')
  expect(await page.locator('#bar').textContent()).not.toMatch(/tool\./)
  const canvas = page.locator('#editor canvas')
  await canvas.click({ position: { x: 120, y: 120 } })
  await canvas.click({ position: { x: 320, y: 220 } })
  await page.keyboard.press('Escape')
  await expect.poll(count).toBe(1)
  await page.getByRole('button', { name: 'Undo' }).click()
  await expect.poll(count).toBe(0)
  await page.getByRole('button', { name: 'Redo' }).click()
  await expect.poll(count).toBe(1)

  const [saved] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Save' }).click()])
  expect(saved.suggestedFilename()).toBe('drawing.dotl')
  const dotl = readFileSync(await saved.path())
  expect(dotl.subarray(0, 2).toString('latin1')).toBe('PK')
  const [svg] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Export SVG' }).click(),
  ])
  expect(readFileSync(await svg.path(), 'utf8')).toMatch(/<svg[\s\S]*<(path|line|polyline)/)

  await page.getByRole('button', { name: 'Undo' }).click()
  await expect.poll(count).toBe(0)
  const [chooser] = await Promise.all([
    page.waitForEvent('filechooser'),
    page.getByRole('button', { name: 'Open…' }).click(),
  ])
  await chooser.setFiles({ name: 'drawing.dotl', mimeType: 'application/vnd.dotloom+zip', buffer: dotl })
  await expect.poll(count).toBe(1)
})
