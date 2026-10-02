/**
 * Editor core and tools against the real engine (DL-INPUT, DL-CMD).
 * The viewport is a DOM-free fake with the same camera math as the real one.
 */

import { afterEach, describe, expect, it } from 'vitest'
import { type Camera, screenToWorld, type ViewSize, worldToScreen } from '../src/camera.js'
import { EditorCore } from '../src/editor/core.js'
import { isEditableTarget } from '../src/editor/dom.js'
import { builtinTools } from '../src/editor/tools/index.js'
import type { InputKey, InputPointer, Tool } from '../src/editor/types.js'
import type { DotloomEngine } from '../src/engine.js'
import { createNodeEngine } from '../src/node.js'
import type { Aabb, Entity, Point } from '../src/types.js'
import { DEFAULT_GRID, type GridSettings, type OverlayState, type ViewportLike } from '../src/viewport.js'

class FakeViewport implements ViewportLike {
  size: ViewSize = { width: 800, height: 600, dpr: 1 }
  camera: Camera = { center: [0, 0], scale: 1 }
  grid: GridSettings = { ...DEFAULT_GRID }
  overlay: OverlayState = {}
  hover: number | null = null
  setCamera(c: Camera): void {
    this.camera = c
  }
  screenToWorld(x: number, y: number): Point {
    return screenToWorld(this.camera, this.size, x, y)
  }
  worldToScreen(p: Point): Point {
    return worldToScreen(this.camera, this.size, p)
  }
  setOverlay(o: OverlayState): void {
    this.overlay = o
  }
  setHover(id: number | null): void {
    this.hover = id
  }
  setGrid(g: Partial<GridSettings>): void {
    this.grid = { ...this.grid, ...g }
  }
  async fit(_b?: Aabb | null): Promise<void> {}
  requestRender(): void {}
}

const cleanup: (() => void)[] = []
afterEach(() => {
  for (const f of cleanup.splice(0)) f()
})

async function setup(): Promise<{ engine: DotloomEngine; core: EditorCore; vp: FakeViewport }> {
  const engine = await createNodeEngine()
  const vp = new FakeViewport()
  const core = new EditorCore(engine, vp)
  for (const t of builtinTools()) core.registerTool(t)
  core.start()
  await core.idle()
  cleanup.push(() => {
    core.dispose()
    engine.dispose()
  })
  return { engine, core, vp }
}

function ptr(vp: FakeViewport, w: Point, extra: Partial<InputPointer> = {}): InputPointer {
  const [x, y] = vp.worldToScreen(w)
  return {
    x,
    y,
    button: 0,
    buttons: 1,
    shift: false,
    mod: false,
    alt: false,
    pointerId: 1,
    pointerType: 'mouse',
    ...extra,
  }
}

async function click(core: EditorCore, vp: FakeViewport, w: Point, extra: Partial<InputPointer> = {}): Promise<void> {
  void core.pointerDown(ptr(vp, w, extra))
  void core.pointerUp(ptr(vp, w, { ...extra, buttons: 0 }))
  await core.idle()
}

async function drag(core: EditorCore, vp: FakeViewport, from: Point, to: Point, steps = 4): Promise<void> {
  void core.pointerDown(ptr(vp, from))
  for (let i = 1; i <= steps; i++) {
    const t = i / steps
    void core.pointerMove(ptr(vp, [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]))
    await core.idle()
  }
  void core.pointerUp(ptr(vp, to, { buttons: 0 }))
  await core.idle()
}

function key(k: string, extra: Partial<InputKey> = {}): InputKey {
  return { key: k, code: k, shift: false, mod: false, alt: false, repeat: false, ...extra }
}

async function press(core: EditorCore, k: string, extra: Partial<InputKey> = {}): Promise<boolean> {
  const handled = core.keyDown(key(k, extra))
  await core.idle()
  return handled
}

async function type(core: EditorCore, s: string): Promise<void> {
  for (const ch of s) core.keyDown(key(ch))
  await press(core, 'Enter')
}

async function entities(engine: DotloomEngine): Promise<Entity[]> {
  return (await engine.documentJson()).entities
}

describe('drawing tools', () => {
  it('line: click-click, chains, Escape ends', async () => {
    const { engine, core, vp } = await setup()
    core.setTool('line')
    await click(core, vp, [0, 0])
    expect(core.state.getSnapshot().toolState).toBe('preview')
    void core.pointerMove(ptr(vp, [50, 0]))
    await core.idle()
    expect(vp.overlay.sketch?.[0]).toMatchObject({ type: 'line', a: [0, 0], b: [50, 0] })
    await click(core, vp, [100, 0])
    await click(core, vp, [100, 100])
    await press(core, 'Escape')
    const es = await entities(engine)
    expect(es.map((e) => e.geometry)).toEqual([
      { type: 'line', a: [0, 0], b: [100, 0] },
      { type: 'line', a: [100, 0], b: [100, 100] },
    ])
    expect(core.state.getSnapshot().toolState).toBe('idle')
  })

  it('typed length follows the pointer direction', async () => {
    const { engine, core, vp } = await setup()
    core.setTool('line')
    await click(core, vp, [0, 0])
    void core.pointerMove(ptr(vp, [30, 30]))
    await core.idle()
    await type(core, '100')
    const g = (await entities(engine))[0]?.geometry as { b: Point }
    expect(g.b[0]).toBeCloseTo(100 / Math.SQRT2, 9)
    expect(g.b[1]).toBeCloseTo(100 / Math.SQRT2, 9)
    // Explicit angle.
    await type(core, '50,90')
    const g2 = (await entities(engine))[1]?.geometry as { a: Point; b: Point }
    expect(g2.b[0]).toBeCloseTo(g2.a[0], 9)
    expect(g2.b[1] - g2.a[1]).toBeCloseTo(50, 9)
  })

  it('polyline: Backspace removes a vertex, Enter finishes; closing on the first point', async () => {
    const { engine, core, vp } = await setup()
    core.setTool('polyline')
    for (const p of [
      [0, 0],
      [100, 0],
      [100, 50],
      [200, 200],
    ] as Point[])
      await click(core, vp, p)
    await press(core, 'Backspace')
    await press(core, 'Enter')
    expect((await entities(engine))[0]?.geometry).toEqual({
      type: 'polyline',
      points: [
        [0, 0],
        [100, 0],
        [100, 50],
      ],
    })
    for (const p of [
      [0, 100],
      [50, 100],
      [50, 150],
      [0, 100],
    ] as Point[])
      await click(core, vp, p)
    expect((await entities(engine))[1]?.geometry).toMatchObject({ type: 'polyline', closed: true })
  })

  it('rect by drag and by typed size; circle by typed radius; arc through 3 points', async () => {
    const { engine, core, vp } = await setup()
    core.setTool('rect')
    await drag(core, vp, [0, 0], [100, 50])
    core.setTool('rect')
    await click(core, vp, [200, 0])
    void core.pointerMove(ptr(vp, [150, 10]))
    await core.idle()
    await type(core, '40,30')
    core.setTool('circle')
    await click(core, vp, [0, -100])
    await type(core, '25')
    core.setTool('arc')
    await click(core, vp, [-100, 0])
    await click(core, vp, [0, 100])
    await click(core, vp, [100, 0])
    const gs = (await entities(engine)).map((e) => e.geometry)
    expect(gs[0]).toEqual({ type: 'rect', origin: [0, 0], width: 100, height: 50 })
    expect(gs[1]).toEqual({ type: 'rect', origin: [160, 0], width: 40, height: 30 })
    expect(gs[2]).toEqual({ type: 'circle', center: [0, -100], radius: 25 })
    const arc = gs[3] as { center: Point; radius: number; sweep: number }
    expect(arc.radius).toBeCloseTo(100, 9)
    expect(arc.center[0]).toBeCloseTo(0, 9)
    expect(arc.sweep).toBeCloseTo(-Math.PI, 9)
  })

  it('snaps to existing endpoints within the screen radius', async () => {
    const { engine, core, vp } = await setup()
    await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [103, 7] } } }])
    core.setTool('line')
    await click(core, vp, [96, 4]) // 7 px away from the endpoint at 1 px/mm
    expect(core.pointer.getSnapshot().snap?.kind).toBe('endpoint')
    await click(core, vp, [200, 0])
    const g = (await entities(engine))[1]?.geometry as { a: Point }
    expect(g.a).toEqual([103, 7])
    // Snap marker is shown in the overlay.
    expect(vp.overlay.markers?.some((m) => m.kind === 'endpoint' || m.kind === 'grid')).toBe(true)
    // Alt disables snapping.
    core.setTool('line')
    await click(core, vp, [96, 4], { alt: true })
    expect(core.pointer.getSnapshot().snap).toBeNull()
  })

  it('text tool captures typing (tool shortcuts do not fire) and dimension is associative', async () => {
    const { engine, core, vp } = await setup()
    core.setTool('text')
    await click(core, vp, [0, 0])
    for (const ch of 'İl ölçü') core.keyDown(key(ch))
    await press(core, 'Enter')
    expect(core.state.getSnapshot().tool).toBe('text')
    const t = (await entities(engine))[0]?.geometry
    expect(t).toMatchObject({ type: 'text', content: 'İl ölçü', position: [0, 0] })

    const r = await engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 100], b: [100, 100] } } },
    ])
    const lineId = r.created[0] as number
    core.setTool('dimension')
    await click(core, vp, [0, 100])
    await click(core, vp, [100, 100])
    await click(core, vp, [50, 130])
    const dims = (await entities(engine)).filter((e) => e.type === 'dotloom.dimension')
    expect(dims).toHaveLength(1)
    expect(dims[0]?.props?.a).toEqual({ entity: lineId, anchor: 'start' })
    const info = await engine.entityInfo(dims[0]?.id as number)
    expect(info.measured).toBeCloseTo(100, 9)
    // Moving the line end updates the measurement.
    await engine.apply([{ op: 'setParams', values: [{ entity: lineId, param: 'b.x', value: 150 }] }])
    expect((await engine.entityInfo(dims[0]?.id as number)).measured).toBeCloseTo(150, 9)
  })
})

describe('selection and manipulation', () => {
  async function twoLines(engine: DotloomEngine): Promise<[number, number]> {
    const r = await engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [100, 0] } } },
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 50], b: [100, 50] } } },
    ])
    return r.created as [number, number]
  }

  it('click, shift-click, window and crossing selection, Escape clears', async () => {
    const { engine, core, vp } = await setup()
    const [a, b] = await twoLines(engine)
    await click(core, vp, [50, 0])
    expect(core.state.getSnapshot().selection).toEqual([a])
    await click(core, vp, [50, 50], { shift: true })
    expect(core.state.getSnapshot().selection).toEqual([a, b])
    await click(core, vp, [50, 50], { shift: true })
    expect(core.state.getSnapshot().selection).toEqual([a])
    await click(core, vp, [300, 300])
    expect(core.state.getSnapshot().selection).toEqual([])
    // Window (left→right) only takes entities fully inside.
    await drag(core, vp, [-10, -10], [110, 20])
    expect(core.state.getSnapshot().selection).toEqual([a])
    // Crossing (right→left) takes touching entities.
    await drag(core, vp, [50, 60], [40, -10])
    expect(new Set(core.state.getSnapshot().selection)).toEqual(new Set([a, b]))
    await press(core, 'Escape')
    expect(core.state.getSnapshot().selection).toEqual([])
  })

  it('drag-move commits one undoable step; capture loss cancels', async () => {
    const { engine, core, vp } = await setup()
    const [a] = await twoLines(engine)
    await click(core, vp, [50, 0])
    await drag(core, vp, [50, 0], [50, -30])
    const moved = (await engine.entityInfo(a)).entity.geometry
    expect(moved).toEqual({ type: 'line', a: [0, -30], b: [100, -30] })
    await press(core, 'z', { mod: true })
    expect((await engine.entityInfo(a)).entity.geometry).toEqual({ type: 'line', a: [0, 0], b: [100, 0] })
    // Start a drag and lose pointer capture: nothing is committed.
    const rev = engine.revision
    void core.pointerDown(ptr(vp, [50, 0]))
    void core.pointerMove(ptr(vp, [50, 40]))
    await core.idle()
    await core.cancel('capture-lost')
    expect(engine.revision).toBe(rev)
    expect((await engine.entityInfo(a)).entity.geometry).toEqual({ type: 'line', a: [0, 0], b: [100, 0] })
  })

  it('grip drag moves one anchor', async () => {
    const { engine, core, vp } = await setup()
    const [a] = await twoLines(engine)
    await click(core, vp, [50, 0])
    expect(vp.overlay.markers?.filter((m) => m.kind === 'handle').length).toBeGreaterThanOrEqual(2)
    await drag(core, vp, [100, 0], [120, 20])
    expect((await engine.entityInfo(a)).entity.geometry).toEqual({ type: 'line', a: [0, 0], b: [120, 20] })
  })

  it('delete, undo/redo, copy/paste, nudge and group shortcuts', async () => {
    const { engine, core, vp } = await setup()
    const [a, b] = await twoLines(engine)
    await engine.setSelection([a, b])
    await core.idle()
    await press(core, 'c', { mod: true })
    await press(core, 'v', { mod: true })
    expect(await entities(engine)).toHaveLength(4)
    await press(core, 'g', { mod: true })
    expect((await engine.documentJson()).groups).toHaveLength(1)
    await press(core, 'Delete')
    expect(await entities(engine)).toHaveLength(2)
    await press(core, 'z', { mod: true })
    expect(await entities(engine)).toHaveLength(4)
    await press(core, 'z', { mod: true, shift: true })
    expect(await entities(engine)).toHaveLength(2)
    await engine.setSelection([a])
    await core.idle()
    await press(core, 'ArrowRight')
    expect((await engine.entityInfo(a)).entity.geometry).toEqual({ type: 'line', a: [10, 0], b: [110, 0] })
    // Tool shortcuts.
    await press(core, 'r')
    expect(core.state.getSnapshot().tool).toBe('rect')
    void vp
  })

  it('move, rotate and scale tools with typed values', async () => {
    const { engine, core, vp } = await setup()
    const [a] = await twoLines(engine)
    await engine.setSelection([a])
    await core.idle()
    core.setTool('move')
    await click(core, vp, [0, 0])
    await type(core, '10,20')
    expect((await engine.entityInfo(a)).entity.geometry).toEqual({ type: 'line', a: [10, 20], b: [110, 20] })
    core.setTool('rotate')
    await click(core, vp, [10, 20])
    await type(core, '90')
    const g = (await engine.entityInfo(a)).entity.geometry as { a: Point; b: Point }
    expect(g.b[0]).toBeCloseTo(10, 9)
    expect(g.b[1]).toBeCloseTo(120, 9)
    core.setTool('scale')
    await click(core, vp, [10, 20])
    await type(core, '0.5')
    const s = (await engine.entityInfo(a)).entity.geometry as { b: Point }
    expect(s.b[1]).toBeCloseTo(70, 9)
  })

  it('split and trim tools', async () => {
    const { engine, core, vp } = await setup()
    await engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [100, 0] } } },
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [50, -50], b: [50, 50] } } },
    ])
    core.setTool('trim')
    await click(core, vp, [80, 0])
    const es = await entities(engine)
    const horiz = es.find((e) => (e.geometry as { a: Point }).a[1] === 0)?.geometry
    expect(horiz).toEqual({ type: 'line', a: [0, 0], b: [50, 0] })
    core.setTool('split')
    await click(core, vp, [50, 20])
    expect(await entities(engine)).toHaveLength(3)
  })
})

describe('configuration (DL-INPUT-5)', () => {
  async function configured(options: ConstructorParameters<typeof EditorCore>[2]) {
    const engine = await createNodeEngine()
    const vp = new FakeViewport()
    const core = new EditorCore(engine, vp, options)
    for (const t of builtinTools()) core.registerTool(t)
    core.start()
    await core.idle()
    cleanup.push(() => {
      core.dispose()
      engine.dispose()
    })
    const r = await engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [103, 7] } } },
    ])
    return { engine, core, vp, line: r.created[0] as number }
  }

  it('remapped shortcuts replace the defaults', async () => {
    const { engine, core, line } = await configured({ shortcuts: { delete: ['q'], undo: ['Mod+u'] } })
    await engine.setSelection([line])
    await core.idle()
    await press(core, 'Delete')
    expect(await entities(engine)).toHaveLength(1)
    await press(core, 'q')
    expect(await entities(engine)).toHaveLength(0)
    await press(core, 'z', { mod: true })
    expect(await entities(engine)).toHaveLength(0)
    await press(core, 'u', { mod: true })
    expect(await entities(engine)).toHaveLength(1)
  })

  it('snap kinds can be switched off and the snap radius is configurable', async () => {
    // Endpoint snapping off: 2 px from the endpoint gives another kind.
    const a = await configured({ snap: { endpoint: false } })
    a.core.setTool('line')
    void a.core.pointerMove(ptr(a.vp, [101.5, 5.5], { buttons: 0 }))
    await a.core.idle()
    expect(a.core.pointer.getSnapshot().snap?.kind).not.toBe('endpoint')
    // A 3 px radius: 7 px from the endpoint no longer snaps to it (the default 10 px does).
    const b = await configured({ snapRadiusPx: 3 })
    b.core.setTool('line')
    void b.core.pointerMove(ptr(b.vp, [96, 4], { buttons: 0 }))
    await b.core.idle()
    expect(b.core.pointer.getSnapshot().snap?.kind).not.toBe('endpoint')
    const c = await configured({})
    c.core.setTool('line')
    void c.core.pointerMove(ptr(c.vp, [96, 4], { buttons: 0 }))
    await c.core.idle()
    expect(c.core.pointer.getSnapshot().snap?.kind).toBe('endpoint')
  })

  it('grid snapping follows the configured grid spacing', async () => {
    const { core, vp } = await configured({})
    core.setTool('line')
    for (const [spacing, at, expected] of [
      [40, [41, 38], [40, 40]],
      [25, [51, 49], [50, 50]],
      [10, [51, 49], [50, 50]],
      [10, [-33, 72], [-30, 70]],
    ] as [number, Point, Point][]) {
      vp.setGrid({ spacing })
      void core.pointerMove(ptr(vp, at, { buttons: 0 }))
      await core.idle()
      const snap = core.pointer.getSnapshot().snap
      expect(snap?.kind, `spacing ${spacing} at ${at}`).toBe('grid')
      expect(snap?.point).toEqual(expected)
    }
  })
})

describe('input pipeline', () => {
  it('coalesces pointer moves (latest wins)', async () => {
    const { core, vp } = await setup()
    const seen: Point[] = []
    const probe: Tool = {
      id: 'probe',
      label: 'probe',
      snaps: false,
      pointerMove: (_ctx, p) => {
        seen.push(p.world)
      },
    }
    core.registerTool(probe)
    core.setTool('probe')
    await core.idle()
    for (let i = 0; i <= 50; i++) void core.pointerMove(ptr(vp, [i, 0]))
    await core.idle()
    expect(seen.length).toBeLessThan(10)
    expect(seen[seen.length - 1]).toEqual([50, 0])
  })

  it('tool registry rejects duplicates; unregistering the active tool falls back to select', async () => {
    const { core } = await setup()
    const t: Tool = { id: 'custom', label: 'custom' }
    const un = core.registerTool(t)
    expect(() => core.registerTool({ id: 'custom', label: 'x' })).toThrow(/already registered/)
    core.setTool('custom')
    await core.idle()
    un()
    await core.idle()
    expect(core.state.getSnapshot().tool).toBe('select')
  })

  it('reports solve failures in state without throwing', async () => {
    const { core } = await setup()
    const r = await core.apply({ commands: [{ op: 'delete', ids: [999] }] })
    expect(r).toBeNull()
    expect(core.state.getSnapshot().error?.code).toBeTruthy()
  })

  it('form fields are not editor targets', () => {
    expect(isEditableTarget({ tagName: 'INPUT', type: 'text' } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: 'INPUT', type: 'checkbox' } as unknown as EventTarget)).toBe(false)
    expect(isEditableTarget({ tagName: 'TEXTAREA' } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: 'DIV', isContentEditable: true } as unknown as EventTarget)).toBe(true)
    expect(isEditableTarget({ tagName: 'CANVAS', isContentEditable: false } as unknown as EventTarget)).toBe(false)
  })
})
