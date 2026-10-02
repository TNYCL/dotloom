/**
 * Shelf configurator with a host-built UI. The page shows its own form; every
 * number comes from the engine and every rule is the shelf type's rule — the
 * page never recomputes compartments itself.
 */

import { loadShelfExample, shelfPlugin } from '@dotloom/example-plugins'
import {
  type EditorHandle,
  EditorProvider,
  useDocument,
  useEditor,
  useEditorState,
  useEntityInfo,
} from '@dotloom/react'
import { createEditor, DotloomError, PluginHost } from '@dotloom/sdk'
import { type ReactNode, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'

const cm = (mm: number | undefined): string => (mm === undefined ? '—' : `${Math.round(mm) / 10} cm`)

function ShelfPanel(props: { id: number }): ReactNode {
  const { core, engine } = useEditor()
  const info = useEntityInfo(props.id)
  const doc = useDocument()
  const canUndo = useEditorState((s) => s.canUndo)
  const canRedo = useEditorState((s) => s.canRedo)
  const [text, setText] = useState('')
  const [message, setMessage] = useState<{ text: string; nearest?: number } | null>(null)
  const p = info?.params ?? {}
  useEffect(() => {
    if (p.width !== undefined) setText(String(Math.round(p.width) / 10))
  }, [p.width])
  const lock = doc?.constraints.find(
    (c) => c.rule.kind === 'fix' && 'prop' in c.rule.param && c.rule.param.prop === 'w1',
  )
  const setWidth = async (mm: number): Promise<void> => {
    try {
      await core.applyOrThrow({
        label: 'Width',
        commands: [{ op: 'setParams', values: [{ entity: props.id, param: 'width', value: mm }] }],
      })
      setMessage(null)
    } catch (e) {
      if (e instanceof DotloomError) {
        const d = e.details as { failure?: { nearest?: { feasible: number }[]; diagnostics?: { labels: string[] }[] } }
        const nearest = d.failure?.nearest?.[0]?.feasible
        const labels = d.failure?.diagnostics?.flatMap((x) => x.labels).join('; ')
        setMessage({ text: `Not possible: ${labels || e.message}`, ...(nearest !== undefined ? { nearest } : {}) })
      }
    }
  }
  return (
    <aside style={{ padding: 16, display: 'grid', gap: 12, alignContent: 'start', borderLeft: '1px solid #ccd' }}>
      <h1 style={{ fontSize: 18, margin: 0 }}>Shelf configurator</h1>
      <label>
        Inner width (cm){' '}
        <input
          aria-label="Inner width in cm"
          value={text}
          inputMode="decimal"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void setWidth(Number(text.replace(',', '.')) * 10)
          }}
          style={{ width: 80 }}
        />
      </label>
      {message && (
        <div role="alert" style={{ color: '#b00020' }}>
          {message.text}
          {message.nearest !== undefined && (
            <div>
              <button type="button" onClick={() => void setWidth(message.nearest as number)}>
                Use {cm(message.nearest)}
              </button>
            </div>
          )}
        </div>
      )}
      <table>
        <tbody>
          <tr>
            <th scope="row">A (left)</th>
            <td data-testid="w1">{cm(p.w1)}</td>
          </tr>
          <tr>
            <th scope="row">B (middle)</th>
            <td data-testid="w2">{cm(p.w2)}</td>
          </tr>
          <tr>
            <th scope="row">C (right)</th>
            <td data-testid="w3">{cm(p.w3)}</td>
          </tr>
        </tbody>
      </table>
      <label>
        <input
          type="checkbox"
          checked={lock !== undefined}
          onChange={() =>
            void core.apply(
              lock
                ? { label: 'Unlock', commands: [{ op: 'removeConstraint', id: lock.id }] }
                : {
                    label: 'Lock',
                    commands: [
                      {
                        op: 'addConstraint',
                        constraint: {
                          rule: { kind: 'fix', param: { entity: props.id, prop: 'w1' }, value: p.w1 ?? 600 },
                          label: 'left compartment locked',
                        },
                      },
                    ],
                  },
            )
          }
        />{' '}
        Lock the left compartment
      </label>
      <p style={{ margin: 0, color: '#555' }}>Rules: compartments fill the width; B = C; B, C ≥ 40 cm.</p>
      <div style={{ display: 'flex', gap: 8 }}>
        <button type="button" disabled={!canUndo} onClick={() => void core.undo()}>
          Undo
        </button>
        <button type="button" disabled={!canRedo} onClick={() => void core.redo()}>
          Redo
        </button>
      </div>
      <p style={{ fontSize: 12, color: '#777' }}>
        Engine revision {useEditorState((s) => s.revision)} · {engine.capabilities?.engineVersion}
      </p>
    </aside>
  )
}

function App(): ReactNode {
  const host = useRef<HTMLDivElement>(null)
  const [state, setState] = useState<{ handle: EditorHandle; id: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    let dispose: (() => void) | null = null
    void (async () => {
      const el = host.current
      if (!el) return
      const ed = await createEditor(el)
      dispose = () => ed.dispose()
      if (!alive) return dispose()
      const plugins = new PluginHost(ed.engine, ed.core)
      await plugins.register(shelfPlugin)
      const id = await loadShelfExample(ed.engine)
      await ed.viewport.fit()
      if (!alive) return
      setState({
        handle: {
          engine: ed.engine,
          core: ed.core,
          viewport: ed.viewport,
          canvas: ed.viewport,
          plugins,
          autosave: null,
        },
        id,
      })
      ;(window as unknown as { dotloom: EditorHandle }).dotloom = {
        engine: ed.engine,
        core: ed.core,
        viewport: ed.viewport,
        canvas: ed.viewport,
        plugins,
        autosave: null,
      }
    })().catch((e) => alive && setError(String(e)))
    return () => {
      alive = false
      dispose?.()
    }
  }, [])
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 320px', height: '100%' }}>
      <div ref={host} style={{ position: 'relative', minHeight: 0 }} />
      {error && <p role="alert">{error}</p>}
      {state && (
        <EditorProvider value={state.handle}>
          <ShelfPanel id={state.id} />
        </EditorProvider>
      )}
    </div>
  )
}

const root = document.getElementById('root')
if (root) createRoot(root).render(<App />)
