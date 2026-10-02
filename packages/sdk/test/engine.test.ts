/**
 * SDK tests against the real WASM engine (DL-SDK, DL-CMD-9/11, DL-SOLVE-11).
 */

import { readFileSync } from 'node:fs'
import { afterEach, describe, expect, it } from 'vitest'
import {
  type DotloomEngine,
  DotloomError,
  decodeSceneDelta,
  EngineHost,
  InlineTransport,
  SceneStore,
} from '../src/index.js'
import { createNodeEngine } from '../src/node.js'
import type { ClientMessage, HostMessage } from '../src/protocol.js'
import type { Command, EntityTypeDef } from '../src/types.js'
import { initSync, WasmEngine } from '../src/wasm/engine/dotloom_wasm.js'

const shelf = JSON.parse(
  readFileSync(new URL('../../../tests/fixtures/plugins/shelf.json', import.meta.url), 'utf8'),
) as EntityTypeDef

const engines: DotloomEngine[] = []
async function make(stepBudget?: number): Promise<DotloomEngine> {
  const e = await createNodeEngine(stepBudget === undefined ? {} : { stepBudget })
  engines.push(e)
  return e
}
afterEach(() => {
  for (const e of engines.splice(0)) e.dispose()
})

const line = (a: [number, number], b: [number, number]): Command => ({
  op: 'createEntity',
  entity: { geometry: { type: 'line', a, b } },
})

describe('engine lifecycle', () => {
  it('initializes with matching protocol and capabilities', async () => {
    const e = await make()
    expect(e.capabilities?.protocol).toBe(1)
    expect(e.capabilities?.import).toContain('dxf')
  })

  it('rejects calls after dispose and is idempotent', async () => {
    const e = await make()
    e.dispose()
    e.dispose()
    await expect(e.undo()).rejects.toMatchObject({ code: 'disposed' })
  })

  it('pending calls reject when disposed during init/work', async () => {
    const e = await make()
    const p = e.apply([line([0, 0], [1, 0])])
    e.dispose()
    await expect(p).rejects.toBeInstanceOf(DotloomError)
  })
})

describe('transactions, events and scene', () => {
  it('commits, emits events after the commit and streams scene deltas', async () => {
    const e = await make()
    const store = new SceneStore()
    const events: string[] = []
    e.on('scene', (s) => store.apply(decodeSceneDelta(s.delta)))
    e.on('committed', (ev) => events.push(`committed:${ev.revision}`))
    e.on('historyChanged', (ev) => events.push(`history:${ev.canUndo}`))
    const r = await e.apply({ label: 'Draw', commands: [line([0, 0], [100, 0]), line([0, 10], [100, 10])] })
    expect(r.created).toHaveLength(2)
    // Scene and events arrive before the result resolves.
    expect(store.items.size).toBe(2)
    expect(events).toContain(`committed:${r.revision}`)
    expect(events).toContain('history:true')
    expect(e.revision).toBe(r.revision)
    await e.undo()
    expect(store.items.size).toBe(0)
    await e.redo()
    expect(store.items.size).toBe(2)
  })

  it('reentrant calls from listeners are queued, not nested', async () => {
    const e = await make()
    const order: string[] = []
    e.on('committed', (ev) => {
      if (ev.label === 'first') {
        order.push('listener')
        void e.apply({ label: 'second', commands: [line([0, 5], [1, 5])] }).then(() => order.push('second done'))
      }
    })
    await e.apply({ label: 'first', commands: [line([0, 0], [1, 0])] })
    order.push('first done')
    await e.historyState()
    await new Promise((r) => setTimeout(r, 0))
    await e.historyState()
    expect(order).toEqual(['listener', 'first done', 'second done'])
  })

  it('stale expected revisions are rejected', async () => {
    const e = await make()
    await e.apply([line([0, 0], [1, 0])])
    await expect(e.apply([line([0, 1], [1, 1])], { expectedRevision: e.revision - 1 })).rejects.toMatchObject({
      code: 'stale',
    })
  })

  it('maps solve failures to typed errors with diagnostics', async () => {
    const e = await make()
    await e.registerTypes(shelf, 'shelf')
    const r = await e.apply([{ op: 'createEntity', entity: { type: 'shelf.unit' } }])
    const id = r.created[0] as number
    await e.apply([
      { op: 'addConstraint', constraint: { rule: { kind: 'fix', param: { entity: id, prop: 'w1' }, value: 600 } } },
    ])
    await e.apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1600 }] }])
    const info = await e.entityInfo(id)
    expect(info.params.w2).toBeCloseTo(500, 6)
    const err = await e
      .apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1300 }] }])
      .catch((x) => x)
    expect(err).toBeInstanceOf(DotloomError)
    expect(err.code).toBe('solve')
    expect(err.details.failure.status).toEqual({ status: 'conflicting' })
    expect(err.details.failure.nearest[0].feasible).toBeCloseTo(1400, 6)
  })

  it('round-trips .dotl through save/load', async () => {
    const e = await make()
    await e.apply([line([0, 0], [100, 0])])
    const bytes = await e.save({ camera: { zoom: 2 } })
    const e2 = await make()
    const report = await e2.load(bytes)
    expect(report.view).toEqual({ camera: { zoom: 2 } })
    const d1 = await e.documentJson()
    const d2 = await e2.documentJson()
    expect(d2.entities).toEqual(d1.entities)
  })

  it('reports unknown methods and corrupt files without crashing', async () => {
    const e = await make()
    await expect(e.load(new Uint8Array([1, 2, 3]))).rejects.toMatchObject({ code: 'file' })
    await expect(e.apply({ commands: [{ op: 'nope' } as unknown as Command] })).rejects.toMatchObject({
      code: 'protocol',
    })
    expect(e.isCrashed).toBe(false)
  })
})

describe('drag protocol', () => {
  it('coalesces queued drag updates (latest wins) and commits once', async () => {
    const e = await make()
    const r = await e.apply([line([0, 0], [100, 0])])
    const id = r.created[0] as number
    await e.beginDrag({ kind: 'anchor', entity: id, anchor: 'end' })
    const previews: boolean[] = []
    e.on('scene', (s) => previews.push(s.preview))
    const results = await Promise.all([e.dragTo([100, 10]), e.dragTo([100, 20]), e.dragTo([100, 30])])
    // The first update may already be running; queued ones are superseded.
    expect(results.filter((x) => x === null).length).toBeGreaterThanOrEqual(1)
    expect(results[2]?.accepted).toBe(true)
    expect(previews.every((p) => p)).toBe(true)
    const commit = await e.endDrag(true)
    expect(commit?.revision).toBe(r.revision + 1)
    const info = await e.entityInfo(id)
    expect(info.anchors.find((a) => a.name === 'end')?.point).toEqual([100, 30])
  })
})

describe('real cancellation', () => {
  it('cancels a long solve between steps and keeps the document unchanged', async () => {
    // A long chain dragged far away needs many solver iterations.
    const e = await make(1)
    const ids = await e.reserveIds(60)
    const commands: Command[] = []
    for (let i = 0; i < 60; i++) {
      commands.push({
        op: 'createEntity',
        id: ids[i] as number,
        entity: { geometry: { type: 'line', a: [i * 10, 0], b: [i * 10 + 10, 0] } },
      })
    }
    for (let i = 0; i < 59; i++) {
      commands.push({
        op: 'addConstraint',
        constraint: {
          rule: {
            kind: 'coincident',
            a: { entity: ids[i] as number, anchor: 'end' },
            b: { entity: ids[i + 1] as number, anchor: 'start' },
          },
        },
      })
      commands.push({
        op: 'addConstraint',
        constraint: {
          rule: {
            kind: 'length',
            line: {
              from: { entity: ids[i] as number, anchor: 'start' },
              to: { entity: ids[i] as number, anchor: 'end' },
            },
            value: 10,
          },
        },
      })
    }
    await e.apply(commands)
    const before = await e.documentJson()
    const ctrl = new AbortController()
    const p = e.apply(
      [{ op: 'setParams', values: [{ entity: ids[30] as number, param: 'a.y', value: 250 }], mode: 'prefer' }],
      { signal: ctrl.signal },
    )
    // Abort only after the solver reported progress: this is a cancel *during* the solve.
    let seen = 0
    e.on('progress', (ev) => {
      seen = ev.iterations
      ctrl.abort()
    })
    await expect(p).rejects.toMatchObject({ code: 'cancelled' })
    expect(seen).toBeGreaterThan(0)
    const after = await e.documentJson()
    expect(after.entities).toEqual(before.entities)
    // The engine stays usable.
    await e.apply([line([0, 50], [10, 50])])
  })
})

describe('protocol robustness', () => {
  function rawHost(): { send: (m: ClientMessage) => void; out: HostMessage[] } {
    const out: HostMessage[] = []
    const host = new EngineHost(
      (m) => out.push(m),
      async () => {
        initSync({ module: readFileSync(new URL('../src/wasm/engine/dotloom_wasm_bg.wasm', import.meta.url)) })
        return WasmEngine
      },
    )
    return { send: (m) => host.receive(m), out }
  }

  it('rejects wrong protocol versions and calls before init', async () => {
    const h = rawHost()
    h.send({ v: 2, kind: 'call', id: 1, method: 'undo', args: [] } as unknown as ClientMessage)
    h.send({ v: 1, kind: 'call', id: 2, method: 'undo', args: [] })
    await new Promise((r) => setTimeout(r, 10))
    const errs = h.out.filter((m) => m.kind === 'result' && !m.ok)
    expect(errs.map((m) => (m.kind === 'result' && !m.ok ? m.error.code : ''))).toEqual(['protocol', 'init'])
  })

  it('in-thread transport delivers asynchronously like a worker', async () => {
    let delivered = false
    const t = new InlineTransport(async () => {
      initSync({ module: readFileSync(new URL('../src/wasm/engine/dotloom_wasm_bg.wasm', import.meta.url)) })
      return WasmEngine
    })
    t.onMessage(() => {
      delivered = true
    })
    t.send({ v: 1, kind: 'init', id: 1 })
    expect(delivered).toBe(false)
    await new Promise((r) => setTimeout(r, 20))
    expect(delivered).toBe(true)
    t.terminate()
  })
})
