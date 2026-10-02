import { formatLength, LENGTH_UNITS, type LengthUnit } from '@dotloom/sdk'
import { type ReactNode, useSyncExternalStore } from 'react'
import { useAutosaveState, useEditor, useEditorState, usePointer } from '../context.js'
import { useT } from '../i18n.js'

function Cursor(props: { unit: LengthUnit }): ReactNode {
  // Only this component re-renders on pointer moves.
  const p = usePointer()
  const t = useT()
  if (!p.world) return <span aria-hidden="true" style={{ minWidth: 180 }} />
  return (
    <span style={{ minWidth: 180, fontVariantNumeric: 'tabular-nums' }}>
      {formatLength(p.world[0], props.unit)}, {formatLength(p.world[1], props.unit)}
      {p.snap && (
        <span className="dl-badge">
          {' '}
          {t('status.snap')}: {p.snap.kind}
        </span>
      )}
    </span>
  )
}

function Zoom(): ReactNode {
  const { canvas, viewport } = useEditor()
  const subscribe = (cb: () => void): (() => void) => (canvas ? canvas.on('camera', cb) : () => undefined)
  const scale = useSyncExternalStore(subscribe, () => viewport.camera.scale)
  return <span title="CSS px per mm">{Math.round(scale * 100)}%</span>
}

/** Prompt, typed value, cursor position, toggles, solving/autosave state. */
export function StatusBar(props: { unit: LengthUnit }): ReactNode {
  const { core } = useEditor()
  const t = useT()
  const prompt = useEditorState((s) => s.prompt)
  const input = useEditorState((s) => s.input)
  const busy = useEditorState((s) => s.busy)
  const snap = useEditorState((s) => s.snapEnabled)
  const grid = useEditorState((s) => s.gridVisible)
  const save = useAutosaveState()
  const sym = LENGTH_UNITS[props.unit].symbol
  return (
    <footer className="dl-statusbar">
      <span className="dl-prompt" role="status" aria-live="polite">
        {prompt ? t(prompt) : ''}
      </span>
      {input && (
        <span className="dl-badge">
          <span className="dl-visually-hidden">{t('status.input')}: </span>
          {input} {sym}
        </span>
      )}
      {busy && (
        <span role="status">
          {t('status.busy')}{' '}
          <button type="button" className="dl-btn" onClick={() => core.cancelSolve()}>
            {t('status.cancel')}
          </button>
        </span>
      )}
      <Cursor unit={props.unit} />
      <label>
        <input type="checkbox" checked={snap} onChange={() => void core.run('toggleSnap')} /> {t('status.snap')}
      </label>
      <label>
        <input type="checkbox" checked={grid} onChange={() => void core.run('toggleGrid')} /> {t('status.grid')}
      </label>
      <Zoom />
      {save && (
        <span>
          {save.saving
            ? t('status.saving')
            : save.dirty
              ? save.lastSavedAt
                ? t('status.autosaved', { time: new Date(save.lastSavedAt).toLocaleTimeString() })
                : t('status.unsaved')
              : t('status.saved')}
        </span>
      )}
    </footer>
  )
}
