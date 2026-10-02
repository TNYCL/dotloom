/**
 * The external plugin app, built outside the repository from the packed tarballs
 * (scripts/smoke-packages.mjs), running in a real browser.
 */

import { expect, test } from '@playwright/test'

test('the external app loads the packaged engine and uses the plugin tool', async ({ page }) => {
  const failed: string[] = []
  page.on('response', (r) => {
    if (r.status() >= 400 && !r.url().includes('favicon')) failed.push(`${r.status()} ${r.url()}`)
  })
  await page.goto('./')
  await page.waitForFunction(() => 'dotloom' in window, null, { timeout: 45_000 })
  const tool = page.getByRole('button', { name: 'Table' })
  await expect(tool).toBeVisible()
  await tool.click()
  await page.locator('.dl-canvas canvas').click({ position: { x: 200, y: 200 } })
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const h = (
          window as unknown as { dotloom: { engine: { documentJson(): Promise<{ entities: { type: string }[] }> } } }
        ).dotloom
        return (await h.engine.documentJson()).entities.map((e) => e.type)
      }),
    )
    .toEqual(['acme.table'])
  expect(failed).toEqual([])
})
