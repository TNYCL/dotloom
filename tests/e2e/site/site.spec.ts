/**
 * The published site layout (docs, API references, playground, examples) under the
 * /dotloom/ sub-path — locally (scripts/serve-site.mjs) or against the deployed URL
 * (DOTLOOM_SITE_URL). Checks that WASM, Worker and font assets load from the
 * sub-path, not from the repository root.
 */

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
