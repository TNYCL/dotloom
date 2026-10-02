/**
 * React panels against the real engine (DL-UI, DL-EXAMPLE shelf flow, DL-SDK-7).
 * The viewport is the SDK's NullViewport (jsdom has no GPU).
 */

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { builtinTools, EditorCore, type EntityTypeDef, NullViewport, PluginHost } from '@dotloomjs/sdk'
import { createNodeEngine } from '@dotloomjs/sdk/node'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { CommandPalette, matchScore } from '../src/components/CommandPalette.js'
import { ConstraintsPanel } from '../src/components/ConstraintsPanel.js'
import { Inspector } from '../src/components/Inspector.js'
import { LayersPanel } from '../src/components/LayersPanel.js'
import { ObjectsPanel } from '../src/components/ObjectsPanel.js'
import { StatusBar } from '../src/components/StatusBar.js'
import { Toolbar } from '../src/components/Toolbar.js'
import { type EditorHandle, EditorProvider, useEditorState } from '../src/context.js'
import { I18nProvider, makeTranslate } from '../src/i18n.js'

// jsdom rewrites import.meta.url, so resolve fixtures from the package directory.
const shelf = JSON.parse(
  readFileSync(resolve(process.cwd(), '../../tests/fixtures/plugins/shelf.json'), 'utf8'),
) as EntityTypeDef

const disposers: (() => void)[] = []
afterEach(() => {
  cleanup()
  for (const d of disposers.splice(0)) d()
})

async function handle(): Promise<EditorHandle> {
  const engine = await createNodeEngine({
    engineWasmUrl: resolve(process.cwd(), '../sdk/src/wasm/engine/dotloom_wasm_bg.wasm'),
  })
  const viewport = new NullViewport()
  const core = new EditorCore(engine, viewport)
  for (const t of builtinTools()) core.registerTool(t)
  core.start()
  const plugins = new PluginHost(engine, core)
  disposers.push(() => {
    core.dispose()
    engine.dispose()
  })
  return { engine, core, viewport, canvas: null, plugins, autosave: null }
}

function ui(h: EditorHandle, node: React.ReactNode, locale = 'en'): ReturnType<typeof render> {
  return render(
    <I18nProvider locale={locale}>
      <EditorProvider value={h}>{node}</EditorProvider>
    </I18nProvider>,
  )
}

async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 30))
  })
}

describe('shelf configurator flow through the inspector', () => {
  async function shelfSetup(): Promise<{ h: EditorHandle; id: number }> {
    const h = await handle()
    await h.plugins.register({ id: 'shelf.example', version: '1.0.0', types: [shelf] })
    const r = await h.engine.apply([
      { op: 'setSettings', patch: { displayUnit: 'centimetre' } },
      { op: 'createEntity', entity: { type: 'shelf.unit' } },
    ])
    const id = r.created[0] as number
    await h.engine.setSelection([id])
    return { h, id }
  }

  it('160 cm gives 60/50/50; 130 cm is rejected with the 140 cm bound; unlock; undo/redo', async () => {
    const { h, id } = await shelfSetup()
    ui(
      h,
      <>
        <Inspector />
        <ConstraintsPanel />
      </>,
    )
    await settle()
    // Lock the left compartment at 60 cm from the inspector.
    fireEvent.click(await screen.findByRole('button', { name: 'Lock value: Left compartment' }))
    await settle()
    expect(screen.getByRole('button', { name: 'Unlock value: Left compartment' })).toBeTruthy()

    const width = screen.getByLabelText('Inner width') as HTMLInputElement
    expect(width.value).toBe('180')
    fireEvent.change(width, { target: { value: '160' } })
    fireEvent.keyDown(width, { key: 'Enter' })
    await settle()
    await waitFor(() => expect((screen.getByLabelText('Middle compartment') as HTMLInputElement).value).toBe('50'))
    expect((screen.getByLabelText('Left compartment') as HTMLInputElement).value).toBe('60')
    expect((screen.getByLabelText('Right compartment') as HTMLInputElement).value).toBe('50')

    // 130 cm violates the hard rules: rejected, nearest feasible value offered.
    fireEvent.change(width, { target: { value: '130 cm' } })
    fireEvent.keyDown(width, { key: 'Enter' })
    await settle()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('compartments fill the inner width')
    const fix = within(alert).getByRole('button', { name: 'Use nearest allowed value 140 cm' })
    expect((await h.engine.entityInfo(id)).params.width).toBeCloseTo(1600, 6)

    // Accept the suggestion.
    fireEvent.click(fix)
    await settle()
    expect((await h.engine.entityInfo(id)).params.width).toBeCloseTo(1400, 6)

    // Unlock (remove the rule) → 130 cm becomes possible.
    fireEvent.click(screen.getByRole('button', { name: 'Unlock value: Left compartment' }))
    await settle()
    fireEvent.change(width, { target: { value: '130' } })
    fireEvent.keyDown(width, { key: 'Enter' })
    await settle()
    const p = (await h.engine.entityInfo(id)).params
    expect(p.width).toBeCloseTo(1300, 6)
    expect((p.w1 ?? 0) + (p.w2 ?? 0) + (p.w3 ?? 0)).toBeCloseTo(1300, 6)
    expect(p.w2).toBeCloseTo(p.w3 ?? 0, 6)

    // Undo twice: back to 140 cm with the lock.
    await h.core.undo()
    await h.core.undo()
    await settle()
    expect((await h.engine.entityInfo(id)).params.width).toBeCloseTo(1400, 6)
    expect(screen.getByRole('button', { name: 'Unlock value: Left compartment' })).toBeTruthy()
    await h.core.redo()
    await settle()
    expect(screen.getByRole('button', { name: 'Lock value: Left compartment' })).toBeTruthy()
  })

  it('a value arriving while typing does not overwrite the draft; Escape restores', async () => {
    const { h, id } = await shelfSetup()
    ui(h, <Inspector />)
    await settle()
    const width = (await screen.findByLabelText('Inner width')) as HTMLInputElement
    expect(width.value).toBe('180')
    // The user starts typing; meanwhile the width changes elsewhere (another tool,
    // a collaborator, a late solve).
    fireEvent.change(width, { target: { value: '15' } })
    await h.engine.apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1700 }], mode: 'exact' }])
    await settle()
    expect(width.value).toBe('15')
    // Escape discards the draft and shows the engine value.
    fireEvent.keyDown(width, { key: 'Escape' })
    await settle()
    expect(width.value).toBe('170')
    // After a submit, engine updates show again.
    fireEvent.change(width, { target: { value: '160' } })
    fireEvent.keyDown(width, { key: 'Enter' })
    await settle()
    await h.engine.apply([{ op: 'setParams', values: [{ entity: id, param: 'width', value: 1500 }], mode: 'exact' }])
    await settle()
    await waitFor(() => expect(width.value).toBe('150'))
  })

  it('invalid input shows a message without touching the document; Turkish UI', async () => {
    const { h } = await shelfSetup()
    ui(h, <Inspector />, 'tr')
    await settle()
    const width = (await screen.findByLabelText('Inner width')) as HTMLInputElement
    const rev = h.engine.revision
    fireEvent.change(width, { target: { value: '12 parsec' } })
    fireEvent.keyDown(width, { key: 'Enter' })
    await settle()
    expect(screen.getByRole('alert').textContent).toBe(makeTranslate('tr')('inspector.invalid'))
    expect(h.engine.revision).toBe(rev)
    expect(screen.getByText('Değerler cm cinsinden')).toBeTruthy()
  })

  it('read-only objects of a disabled plugin are explained and not editable', async () => {
    const { h } = await shelfSetup()
    await h.plugins.disable('shelf.example')
    ui(h, <Inspector />)
    await settle()
    expect(await screen.findByText(/is disabled\. Enable it to edit/)).toBeTruthy()
    expect((screen.getByLabelText('Inner width') as HTMLInputElement).readOnly).toBe(true)
  })
})

describe('panels', () => {
  it('objects list selects without the canvas and supports keyboard navigation', async () => {
    const h = await handle()
    await h.engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [10, 0] }, name: 'Wall A' } },
      { op: 'createEntity', entity: { geometry: { type: 'circle', center: [0, 0], radius: 5 } } },
    ])
    ui(h, <ObjectsPanel />)
    await settle()
    const list = await screen.findByRole('listbox', { name: 'Objects' })
    fireEvent.click(within(list).getByText(/Wall A/))
    await settle()
    expect(h.core.state.getSnapshot().selection).toHaveLength(1)
    fireEvent.keyDown(list, { key: 'ArrowDown' })
    fireEvent.keyDown(list, { key: ' ', shiftKey: true })
    await settle()
    expect(h.core.state.getSnapshot().selection).toHaveLength(2)
    fireEvent.change(screen.getByLabelText('Filter objects'), { target: { value: 'circle' } })
    expect(within(list).queryByText(/Wall A/)).toBeNull()
  })

  it('layers: add, toggle visibility, choose the active layer for new objects', async () => {
    const h = await handle()
    ui(h, <LayersPanel />)
    await settle()
    fireEvent.click(screen.getByRole('button', { name: 'Add layer' }))
    await settle()
    const doc = await h.engine.documentJson()
    expect(doc.layers).toHaveLength(2)
    const added = doc.layers[1]
    if (!added) throw new Error('no layer')
    fireEvent.click(screen.getByRole('button', { name: `Visible: ${added.name}` }))
    await settle()
    expect((await h.engine.documentJson()).layers[1]?.visible).toBe(false)
    fireEvent.click(screen.getByRole('radio', { name: `Draw on this layer: ${added.name}` }))
    await h.core.apply({ commands: [{ op: 'createEntity', entity: { geometry: { type: 'point', at: [0, 0] } } }] })
    expect((await h.engine.documentJson()).entities[0]?.layer).toBe(added.id)
  })

  it('toolbar reflects registered tools and the active tool', async () => {
    const h = await handle()
    ui(h, <Toolbar />)
    fireEvent.click(screen.getByRole('button', { name: 'Rectangle' }))
    await act(async () => {
      await h.core.idle()
    })
    expect(screen.getByRole('button', { name: 'Rectangle' }).getAttribute('aria-pressed')).toBe('true')
    act(() => {
      h.core.registerTool({ id: 'acme.stamp', label: 'Stamp' })
    })
    expect(screen.getByRole('button', { name: 'Stamp' })).toBeTruthy()
  })

  it('pointer moves re-render only the status bar cursor', async () => {
    const h = await handle()
    let toolbarRenders = 0
    function CountingToolbar(): React.ReactNode {
      toolbarRenders++
      useEditorState((s) => s.tool)
      return null
    }
    ui(
      h,
      <>
        <CountingToolbar />
        <StatusBar unit="millimetre" />
      </>,
    )
    const before = toolbarRenders
    await act(async () => {
      for (let i = 0; i < 20; i++)
        await h.core.pointerMove({
          x: i,
          y: 10,
          button: 0,
          buttons: 0,
          shift: false,
          mod: false,
          alt: false,
          pointerId: 1,
          pointerType: 'mouse',
        })
    })
    expect(toolbarRenders).toBe(before)
    expect(screen.getByText(/mm, /)).toBeTruthy()
  })

  it('command palette filters, navigates and runs', async () => {
    const ran: string[] = []
    const actions = ['Undo', 'Redo', 'Export SVG', 'Tool: Line'].map((label) => ({
      id: label,
      label,
      run: () => void ran.push(label),
    }))
    let closed = false
    render(<CommandPalette actions={actions} onClose={() => (closed = true)} />)
    const input = screen.getByRole('combobox')
    fireEvent.change(input, { target: { value: 'svg' } })
    expect(screen.getAllByRole('option')).toHaveLength(1)
    fireEvent.change(input, { target: { value: 'tl' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(ran).toEqual(['Tool: Line'])
    expect(closed).toBe(true)
    expect(matchScore('Export SVG', 'es')).toBeGreaterThan(0)
  })
})

describe('constraints panel', () => {
  it('explains a non-converged file, toggles and removes rules, applies plugin templates', async () => {
    const h = await handle()
    // Three points whose distances break the triangle inequality, as a file could
    // contain them (opening never solves).
    const r = await h.engine.apply([
      { op: 'createEntity', entity: { geometry: { type: 'point', at: [0, 0] } } },
      { op: 'createEntity', entity: { geometry: { type: 'point', at: [100, 0] } } },
      { op: 'createEntity', entity: { geometry: { type: 'point', at: [50, 10] } } },
    ])
    const [a, b, c] = r.created as [number, number, number]
    const doc = await h.engine.documentJson()
    const dist = (id: number, p: number, q: number, value: number) => ({
      id,
      label: `d${id}`,
      rule: {
        kind: 'distance' as const,
        a: { entity: p, anchor: 'point' },
        b: { entity: q, anchor: 'point' },
        value,
      },
    })
    doc.constraints = [dist(100, a, b, 100), dist(101, b, c, 10), dist(102, a, c, 500)]
    doc.nextId = Math.max(doc.nextId, 200)
    await h.engine.loadJson(doc)
    await h.engine.setSelection([a, b, c])
    ui(h, <ConstraintsPanel />)
    await waitFor(() => expect(screen.getAllByText('The solver did not converge.').length).toBeGreaterThan(0))
    expect(screen.getAllByRole('checkbox')).toHaveLength(3)

    // Disabling the impossible rule makes the rest solvable (one transaction).
    fireEvent.click(screen.getByRole('checkbox', { name: 'Enabled: d102' }))
    await waitFor(async () => {
      const d = await h.engine.documentJson()
      expect(d.constraints.find((x) => x.id === 102)?.enabled).toBe(false)
    })
    await waitFor(() => expect(screen.queryByText('The solver did not converge.')).toBeNull())
    expect((screen.getByRole('checkbox', { name: 'Enabled: d102' }) as HTMLInputElement).checked).toBe(false)

    // Removing a rule.
    fireEvent.click(screen.getByRole('button', { name: 'Remove rule: d102' }))
    await waitFor(() => expect(screen.getAllByRole('checkbox')).toHaveLength(2))
    expect((await h.engine.documentJson()).constraints.map((x) => x.id)).toEqual([100, 101])

    // Plugin constraint templates appear for a matching selection size and add rules.
    await h.plugins.register({
      id: 'acme.rules',
      version: '1.0.0',
      constraintTemplates: [
        {
          id: 'pin',
          label: 'Pin point',
          arity: 1,
          build: ([id]) => [{ rule: { kind: 'fixPoint', a: { entity: id as number, anchor: 'point' }, at: [0, 0] } }],
        },
      ],
    })
    await act(async () => {
      await h.engine.setSelection([a])
    })
    fireEvent.click(await screen.findByRole('button', { name: '+ Pin point' }))
    await waitFor(async () => {
      const d = await h.engine.documentJson()
      expect(d.constraints.some((x) => x.rule.kind === 'fixPoint')).toBe(true)
    })
  })
})
