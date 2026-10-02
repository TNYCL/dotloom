/**
 * Plugin lifecycle, storage/autosave and version checks (DL-PLUGIN-1/2/4, DL-FILE-7/9).
 */

import { readFileSync } from 'node:fs'
import { afterEach, describe, expect, it } from 'vitest'
import { screenToWorld, worldToScreen } from '../src/camera.js'
import { EditorCore } from '../src/editor/core.js'
import { builtinTools } from '../src/editor/tools/index.js'
import type { InputPointer, Tool } from '../src/editor/types.js'
import type { DotloomEngine } from '../src/engine.js'
import { createNodeEngine } from '../src/node.js'
import { type DotloomPlugin, PluginHost } from '../src/plugins.js'
import { Autosave, MemoryStorage } from '../src/storage.js'
import type { EntityTypeDef, Point } from '../src/types.js'
import { SDK_VERSION, satisfies } from '../src/version.js'
import { DEFAULT_GRID, type ViewportLike } from '../src/viewport.js'

const shelf = JSON.parse(
  readFileSync(new URL('../../../tests/fixtures/plugins/shelf.json', import.meta.url), 'utf8'),
) as EntityTypeDef

const vp: ViewportLike = {
  size: { width: 800, height: 600, dpr: 1 },
  camera: { center: [0, 0], scale: 1 },
  grid: { ...DEFAULT_GRID },
  setCamera() {},
  screenToWorld(x, y) {
    return screenToWorld(this.camera, this.size, x, y)
  },
  worldToScreen(p) {
    return worldToScreen(this.camera, this.size, p)
  },
  setOverlay() {},
  setHover() {},
  setGrid() {},
  async fit() {},
  requestRender() {},
}

const cleanup: (() => void)[] = []
afterEach(() => {
  for (const f of cleanup.splice(0)) f()
})

async function setup(): Promise<{ engine: DotloomEngine; core: EditorCore; host: PluginHost }> {
  const engine = await createNodeEngine()
  const core = new EditorCore(engine, vp)
  for (const t of builtinTools()) core.registerTool(t)
  core.start()
  const host = new PluginHost(engine, core)
  cleanup.push(() => {
    core.dispose()
    engine.dispose()
  })
  return { engine, core, host }
}

function shelfPlugin(log: string[]): DotloomPlugin {
  const tool: () => Tool = () => ({
    id: 'shelf.place',
    label: 'Place shelf',
    pointerDown: async (ctx, p) => {
      await ctx.apply({
        commands: [
          { op: 'createEntity', entity: { type: 'shelf.unit', transform: [1, 0, 0, 1, p.point[0], p.point[1]] } },
        ],
      })
    },
  })
  return {
    id: 'shelf.configurator',
    version: '1.0.0',
    sdk: '^0.1.0',
    types: [shelf],
    tools: [tool],
    commands: {
      add: {
        label: 'Add shelf',
        run: (_api, args) => ({
          label: 'Add shelf',
          commands: [{ op: 'createEntity', entity: { type: 'shelf.unit', props: { width: Number(args) } } }],
        }),
      },
    },
    snapProviders: [
      {
        id: 'shelf.origin',
        snap: (p) => (Math.hypot(p[0] - 503.5, p[1] - 497.25) < 50 ? [{ point: [503.5, 497.25] }] : []),
      },
    ],
    importers: [
      {
        id: 'csv-points',
        extensions: ['.csv'],
        import: (bytes) => ({
          label: 'Import CSV',
          commands: new TextDecoder()
            .decode(bytes)
            .trim()
            .split('\n')
            .map((l) => l.split(',').map(Number) as Point)
            .map((at) => ({ op: 'createEntity' as const, entity: { geometry: { type: 'point' as const, at } } })),
        }),
      },
    ],
    exporters: [
      {
        id: 'count',
        extension: '.txt',
        mime: 'text/plain',
        export: async (api) => new TextEncoder().encode(String((await api.engine.documentJson()).entities.length)),
      },
    ],
    activate(api) {
      log.push('activate')
      api.on('committed', () => log.push('commit'))
      return () => log.push('cleanup')
    },
  }
}

function ptr(w: Point): InputPointer {
  const [x, y] = vp.worldToScreen(w)
  return { x, y, button: 0, buttons: 1, shift: false, mod: false, alt: false, pointerId: 1, pointerType: 'mouse' }
}

describe('plugin lifecycle', () => {
  it('registers types, tools, commands and snap providers; disable/enable/unregister', async () => {
    const { engine, core, host } = await setup()
    const log: string[] = []
    await host.register(shelfPlugin(log))
    expect(host.list()).toEqual([
      expect.objectContaining({ id: 'shelf.configurator', enabled: true, types: ['shelf.unit'], commands: ['add'] }),
    ])
    expect(core.listTools().some((t) => t.id === 'shelf.place')).toBe(true)
    // Plugin tool with plugin snap provider.
    core.setTool('shelf.place')
    // The grid offers (510, 500) at ~5.4 mm; the provider's point is closer.
    void core.pointerDown(ptr([505, 498]))
    await core.idle()
    const created = (await engine.documentJson()).entities.find((e) => e.type === 'shelf.unit')
    expect(created?.transform?.slice(4)).toEqual([503.5, 497.25])
    // Named command.
    await host.run('shelf.configurator:add', 1200)
    expect(log.filter((x) => x === 'commit').length).toBe(2)
    const id = created?.id as number

    await host.disable('shelf.configurator')
    expect(log).toContain('cleanup')
    expect(core.listTools().some((t) => t.id === 'shelf.place')).toBe(false)
    expect(core.state.getSnapshot().tool).toBe('select')
    const ro = await engine.entityInfo(id)
    expect(ro.readOnly?.reason).toBe('disabledPlugin')
    // Disabled: edits of its entities are rejected, commands unavailable, listeners removed.
    await expect(
      engine.apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 900 }] }]),
    ).rejects.toBeTruthy()
    await expect(host.run('shelf.configurator:add', 1)).rejects.toMatchObject({ code: 'plugin' })
    const commits = log.filter((x) => x === 'commit').length
    await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [0, 0] } } }])
    expect(log.filter((x) => x === 'commit').length).toBe(commits)

    await host.enable('shelf.configurator')
    expect((await engine.entityInfo(id)).readOnly).toBeNull()
    await engine.apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 900 }] }])

    await host.unregister('shelf.configurator')
    expect(host.list()).toEqual([])
    expect((await engine.entityInfo(id)).readOnly?.reason).toBe('missingPlugin')
    // Data survives a save/load without the plugin.
    const bytes = await engine.save()
    const e2 = await createNodeEngine()
    cleanup.push(() => e2.dispose())
    const report = await e2.load(bytes)
    expect(report.missingPlugins).toContain('shelf.unit')
    expect((await e2.documentJson()).entities.find((e) => e.id === id)?.props?.width).toBe(900)
  })

  it('lists tools, runs onDispose and listener cleanups, mounts panels once, dispose() releases everything', async () => {
    const { engine, core, host } = await setup()
    const log: string[] = []
    let commits = 0
    const store = new MemoryStorage()
    const plugin: DotloomPlugin = {
      ...shelfPlugin(log),
      storage: [store],
      panels: [
        {
          id: 'info',
          title: 'Shelf',
          forTypes: ['shelf.unit'],
          mount(el, api) {
            el.textContent = `selected ${api.selection().length}`
            log.push('mount')
            return () => {
              el.textContent = ''
              log.push('unmount')
            }
          },
        },
      ],
      activate(api) {
        api.on('committed', () => {
          commits++
        })
        // What a plugin with its own GPU buffers, DOM nodes or timers registers.
        api.onDispose(() => log.push('release'))
        return () => log.push('cleanup')
      },
    }
    await host.register(plugin)
    expect(host.list()[0]).toMatchObject({ tools: ['shelf.place'], commands: ['add'], enabled: true })
    expect(host.storageAdapters()).toEqual([store])

    // Panels get the live selection; unmounting twice is harmless.
    const el = { textContent: '' } as unknown as HTMLElement
    const off = host.mountPanel('shelf.configurator', 'info', el)
    expect(el.textContent).toBe('selected 0')
    off()
    off()
    expect(log.filter((x) => x === 'unmount')).toHaveLength(1)

    // Disabling unmounts open panels and runs every cleanup exactly once.
    host.mountPanel('shelf.configurator', 'info', el)
    await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [0, 0] } } }])
    expect(commits).toBe(1)
    await host.disable('shelf.configurator')
    expect(log.filter((x) => x === 'unmount')).toHaveLength(2)
    expect(log.filter((x) => x === 'release')).toHaveLength(1)
    expect(log.filter((x) => x === 'cleanup')).toHaveLength(1)
    expect(host.list()[0]).toMatchObject({ tools: [], enabled: false })
    expect(host.storageAdapters()).toEqual([])
    expect(() => host.mountPanel('shelf.configurator', 'info', el)).toThrow(/disabled/)
    await engine.apply([{ op: 'createEntity', entity: { geometry: { type: 'point', at: [1, 1] } } }])
    expect(commits).toBe(1)

    // dispose(): every plugin unregistered, cleanups run, registry subscribers dropped.
    await host.enable('shelf.configurator')
    host.mountPanel('shelf.configurator', 'info', el)
    let changes = 0
    host.subscribe(() => {
      changes++
    })
    await host.dispose()
    expect(host.list()).toEqual([])
    expect(log.filter((x) => x === 'release')).toHaveLength(2)
    expect(log.filter((x) => x === 'unmount')).toHaveLength(3)
    expect(core.listTools().some((t) => t.id === 'shelf.place')).toBe(false)
    expect((await engine.pluginTypes()).map((t) => t.typeId)).not.toContain('shelf.unit')
    const seen = changes
    await host.register({ id: 'acme.after', version: '1.0.0' })
    expect(changes).toBe(seen)
  })

  it('rejects invalid ids, duplicates, incompatible SDK ranges, foreign namespaces and type conflicts', async () => {
    const { host, core } = await setup()
    await expect(host.register({ id: 'Bad Id', version: '1.0.0' })).rejects.toMatchObject({ code: 'plugin' })
    await host.register({ id: 'acme.one', version: '1.0.0' })
    await expect(host.register({ id: 'acme.one', version: '1.0.1' })).rejects.toThrow(/already registered/)
    await expect(host.register({ id: 'acme.two', version: '1.0.0', sdk: '^2.0.0' })).rejects.toThrow(
      /requires @dotloom\/sdk/,
    )
    await expect(host.register({ id: 'acme.three', version: '1.0.0', types: [shelf] })).rejects.toThrow(/namespace/)
    await host.register({ id: 'shelf.a', version: '1.0.0', types: [shelf] })
    await expect(host.register({ id: 'shelf.b', version: '1.0.0', types: [shelf] })).rejects.toThrow(
      /already registered/,
    )
    expect(host.list().map((p) => p.id)).toEqual(['acme.one', 'shelf.a'])
    // A tool id clash rolls the whole plugin back.
    const clash: DotloomPlugin = { id: 'acme.clash', version: '1.0.0', tools: [() => ({ id: 'line', label: 'x' })] }
    await expect(host.register(clash)).rejects.toThrow(/already registered/)
    expect(host.list().some((p) => p.id === 'acme.clash')).toBe(false)
    expect(core.listTools().filter((t) => t.id === 'line')).toHaveLength(1)
  })

  it('importers and exporters run as transactions / outputs', async () => {
    const { engine, host } = await setup()
    await host.register(shelfPlugin([]))
    await host.importFile('points.CSV', new TextEncoder().encode('0,0\n10,5\n20,10\n'))
    expect((await engine.documentJson()).entities).toHaveLength(3)
    await engine.undo()
    expect((await engine.documentJson()).entities).toHaveLength(0)
    await engine.redo()
    const out = await host.exportWith('shelf.configurator', 'count')
    expect(new TextDecoder().decode(out)).toBe('3')
    await expect(host.importFile('x.unknown', new Uint8Array())).rejects.toMatchObject({ code: 'import' })
  })
})

describe('storage and autosave', () => {
  it('autosaves after commits and recovers in a new session', async () => {
    const storage = new MemoryStorage()
    const e1 = await createNodeEngine()
    cleanup.push(() => e1.dispose())
    const a1 = new Autosave(e1, storage, { debounceMs: 10, name: 'raf-tasarimi.dotl', view: () => ({ zoom: 3 }) })
    a1.start()
    await e1.apply([{ op: 'createEntity', entity: { geometry: { type: 'circle', center: [0, 0], radius: 5 } } }])
    await new Promise((r) => setTimeout(r, 40))
    await a1.flush()
    expect(a1.state.getSnapshot()).toMatchObject({ dirty: true, saving: false, error: null })
    const meta = await storage.meta('autosave')
    expect(meta).toMatchObject({ name: 'raf-tasarimi.dotl', clean: false })

    // "Crash": a new engine finds the recoverable autosave.
    const e2 = await createNodeEngine()
    cleanup.push(() => e2.dispose())
    const a2 = new Autosave(e2, storage)
    expect(await a2.recoverable()).toMatchObject({ name: 'raf-tasarimi.dotl' })
    const view = await a2.restore()
    expect(view).toEqual({ zoom: 3 })
    expect((await e2.documentJson()).entities).toHaveLength(1)
    // After the user saves, nothing is offered for recovery.
    await a2.markSaved()
    expect(await a2.recoverable()).toBeNull()
    await a2.discard()
    expect(await storage.list()).toEqual([])
    a1.dispose()
    a2.dispose()
  })

  it('version ranges', () => {
    const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version: string }
    expect(SDK_VERSION).toBe(pkg.version)
    expect(satisfies('0.1.4', '^0.1.0')).toBe(true)
    expect(satisfies('0.2.0', '^0.1.0')).toBe(false)
    expect(satisfies('1.4.0', '^1.2.0')).toBe(true)
    expect(satisfies('2.0.0', '^1.2.0')).toBe(false)
    expect(satisfies('1.2.9', '~1.2.0')).toBe(true)
    expect(satisfies('1.3.0', '>=1.2.0 <2.0.0')).toBe(true)
    expect(satisfies('1.0.0', '*')).toBe(true)
    expect(satisfies('x', '*')).toBe(false)
  })
})

describe('units', async () => {
  const { formatLength, parseLength, parseAngle, formatAngle } = await import('../src/units.js')
  it('formats and parses display units (incl. decimal comma)', () => {
    expect(formatLength(1600, 'centimetre')).toBe('160 cm')
    expect(formatLength(1234.6, 'metre')).toBe('1.235 m')
    expect(formatLength(25.4, 'inch')).toBe('1 in')
    expect(parseLength('160', 'centimetre')).toBe(1600)
    expect(parseLength('1,2 m', 'millimetre')).toBe(1200)
    expect(parseLength(' -5mm ', 'metre')).toBe(-5)
    expect(parseLength('2 ft', 'metre')).toBeCloseTo(609.6, 9)
    expect(parseLength('1+1', 'metre')).toBeNull()
    expect(parseLength('12 parsec', 'metre')).toBeNull()
    expect(parseAngle('90°')).toBeCloseTo(Math.PI / 2, 12)
    expect(parseAngle('0,5 rad')).toBe(0.5)
    expect(formatAngle(Math.PI)).toBe('180°')
  })
  it('formats and parses durations', async () => {
    const { formatDuration, parseDuration } = await import('../src/units.js')
    expect(formatDuration(9000)).toBe('2h 30m')
    expect(formatDuration(45)).toBe('45s')
    expect(formatDuration(0)).toBe('0s')
    expect(parseDuration('2h 30m')).toBe(9000)
    expect(parseDuration('1,5 h')).toBe(5400)
    expect(parseDuration('45m')).toBe(2700)
    expect(parseDuration('90')).toBe(90)
    expect(parseDuration('2 hours 5 minutes')).toBe(7500)
    expect(parseDuration('2 days')).toBeNull()
    expect(parseDuration('h')).toBeNull()
  })
})
