/**
 * The complete `<DotloomEditor>` in jsdom (DL-UI-1/7/8, DL-PLUGIN-1, DL-FILE-9): the
 * real engine runs in-thread; the real renderer module loads, and the GPU backends
 * fail because jsdom has neither WebGPU nor WebGL2 — which is the GPU-failure state.
 */

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import {
  type DotloomPlugin,
  type EngineOptions,
  type EntityTypeDef,
  InlineTransport,
  MemoryStorage,
  type StoredMeta,
  TOOL_MESSAGES_EN,
  TOOL_MESSAGES_TR,
  toolText,
} from '@dotloomjs/sdk'
import { createNodeEngine } from '@dotloomjs/sdk/node'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { initSync, WasmEngine } from '../../sdk/src/wasm/engine/dotloom_wasm.js'
import type { EditorHandle } from '../src/context.js'
import { DotloomEditor, type DotloomEditorProps } from '../src/DotloomEditor.js'
import { en, tr } from '../src/i18n.js'

const sdk = resolve(process.cwd(), '../sdk/src')
const shelf = JSON.parse(
  readFileSync(resolve(process.cwd(), '../../tests/fixtures/plugins/shelf.json'), 'utf8'),
) as EntityTypeDef
const renderWasm = `data:application/wasm;base64,${readFileSync(
  resolve(sdk, 'wasm/render/dotloom_render_web_bg.wasm'),
).toString('base64')}`

function inlineEngine(stepBudget?: number): EngineOptions {
  return {
    transport: new InlineTransport(async () => {
      initSync({ module: readFileSync(resolve(sdk, 'wasm/engine/dotloom_wasm_bg.wasm')) })
      return WasmEngine
    }, stepBudget),
  }
}

afterEach(() => {
  cleanup()
})

/** Render the editor and wait until it is ready. */
async function editor(props: Partial<DotloomEditorProps> = {}): Promise<EditorHandle> {
  let ready: EditorHandle | null = null
  render(
    <DotloomEditor
      engine={inlineEngine()}
      viewport={{ renderWasmUrl: renderWasm }}
      autosave={false}
      {...props}
      onReady={(h) => {
        ready = h
      }}
      style={{ height: 600 }}
    />,
  )
  await waitFor(() => expect(ready).not.toBeNull(), { timeout: 20_000 })
  return ready as unknown as EditorHandle
}

describe('editor states', () => {
  it('explains a GPU start failure with every attempt and stays usable without a canvas', async () => {
    const h = await editor()
    expect(h.canvas).toBeNull()
    const alert = screen.getByText('Graphics could not start').closest('[role="alert"]') as HTMLElement
    // Backend errors are developer details, behind a disclosure.
    expect(within(alert).getByText('Technical details').tagName).toBe('SUMMARY')
    const attempts = within(alert)
      .getAllByRole('listitem')
      .map((li) => li.textContent ?? '')
    expect(attempts.map((a) => a.split(':')[0])).toEqual(['webgpu', 'webgl2'])
    expect(attempts.every((a) => a.length > 'webgl2: '.length)).toBe(true)
    // Empty drawing: no note while the GPU message is shown; the panels work
    // (non-canvas editing path): create through the engine, select in the list.
    await act(async () => {
      await h.core.apply({
        label: 'Line',
        commands: [{ op: 'createEntity', entity: { geometry: { type: 'line', a: [0, 0], b: [100, 0] } } }],
      })
    })
    const objects = screen.getByRole('listbox', { name: 'Objects' })
    fireEvent.click(await within(objects).findByRole('option'))
    await waitFor(() => expect(h.core.state.getSnapshot().selection).toHaveLength(1))
  })

  it('shows the GPU message instead of the empty-drawing note when graphics failed', async () => {
    // The note itself (graphics working) is checked in the browser:
    // tests/e2e/specs/playground.spec.ts.
    const h = await editor()
    expect(screen.getByText('Graphics could not start')).toBeTruthy()
    expect(screen.queryByText('Empty drawing')).toBeNull()
    expect(h.core.state.getSnapshot().tool).toBe('select')
  })

  it('reports a corrupt file without crashing and keeps an empty, working document', async () => {
    const h = await editor({ document: new Uint8Array([0x50, 0x4b, 0x03, 0x04, 1, 2, 3]) })
    const alert = await screen.findByText('The file could not be opened')
    expect(alert.closest('[role="alert"]')?.textContent).toMatch(/zip|dotl|corrupt|invalid|archive/i)
    expect((await h.engine.documentJson()).entities).toHaveLength(0)
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    await waitFor(() => expect(screen.queryByText('The file could not be opened')).toBeNull())
  })

  it('names missing plugin types of an opened file and keeps their objects read-only', async () => {
    const author = await createNodeEngine({ engineWasmUrl: resolve(sdk, 'wasm/engine/dotloom_wasm_bg.wasm') })
    await author.registerTypes([shelf], 'shelf.example')
    await author.apply([{ op: 'createEntity', entity: { type: 'shelf.unit' } }])
    const bytes = await author.save()
    author.dispose()
    const h = await editor({ document: bytes })
    expect(
      await screen.findByText(/These object types are not available: shelf\.unit\. Their objects are read-only/),
    ).toBeTruthy()
    const [entity] = (await h.engine.documentJson()).entities
    expect((await h.engine.entityInfo(entity?.id as number)).readOnly?.reason).toBe('missingPlugin')
  })

  it('shows the busy state of a long solve and cancels it from the status bar', async () => {
    const h = await editor({ engine: inlineEngine(1) })
    // A chain of 60 linked lines with fixed lengths; moving its middle far away takes
    // many solver steps, each followed by a yield to the event loop.
    const ids = await h.engine.reserveIds(60)
    const commands: Parameters<typeof h.engine.apply>[0] = []
    const cmds = commands as Exclude<typeof commands, { commands: unknown }>
    for (let i = 0; i < 60; i++) {
      cmds.push({
        op: 'createEntity',
        id: ids[i] as number,
        entity: { geometry: { type: 'line', a: [i * 10, 0], b: [i * 10 + 10, 0] } },
      })
    }
    for (let i = 0; i < 59; i++) {
      const a = ids[i] as number
      const b = ids[i + 1] as number
      cmds.push(
        {
          op: 'addConstraint',
          constraint: {
            rule: { kind: 'coincident', a: { entity: a, anchor: 'end' }, b: { entity: b, anchor: 'start' } },
          },
        },
        {
          op: 'addConstraint',
          constraint: {
            rule: {
              kind: 'length',
              line: { from: { entity: a, anchor: 'start' }, to: { entity: a, anchor: 'end' } },
              value: 10,
            },
          },
        },
      )
    }
    await h.engine.apply(cmds)
    const before = (await h.engine.documentJson()).entities
    let result: unknown = 'pending'
    void h.core
      .apply({
        label: 'Move',
        commands: [
          { op: 'setParams', values: [{ entity: ids[30] as number, param: 'a.y', value: 250 }], mode: 'prefer' },
        ],
      })
      .then((r) => {
        result = r
      })
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(result).toBeNull())
    expect(screen.getByText(/Solving was cancelled|cancel/i)).toBeTruthy()
    expect((await h.engine.documentJson()).entities).toEqual(before)
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull()
  })
})

describe('lifecycle', () => {
  it('unmounting releases the engine, plugins, autosave and listeners', async () => {
    const released: string[] = []
    const plugin: DotloomPlugin = {
      id: 'acme.probe',
      version: '1.0.0',
      activate(api) {
        api.onDispose(() => released.push('plugin'))
        return undefined
      },
    }
    let ready: EditorHandle | null = null
    const view = render(
      <DotloomEditor
        engine={inlineEngine()}
        viewport={{ renderWasmUrl: renderWasm }}
        plugins={[plugin]}
        autosave={{ key: 'probe', storage: new MemoryStorage() }}
        onReady={(h) => {
          ready = h
        }}
      />,
    )
    await waitFor(() => expect(ready).not.toBeNull(), { timeout: 20_000 })
    const h = ready as unknown as EditorHandle
    let autosaveDisposed = false
    const dispose = h.autosave?.dispose.bind(h.autosave)
    if (h.autosave && dispose) {
      h.autosave.dispose = () => {
        autosaveDisposed = true
        dispose()
      }
    }
    view.unmount()
    await waitFor(() => expect(released).toEqual(['plugin']))
    expect(autosaveDisposed).toBe(true)
    expect(h.plugins.list()).toEqual([])
    await expect(h.engine.documentJson()).rejects.toMatchObject({ code: 'disposed' })
  })
})

describe('plugin contributions in the editor', () => {
  it('mounts plugin panels for matching selections and autosaves to the plugin storage', async () => {
    class RecordingStorage extends MemoryStorage {
      writes = 0
      override async write(key: string, bytes: Uint8Array, meta: StoredMeta): Promise<void> {
        this.writes++
        await super.write(key, bytes, meta)
      }
    }
    const store = new RecordingStorage()
    const mounted: string[] = []
    const plugin: DotloomPlugin = {
      id: 'shelf.example',
      version: '1.0.0',
      types: [shelf],
      storage: [store],
      panels: [
        {
          id: 'info',
          title: 'Shelf info',
          forTypes: ['shelf.unit'],
          mount(el, api) {
            mounted.push('mount')
            el.textContent = `${api.selection().length} shelf selected`
            return () => {
              mounted.push('unmount')
            }
          },
        },
      ],
    }
    const h = await editor({ plugins: [plugin], autosave: { key: 'test' } })
    expect(h.autosave).not.toBeNull()
    expect(screen.queryByRole('heading', { name: 'Shelf info' })).toBeNull()
    let id = 0
    await act(async () => {
      const r = await h.core.apply({
        label: 'Shelf',
        commands: [{ op: 'createEntity', entity: { type: 'shelf.unit' } }],
      })
      id = r?.created[0] as number
      await h.engine.setSelection([id])
    })
    expect(await screen.findByRole('heading', { name: 'Shelf info' })).toBeTruthy()
    expect(screen.getByText('1 shelf selected')).toBeTruthy()
    // Autosave went to the plugin's adapter (the default is IndexedDB).
    await waitFor(() => expect(store.writes).toBeGreaterThan(0), { timeout: 5000 })
    expect((await store.list()).map((f) => f.key)).toContain('test')
    // A selection without the type hides the panel; disabling the plugin unmounts it.
    await act(async () => {
      await h.engine.setSelection([])
    })
    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Shelf info' })).toBeNull())
    expect(mounted).toEqual(['mount', 'unmount'])
    await act(async () => {
      await h.engine.setSelection([id])
    })
    await screen.findByRole('heading', { name: 'Shelf info' })
    await act(async () => {
      await h.plugins.disable('shelf.example')
    })
    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Shelf info' })).toBeNull())
    expect(mounted).toEqual(['mount', 'unmount', 'mount', 'unmount'])
  })
})

describe('host storage', () => {
  it('a host-supplied adapter receives autosaves; its failures are shown and recover', async () => {
    class FlakyStorage extends MemoryStorage {
      fail: Error | null = new DOMException('The quota has been exceeded.', 'QuotaExceededError')
      override async write(key: string, bytes: Uint8Array, meta: StoredMeta): Promise<void> {
        if (this.fail) throw this.fail
        await super.write(key, bytes, meta)
      }
    }
    const store = new FlakyStorage()
    const h = await editor({ autosave: { key: 'host', storage: store } })
    const line = (y: number) => ({
      label: 'Line',
      commands: [
        {
          op: 'createEntity' as const,
          entity: {
            geometry: { type: 'line' as const, a: [0, y] as [number, number], b: [10, y] as [number, number] },
          },
        },
      ],
    })
    await act(async () => {
      await h.core.apply(line(0))
    })
    expect(await screen.findByText('Autosave failed: The quota has been exceeded.', {}, { timeout: 5000 })).toBeTruthy()
    // Not stuck in "Saving…": the changes are reported as unsaved.
    expect(screen.queryByText('Saving…')).toBeNull()
    expect(screen.getByText('Unsaved changes')).toBeTruthy()
    // The document itself is untouched by the storage failure.
    expect((await h.engine.documentJson()).entities).toHaveLength(1)
    store.fail = null
    await act(async () => {
      await h.core.apply(line(10))
    })
    await waitFor(() => expect(screen.queryByText(/Autosave failed/)).toBeNull(), { timeout: 5000 })
    expect((await store.list()).map((f) => f.key)).toEqual(['host'])
    expect((await store.meta('host'))?.clean).toBe(false)
  })
})

describe('themes and messages', () => {
  it('the system theme follows the OS preference and its changes', async () => {
    let dark = true
    const listeners = new Set<() => void>()
    const original = window.matchMedia
    window.matchMedia = ((query: string) => ({
      get matches() {
        return query.includes('dark') && dark
      },
      media: query,
      addEventListener: (_: string, cb: () => void) => listeners.add(cb),
      removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
    })) as unknown as typeof window.matchMedia
    try {
      const { container } = render(<DotloomEditor engine={inlineEngine()} autosave={false} theme="system" />)
      const root = container.querySelector('.dl-editor') as HTMLElement
      expect(root.dataset.theme).toBe('dark')
      dark = false
      act(() => {
        for (const cb of listeners) cb()
      })
      expect(root.dataset.theme).toBe('light')
    } finally {
      window.matchMedia = original
    }
  })

  it('every English message has a Turkish translation with the same placeholders', () => {
    const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()
    expect(Object.keys(tr).sort()).toEqual(Object.keys(en).sort())
    for (const [key, text] of Object.entries(en)) {
      const t = (tr as Record<string, string>)[key] ?? ''
      expect(t.trim(), key).not.toBe('')
      expect(placeholders(t), key).toEqual(placeholders(text))
    }
    expect(Object.keys(TOOL_MESSAGES_TR).sort()).toEqual(Object.keys(TOOL_MESSAGES_EN).sort())
    expect(toolText('tool.line.start', 'tr-TR')).toBe(TOOL_MESSAGES_TR['tool.line.start'])
    expect(toolText('acme.custom', 'tr')).toBe('acme.custom')
  })
})
