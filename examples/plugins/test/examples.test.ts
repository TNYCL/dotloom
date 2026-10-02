/**
 * Example domains on the real engine (DL-EXAMPLE-1/2/3).
 */

import { builtinTools, type DotloomEngine, EditorCore, NullViewport, PluginHost } from '@dotloom/sdk'
import { createNodeEngine } from '@dotloom/sdk/node'
import { afterEach, describe, expect, it } from 'vitest'
import {
  floorplanPlugin,
  loadFloorplanExample,
  loadShelfExample,
  loadTimelineExample,
  shelfPlugin,
  timelinePlugin,
} from '../src/index.js'

const disposers: (() => void)[] = []
afterEach(() => {
  for (const d of disposers.splice(0)) d()
})

async function setup(): Promise<{ engine: DotloomEngine; core: EditorCore; host: PluginHost }> {
  const engine = await createNodeEngine()
  const core = new EditorCore(engine, new NullViewport())
  for (const t of builtinTools()) core.registerTool(t)
  core.start()
  const host = new PluginHost(engine, core)
  for (const p of [shelfPlugin, floorplanPlugin, timelinePlugin]) await host.register(p)
  disposers.push(() => {
    core.dispose()
    engine.dispose()
  })
  return { engine, core, host }
}

const param = async (e: DotloomEngine, id: number, name: string): Promise<number> =>
  (await e.entityInfo(id)).params[name] ?? Number.NaN

describe('shelf configurator', () => {
  it('180 cm → 160 cm gives 60/50/50; 130 cm is rejected with 140 cm as the bound', async () => {
    const { engine } = await setup()
    const id = await loadShelfExample(engine)
    expect(await param(engine, id, 'width')).toBeCloseTo(1800, 6)
    await engine.apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1600 }] }])
    expect(
      [await param(engine, id, 'w1'), await param(engine, id, 'w2'), await param(engine, id, 'w3')].map((v) =>
        Math.round(v),
      ),
    ).toEqual([600, 500, 500])
    const err = await engine
      .apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1300 }] }])
      .catch((e) => e)
    expect(err.code).toBe('solve')
    expect(err.details.failure.nearest[0].feasible).toBeCloseTo(1400, 6)
    expect(await param(engine, id, 'width')).toBeCloseTo(1600, 6)
  })
})

describe('room planner', () => {
  it('moving a corner keeps walls connected; dimensions follow', async () => {
    const { engine } = await setup()
    const { walls, dimension } = await loadFloorplanExample(engine)
    const [w1, w2] = walls as [number, number]
    expect((await engine.entityInfo(dimension)).measured).toBeCloseTo(5000, 6)
    // Drag the shared corner of wall 1 / wall 2.
    await engine.beginDrag({ kind: 'anchor', entity: w1, anchor: 'end' })
    await engine.dragTo([6000, 0])
    await engine.endDrag(true)
    const end1 = (await engine.entityInfo(w1)).anchors.find((a) => a.name === 'end')?.point
    const start2 = (await engine.entityInfo(w2)).anchors.find((a) => a.name === 'start')?.point
    expect(end1).toEqual(start2)
    expect(end1?.[0]).toBeCloseTo(6000, 6)
    expect(await engine.verify()).toEqual([])
  })

  it('shortening a wall slides its door; impossible lengths are reported, not committed', async () => {
    const { engine } = await setup()
    const { walls, door } = await loadFloorplanExample(engine)
    const w1 = walls[0] as number
    // Lock the door offset: then the wall cannot be shorter than offset + width.
    await engine.apply([{ op: 'setParams', values: [{ entity: w1, param: 'end.x', value: 1500 }], mode: 'prefer' }])
    expect(await param(engine, door, 'width')).toBeCloseTo(900, 6)
    expect((await param(engine, door, 'offset')) + 900).toBeLessThanOrEqual(1500 + 1e-6)
    // Pin the wall start and the door offset: now the wall cannot get shorter than
    // offset + minimum door width (the free room would otherwise just move).
    await engine.apply([
      {
        op: 'addConstraint',
        constraint: { rule: { kind: 'fix', param: { entity: door, prop: 'offset' }, value: 600 } },
      },
      {
        op: 'addConstraint',
        constraint: { rule: { kind: 'fixPoint', a: { entity: w1, anchor: 'start' }, at: [0, 0] } },
      },
    ])
    const rev = engine.revision
    const err = await engine
      .apply([{ op: 'setParams', values: [{ entity: w1, param: 'end.x', value: 1000 }] }])
      .catch((e) => e)
    expect(err.code).toBe('solve')
    expect(err.details.failure.diagnostics[0].labels.join(' ')).toMatch(/door fits in the wall/)
    expect(engine.revision).toBe(rev)
  })

  it('deleting a wall deletes its door; save/load keeps references and rules', async () => {
    const { engine } = await setup()
    const { walls, door } = await loadFloorplanExample(engine)
    const bytes = await engine.save()
    const e2 = await createNodeEngine()
    disposers.push(() => e2.dispose())
    const h2 = new PluginHost(e2)
    await h2.register(floorplanPlugin)
    await e2.load(bytes)
    const d2 = (await e2.documentJson()).entities.find((e) => e.id === door)
    expect(d2?.props?.host).toEqual({ ref: walls[0] })
    expect((await e2.documentJson()).constraints).toHaveLength(4)
    await e2.apply([{ op: 'delete', ids: [walls[0] as number], policy: 'cascade' }])
    const left = (await e2.documentJson()).entities.map((e) => e.id)
    expect(left).not.toContain(door)
    // Constraints touching the deleted wall are gone too (no orphans).
    expect((await e2.documentJson()).constraints).toHaveLength(2)
    await e2.undo()
    expect((await e2.documentJson()).entities.map((e) => e.id)).toContain(door)
  })

  it('wall and door tools build connected walls and a hosted door', async () => {
    const { engine, core } = await setup()
    await engine.newDocument()
    const vp = core.viewport
    const click = async (x: number, y: number): Promise<void> => {
      const [sx, sy] = vp.worldToScreen([x, y])
      void core.pointerDown({
        x: sx,
        y: sy,
        button: 0,
        buttons: 1,
        shift: false,
        mod: false,
        alt: true,
        pointerId: 1,
        pointerType: 'mouse',
      })
      void core.pointerUp({
        x: sx,
        y: sy,
        button: 0,
        buttons: 0,
        shift: false,
        mod: false,
        alt: true,
        pointerId: 1,
        pointerType: 'mouse',
      })
      await core.idle()
    }
    vp.setCamera({ center: [2000, 1500], scale: 0.1 })
    core.setTool('floorplan.wall')
    await click(0, 0)
    await click(4000, 0)
    await click(4000, 3000)
    await click(0, 0)
    const doc = await engine.documentJson()
    expect(doc.entities.filter((e) => e.type === 'floorplan.wall')).toHaveLength(3)
    expect(doc.constraints).toHaveLength(3)
    core.setTool('floorplan.door')
    await click(2000, 0)
    const d = (await engine.documentJson()).entities.find((e) => e.type === 'floorplan.door')
    expect(d?.props?.offset).toBeCloseTo(1550, 6)
  })
})

describe('room planner: snapping and layers', () => {
  it('the wall tool closes the room on the first wall end by snapping', async () => {
    const { engine, core } = await setup()
    await engine.newDocument()
    const vp = core.viewport
    const pointer = (x: number, y: number, buttons: number) => {
      const [sx, sy] = vp.worldToScreen([x, y])
      return {
        x: sx,
        y: sy,
        button: 0,
        buttons,
        shift: false,
        mod: false,
        alt: false,
        pointerId: 1,
        pointerType: 'mouse' as const,
      }
    }
    const click = async (x: number, y: number): Promise<void> => {
      void core.pointerMove(pointer(x, y, 0))
      void core.pointerDown(pointer(x, y, 1))
      void core.pointerUp(pointer(x, y, 0))
      await core.idle()
    }
    vp.setCamera({ center: [2000, 1500], scale: 0.1 }) // 1 px = 10 mm
    core.setTool('floorplan.wall')
    await click(0, 0)
    await click(4000, 0)
    await click(4000, 3000)
    // 4.4 px from the first corner: the endpoint snap wins over the grid.
    void core.pointerMove(pointer(37, -24, 0))
    await core.idle()
    expect(core.pointer.getSnapshot().snap?.kind).toBe('endpoint')
    await click(37, -24)
    const doc = await engine.documentJson()
    const walls = doc.entities.filter((e) => e.type === 'floorplan.wall')
    expect(walls).toHaveLength(3)
    expect(walls[2]?.props?.end).toEqual([0, 0])
    // Each joint, including the closing one, is a coincidence rule.
    expect(doc.constraints).toHaveLength(3)
    expect(await engine.verify()).toEqual([])
  })

  it('walls and dimensions live on their layers; a locked layer protects its walls', async () => {
    const { engine } = await setup()
    const { walls, dimension } = await loadFloorplanExample(engine)
    const doc = await engine.documentJson()
    const layer = (name: string) => doc.layers.find((l) => l.name === name)?.id
    const wallsLayer = layer('Walls')
    const dimsLayer = layer('Dimensions')
    expect(wallsLayer).toBeDefined()
    expect(dimsLayer).toBeDefined()
    for (const id of walls) expect(doc.entities.find((e) => e.id === id)?.layer).toBe(wallsLayer)
    expect(doc.entities.find((e) => e.id === dimension)?.layer).toBe(dimsLayer)
    await engine.apply([{ op: 'updateLayer', id: wallsLayer as number, patch: { locked: true } }])
    const w = walls[0] as number
    const err = await engine
      .apply([{ op: 'setParams', values: [{ entity: w, param: 'thickness', value: 300 }] }])
      .catch((e) => e)
    expect(err.code).toBe('command')
    await engine.apply([{ op: 'updateLayer', id: wallsLayer as number, patch: { locked: false } }])
    await engine.apply([{ op: 'setParams', values: [{ entity: w, param: 'thickness', value: 300 }] }])
    expect(await param(engine, w, 'thickness')).toBeCloseTo(300, 9)
  })
})

describe('timeline', () => {
  it('moving a block pushes the following blocks; the locked release time holds', async () => {
    const { engine } = await setup()
    const ids = await loadTimelineExample(engine)
    const H = 3600
    expect(await engine.verify()).toEqual([])
    // Move "Design" 30 minutes later: "Build" is pushed, "Release" stays at 16:00.
    const begin = (await engine.entityInfo(ids.design)).anchors.find((a) => a.name === 'begin')?.point as [
      number,
      number,
    ]
    await engine.beginDrag({ kind: 'anchor', entity: ids.design, anchor: 'begin' })
    await engine.dragTo([begin[0] + 50, begin[1]])
    await engine.endDrag(true)
    expect(await param(engine, ids.design, 'start')).toBeCloseTo(9.5 * H, 3)
    expect(await param(engine, ids.build, 'start')).toBeGreaterThanOrEqual(11.5 * H - 1e-3)
    expect(await param(engine, ids.release, 'start')).toBeCloseTo(16 * H, 6)
    expect(await engine.verify()).toEqual([])
    // The 30-minute gap between "Build" and "Review" holds exactly where it binds.
    const buildEnd = (await param(engine, ids.build, 'start')) + (await param(engine, ids.build, 'duration'))
    expect((await param(engine, ids.review, 'start')) - buildEnd).toBeGreaterThanOrEqual(30 * 60 - 1e-3)
    // Typing a "Review" start only 10 minutes after the current end of "Build": the
    // solver makes room earlier in the plan (free starts and durations) instead of
    // breaking the gap.
    const reviewStart = buildEnd + 10 * 60
    await engine.apply([{ op: 'setParams', values: [{ entity: ids.review, param: 'start', value: reviewStart }] }])
    expect(await param(engine, ids.review, 'start')).toBeCloseTo(reviewStart, 6)
    const newBuildEnd = (await param(engine, ids.build, 'start')) + (await param(engine, ids.build, 'duration'))
    expect(reviewStart - newBuildEnd).toBeGreaterThanOrEqual(30 * 60 - 1e-3)
    expect(await engine.verify()).toEqual([])
    await engine.undo()
    // Dragging "Review" past the locked release stops at the nearest allowed time.
    const rb = (await engine.entityInfo(ids.review)).anchors.find((a) => a.name === 'begin')?.point as [number, number]
    await engine.beginDrag({ kind: 'anchor', entity: ids.review, anchor: 'begin' })
    const preview = await engine.dragTo([rb[0] + 200, rb[1]])
    expect(preview?.accepted).toBe(true)
    await engine.endDrag(true)
    const reviewEnd = (await param(engine, ids.review, 'start')) + (await param(engine, ids.review, 'duration'))
    expect(reviewEnd).toBeLessThanOrEqual(16 * H + 1e-3)
    expect(await param(engine, ids.release, 'start')).toBeCloseTo(16 * H, 6)
  })

  it('durations respect the minimum and equal-duration rules', async () => {
    const { engine } = await setup()
    const ids = await loadTimelineExample(engine)
    const err = await engine
      .apply([{ op: 'setParams', values: [{ entity: ids.review, param: 'duration', value: 60 }] }])
      .catch((e) => e)
    expect(err.code).toBe('solve')
    // Release duration equals review duration: changing review changes release (its start is locked).
    await engine.apply([{ op: 'setParams', values: [{ entity: ids.review, param: 'duration', value: 2700 }] }])
    expect(await param(engine, ids.release, 'duration')).toBeCloseTo(2700, 6)
  })
})
