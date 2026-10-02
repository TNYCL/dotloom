/**
 * Editor interaction and engine protocol in real browsers (DL-INPUT, DL-SDK, DL-SOLVE-11).
 * Uses the default backend order (WebGPU, then WebGL2).
 */

import { expect, type Page, test } from '@playwright/test'

async function open(page: Page): Promise<void> {
  await page.goto('./')
  const r = (await page.evaluate(() => window.dl.ready)) as { ok: boolean; error?: string }
  expect(r.ok, r.error).toBe(true)
}

async function count(page: Page): Promise<number> {
  return page.evaluate(async () => (await window.dl.editor?.engine.documentJson())?.entities.length ?? -1)
}

test('draw a line with the mouse, select it and delete it with the keyboard', async ({ page }) => {
  await open(page)
  const canvas = page.locator('#stage canvas')
  await canvas.click({ position: { x: 10, y: 10 } })
  await page.keyboard.press('l')
  await expect.poll(() => page.evaluate(() => window.dl.editor?.core.state.getSnapshot().tool)).toBe('line')
  await canvas.click({ position: { x: 100, y: 200 } })
  await canvas.click({ position: { x: 300, y: 200 } })
  await page.keyboard.press('Escape')
  await expect.poll(() => count(page)).toBe(1)
  const g = await page.evaluate(async () => (await window.dl.editor?.engine.documentJson())?.entities[0]?.geometry)
  expect(g).toEqual({ type: 'line', a: [-220, 0], b: [-20, 0] })
  await page.keyboard.press('v')
  await canvas.click({ position: { x: 200, y: 200 } })
  await expect.poll(() => page.evaluate(() => window.dl.editor?.core.state.getSnapshot().selection.length)).toBe(1)
  await page.keyboard.press('Delete')
  await expect.poll(() => count(page)).toBe(0)
  await page.keyboard.press('Control+z')
  await expect.poll(() => count(page)).toBe(1)
})

test('typing in a form field never triggers editor shortcuts', async ({ page }) => {
  await open(page)
  await page.evaluate(async () => {
    await window.dl.apply([{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [50, 0] } } }])
    const ids = (await window.dl.editor?.engine.documentJson())?.entities.map((e) => e.id) ?? []
    await window.dl.editor?.engine.setSelection(ids)
  })
  await page.locator('#field').fill('')
  await page.locator('#field').type('line l, delete')
  await page.locator('#field').press('Backspace')
  await page.locator('#field').press('Delete')
  await page.locator('#field').press('Control+a')
  expect(await page.locator('#field').inputValue()).toBe('line l, delet')
  expect(await count(page)).toBe(1)
  expect(await page.evaluate(() => window.dl.editor?.core.state.getSnapshot().tool)).toBe('select')
})

test('marquee selection with a real drag', async ({ page }) => {
  await open(page)
  await page.evaluate(async () => {
    await window.dl.apply([
      { op: 'createEntity', entity: { geometry: { type: 'rect', origin: [0, 0], width: 40, height: 40 } } },
      { op: 'createEntity', entity: { geometry: { type: 'rect', origin: [100, 0], width: 40, height: 40 } } },
    ])
  })
  const box = await page.locator('#stage canvas').boundingBox()
  if (!box) throw new Error('no canvas')
  // Window selection (left → right) around the first rect only: world (−10..60, −10..60).
  await page.mouse.move(box.x + 310, box.y + 210)
  await page.mouse.down()
  await page.mouse.move(box.x + 340, box.y + 180, { steps: 5 })
  await page.mouse.move(box.x + 380, box.y + 140, { steps: 5 })
  await page.mouse.up()
  await expect.poll(() => page.evaluate(() => window.dl.editor?.core.state.getSnapshot().selection.length)).toBe(1)
})

test('a long solve in the worker is cancelled for real and leaves the document unchanged', async ({ page }) => {
  await open(page)
  const r = await page.evaluate(async () => {
    const engine = window.dl.editor?.engine
    if (!engine) throw new Error('no engine')
    const ids = await engine.reserveIds(80)
    const commands: unknown[] = []
    for (let i = 0; i < 80; i++) {
      commands.push({
        op: 'createEntity',
        id: ids[i],
        entity: { geometry: { type: 'line', a: [i * 10, 0], b: [i * 10 + 10, 0] } },
      })
    }
    for (let i = 0; i < 79; i++) {
      commands.push({
        op: 'addConstraint',
        constraint: {
          rule: {
            kind: 'coincident',
            a: { entity: ids[i], anchor: 'end' },
            b: { entity: ids[i + 1], anchor: 'start' },
          },
        },
      })
      commands.push({
        op: 'addConstraint',
        constraint: {
          rule: {
            kind: 'length',
            line: { from: { entity: ids[i], anchor: 'start' }, to: { entity: ids[i], anchor: 'end' } },
            value: 10,
          },
        },
      })
    }
    await engine.apply(commands as never)
    const before = JSON.stringify((await engine.documentJson()).entities)
    const rev = engine.revision
    const ctrl = new AbortController()
    let progressed = 0
    const off = engine.on('progress', (p) => {
      progressed = p.iterations
      ctrl.abort()
    })
    const t0 = performance.now()
    const outcome = await engine
      .apply([{ op: 'setParams', values: [{ entity: ids[40] as number, param: 'a.y', value: 400 }], mode: 'prefer' }], {
        signal: ctrl.signal,
      })
      .then(
        () => 'committed',
        (e: { code?: string }) => e.code,
      )
    const ms = performance.now() - t0
    off()
    const after = JSON.stringify((await engine.documentJson()).entities)
    // The engine is still usable.
    await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [0, 100] } } }])
    return { outcome, progressed, unchanged: before === after, sameRevision: rev === engine.revision - 1, ms }
  })
  expect(r.outcome).toBe('cancelled')
  expect(r.progressed).toBeGreaterThan(0)
  expect(r.unchanged).toBe(true)
  expect(r.sameRevision).toBe(true)
})

test('worker crash is reported and a new engine reopens the saved document', async ({ page }) => {
  await open(page)
  const r = await page.evaluate(async () => {
    const editor = window.dl.editor
    if (!editor) throw new Error('no editor')
    await editor.engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'circle', center: [0, 0], radius: 30 } } },
    ])
    const saved = await editor.engine.save()
    const crashed = new Promise<string>((res) => editor.engine.on('crash', (c) => res(c.message)))
    const code = await editor.engine.debugCrashForTesting().then(
      () => 'no-crash',
      (e: { code?: string }) => e.code,
    )
    const message = await crashed
    const after = await editor.engine.undo().then(
      () => 'ok',
      (e: { code?: string }) => e.code,
    )
    // Reopen in a fresh engine (new worker).
    const fresh = await window.dl.sdk.DotloomEngine.create()
    await fresh.load(saved)
    const n = (await fresh.documentJson()).entities.length
    fresh.dispose()
    return { code, message, after, crashed: editor.engine.isCrashed, n }
  })
  expect(r.code).toBe('crashed')
  expect(r.crashed).toBe(true)
  expect(r.after).toBe('crashed')
  expect(r.message.length).toBeGreaterThan(0)
  expect(r.n).toBe(1)
})

test('stale revisions are rejected through the worker', async ({ page }) => {
  await open(page)
  const code = await page.evaluate(async () => {
    const engine = window.dl.editor?.engine
    if (!engine) throw new Error('no engine')
    await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [1, 1] } } }])
    return engine
      .apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [2, 2] } } }], {
        expectedRevision: engine.revision - 1,
      })
      .then(
        () => 'ok',
        (e: { code?: string }) => e.code,
      )
  })
  expect(code).toBe('stale')
})
