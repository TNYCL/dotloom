/**
 * The reference React editor (apps/playground) in real browsers (DL-UI, DL-EXAMPLE).
 */

import { readFileSync } from 'node:fs'
import { expect, type Page, test } from '@playwright/test'
import { decodePng } from './png.js'
import { webgl2Environment } from './probes.js'

test.use({ baseURL: 'http://localhost:5198/', deviceScaleFactor: 1 })

async function ready(page: Page): Promise<void> {
  await page.waitForFunction(() => 'dotloom' in window, null, { timeout: 30_000 })
  // Wait until rendering runs on a stable backend (see Viewport.whenStable).
  await page.evaluate(async () => {
    const v = (window as unknown as { dotloom: { viewport: { whenStable?: () => Promise<void> } } }).dotloom.viewport
    await v.whenStable?.()
  })
}

async function entityCount(page: Page): Promise<number> {
  return page.evaluate(async () => {
    const h = (window as unknown as { dotloom: { engine: { documentJson(): Promise<{ entities: unknown[] }> } } })
      .dotloom
    return (await h.engine.documentJson()).entities.length
  })
}

async function selectObject(page: Page, name: RegExp): Promise<void> {
  await page.getByRole('listbox', { name: 'Objects' }).getByText(name).click()
}

test('shelf example: 160 cm → 60/50/50, 130 cm rejected with the 140 cm bound, unlock, undo/redo', async ({ page }) => {
  await page.goto('./?example=shelf')
  await ready(page)
  await expect.poll(() => entityCount(page)).toBe(1)
  await selectObject(page, /Shelf/)
  const width = page.getByRole('textbox', { name: 'Inner width', exact: true })
  await expect(width).toHaveValue('180')
  await expect(page.getByRole('button', { name: 'Unlock value: Left compartment' })).toBeVisible()
  await width.fill('160')
  await width.press('Enter')
  await expect(page.getByRole('textbox', { name: 'Middle compartment', exact: true })).toHaveValue('50')
  await expect(page.getByRole('textbox', { name: 'Right compartment', exact: true })).toHaveValue('50')
  await expect(page.getByRole('textbox', { name: 'Left compartment', exact: true })).toHaveValue('60')
  await width.fill('130')
  await width.press('Enter')
  const suggestion = page.getByRole('button', { name: 'Use nearest allowed value 140 cm' })
  await expect(suggestion).toBeVisible()
  await expect(page.getByRole('alert').first()).toContainText('compartments fill the inner width')
  await suggestion.click()
  await expect(width).toHaveValue('140')
  await page.getByRole('button', { name: 'Unlock value: Left compartment' }).click()
  await width.fill('130')
  await width.press('Enter')
  await expect(width).toHaveValue('130')
  await page.getByRole('button', { name: 'Undo' }).click()
  await page.getByRole('button', { name: 'Undo' }).click()
  await expect(width).toHaveValue('140')
  await expect(page.getByRole('button', { name: 'Unlock value: Left compartment' })).toBeVisible()
  await page.getByRole('button', { name: 'Redo' }).click()
  await expect(page.getByRole('button', { name: 'Lock value: Left compartment' })).toBeVisible()
})

test('room planner example renders, lists walls and doors, and switches to the dark theme', async ({ page }) => {
  await page.goto('./?example=floorplan')
  await ready(page)
  const backend = await page.evaluate(
    () => (window as unknown as { dotloom: { canvas: { backend: string } } }).dotloom.canvas.backend,
  )
  if (backend === 'webgl2') {
    const problem = await webgl2Environment(page)
    test.skip(problem !== null, `${test.info().project.name}: ${problem} (environment, reproduced with plain WebGL)`)
  }
  await expect.poll(() => entityCount(page)).toBe(6)
  const list = page.getByRole('listbox', { name: 'Objects' })
  await expect(list.getByText(/Wall 1/)).toBeVisible()
  await expect(list.getByText(/Entrance/)).toBeVisible()
  const canvas = page.locator('.dl-canvas canvas')
  await page.waitForTimeout(300)
  let img = decodePng(await canvas.screenshot())
  let ink = 0
  for (let i = 0; i < img.data.length; i += 4 * 97) if ((img.data[i] ?? 255) < 200) ink++
  expect(ink, 'drawn walls').toBeGreaterThan(20)
  await page.getByRole('combobox', { name: 'Theme' }).selectOption('dark')
  await expect(page.locator('.dl-editor')).toHaveAttribute('data-theme', 'dark')
  await page.waitForTimeout(300)
  img = decodePng(await canvas.screenshot())
  let dark = 0
  let total = 0
  for (let i = 0; i < img.data.length; i += 4 * 53) {
    total++
    if ((img.data[i] ?? 255) < 60) dark++
  }
  expect(dark / total, 'mostly dark canvas background').toBeGreaterThan(0.6)
})

test('command palette exports SVG; save and reopen a .dotl file', async ({ page }) => {
  await page.goto('./?example=timeline')
  await ready(page)
  await expect.poll(() => entityCount(page)).toBe(4)
  await page.locator('.dl-canvas canvas').click({ position: { x: 5, y: 5 } })
  await page.keyboard.press('ControlOrMeta+k')
  const input = page.getByPlaceholder('Type a command…')
  await input.fill('svg')
  const [svgDownload] = await Promise.all([page.waitForEvent('download'), input.press('Enter')])
  const svg = readFileSync(await svgDownload.path(), 'utf8')
  expect(svg).toContain('<svg')
  expect(svgDownload.suggestedFilename()).toMatch(/\.svg$/)

  const [dotl] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('menuitem', { name: 'Save .dotl' }).click(),
  ])
  const path = await dotl.path()
  expect(dotl.suggestedFilename()).toMatch(/\.dotl$/)
  await page.getByRole('menuitem', { name: 'New' }).click()
  await expect.poll(() => entityCount(page)).toBe(0)
  const [chooser] = await Promise.all([
    page.waitForEvent('filechooser'),
    page.getByRole('menuitem', { name: 'Open…' }).click(),
  ])
  await chooser.setFiles({
    name: 'zaman-cizelgesi.dotl',
    mimeType: 'application/vnd.dotloom+zip',
    buffer: readFileSync(path),
  })
  await expect.poll(() => entityCount(page)).toBe(4)
})

test('empty drawing note, Ctrl+S / Ctrl+O, DXF and PNG export, visible keyboard focus', async ({ page }) => {
  await page.goto('./')
  await ready(page)
  await expect(page.getByText('Empty drawing')).toBeVisible()
  await page.evaluate(async () => {
    const h = (window as unknown as { dotloom: { core: { apply(tx: unknown): Promise<unknown> } } }).dotloom
    await h.core.apply({
      label: 'Line',
      commands: [{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [1000, 500] } } }],
    })
  })
  await expect(page.getByText('Empty drawing')).toBeHidden()

  // Ctrl+S downloads the document and the status bar reports it as saved.
  await page.locator('.dl-canvas canvas').click({ position: { x: 5, y: 5 } })
  const [saved] = await Promise.all([page.waitForEvent('download'), page.keyboard.press('ControlOrMeta+s')])
  expect(saved.suggestedFilename()).toMatch(/\.dotl$/)
  await expect(page.locator('.dl-statusbar')).toContainText('Saved')
  const dotl = readFileSync(await saved.path())

  // DXF and PNG through the command palette.
  const exportVia = async (query: string) => {
    await page.keyboard.press('ControlOrMeta+k')
    const input = page.getByPlaceholder('Type a command…')
    await input.fill(query)
    const [d] = await Promise.all([page.waitForEvent('download'), input.press('Enter')])
    return { name: d.suggestedFilename(), bytes: readFileSync(await d.path()) }
  }
  const dxf = await exportVia('dxf')
  expect(dxf.name).toMatch(/\.dxf$/)
  expect(dxf.bytes.toString('latin1')).toMatch(/SECTION[\s\S]*ENTITIES[\s\S]*LINE/)
  const png = await exportVia('png')
  expect(png.name).toMatch(/\.png$/)
  const img = decodePng(png.bytes)
  expect(img.width).toBeGreaterThan(0)
  const background = img.at(0, 0)
  let ink = 0
  for (let y = 0; y < img.height; y += 2)
    for (let x = 0; x < img.width; x += 2)
      if (img.at(x, y).some((c, i) => Math.abs(c - (background[i] ?? 0)) > 40)) ink++
  expect(ink, 'the exported PNG shows the line').toBeGreaterThan(10)

  // Ctrl+O opens a file (here the one saved above, after starting a new document).
  await page.getByRole('menuitem', { name: 'New' }).click()
  await expect.poll(() => entityCount(page)).toBe(0)
  await page.locator('.dl-canvas canvas').click({ position: { x: 5, y: 5 } })
  const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.keyboard.press('ControlOrMeta+o')])
  await chooser.setFiles({ name: 'line.dotl', mimeType: 'application/vnd.dotloom+zip', buffer: dotl })
  await expect.poll(() => entityCount(page)).toBe(1)

  // Keyboard focus is visible on editor controls.
  await page.getByRole('menuitem', { name: 'New' }).focus()
  await page.keyboard.press('Tab')
  const outline = await page.evaluate(() => {
    const el = document.activeElement as HTMLElement | null
    if (!el || el === document.body) return null
    const cs = getComputedStyle(el)
    return { style: cs.outlineStyle, width: Number.parseFloat(cs.outlineWidth) }
  })
  expect(outline).not.toBeNull()
  expect(outline?.style).not.toBe('none')
  expect(outline?.width).toBeGreaterThanOrEqual(2)
})

test('unsaved work is offered for recovery after a reload', async ({ page }) => {
  await page.goto('./?example=floorplan')
  await ready(page)
  await expect.poll(() => entityCount(page)).toBe(6)
  // Change something, wait for the autosave, reload without saving.
  await selectObject(page, /Entrance/)
  const width = page.getByRole('textbox', { name: 'Width', exact: true })
  await width.fill('1')
  await width.press('Enter')
  await expect(width).toHaveValue('1')
  await expect(page.locator('.dl-statusbar')).toContainText(/Autosaved/)
  await page.goto('./')
  await ready(page)
  const dialog = page.getByRole('dialog', { name: 'Restore unsaved work?' })
  await expect(dialog).toBeVisible()
  await dialog.getByRole('button', { name: 'Restore' }).click()
  await expect.poll(() => entityCount(page)).toBe(6)
  await selectObject(page, /Entrance/)
  await expect(page.getByRole('textbox', { name: 'Width', exact: true })).toHaveValue('1')
})

test('keyboard-only: reach an object through the list and edit it; Turkish UI', async ({ browser }) => {
  const ctx = await browser.newContext({ locale: 'tr-TR', baseURL: 'http://localhost:5198/' })
  const page = await ctx.newPage()
  try {
    await page.goto('./?example=shelf')
    await ready(page)
    await expect(page.getByRole('heading', { name: 'Özellikler' })).toBeVisible()
    await expect.poll(() => entityCount(page)).toBe(1)
    const list = page.getByRole('listbox', { name: 'Nesneler' })
    await list.focus()
    await page.keyboard.press('Home')
    await page.keyboard.press('Space')
    const width = page.getByRole('textbox', { name: 'Inner width', exact: true })
    await expect(width).toBeVisible()
    await width.focus()
    await page.keyboard.press('ControlOrMeta+a')
    await page.keyboard.type('1,7 m')
    await page.keyboard.press('Enter')
    await expect(width).toHaveValue('170')
    await expect(page.getByText('Değerler cm cinsinden')).toBeVisible()
  } finally {
    await ctx.close()
  }
})
