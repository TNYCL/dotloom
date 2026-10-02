import type { Entity } from '@dotloom/sdk'
import { type KeyboardEvent, type ReactNode, useMemo, useRef, useState } from 'react'
import { useDocument, useEditor, useEditorState, usePluginTypes } from '../context.js'
import { useT } from '../i18n.js'

const ROW = 26
const VIEW = 240

function label(e: Entity, typeLabel: string): string {
  return e.name ? `${e.name} — ${typeLabel}` : `${typeLabel} #${e.id}`
}

/**
 * Objects list: virtualized (only visible rows are rendered), keyboard navigable
 * (arrows, Home/End, Enter/Space selects, Shift/Ctrl extend), filterable.
 * Selecting here is the canvas-free path to every object.
 */
export function ObjectsPanel(): ReactNode {
  const { engine } = useEditor()
  const t = useT()
  const doc = useDocument()
  const types = usePluginTypes()
  const selection = useEditorState((s) => s.selection)
  const [filter, setFilter] = useState('')
  const [scroll, setScroll] = useState(0)
  const [focus, setFocus] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)
  const rows = useMemo(() => {
    const all = doc?.entities ?? []
    const f = filter.trim().toLowerCase()
    return all
      .map((e) => {
        const tl = types.get(e.type)?.label ?? (e.type.startsWith('dotloom.') ? e.type.slice(8) : e.type)
        return { e, text: label(e, tl), readOnly: !e.type.startsWith('dotloom.') && !types.has(e.type) }
      })
      .filter((r) => !f || r.text.toLowerCase().includes(f))
  }, [doc, filter, types])
  const sel = new Set(selection)
  const select = (id: number, extend: boolean): void => {
    const next = extend ? (sel.has(id) ? selection.filter((x) => x !== id) : [...selection, id]) : [id]
    void engine.setSelection(next)
  }
  const first = Math.max(0, Math.floor(scroll / ROW) - 4)
  const last = Math.min(rows.length, Math.ceil((scroll + VIEW) / ROW) + 4)
  const onKey = (ev: KeyboardEvent<HTMLDivElement>): void => {
    let f = focus
    if (ev.key === 'ArrowDown') f = Math.min(rows.length - 1, focus + 1)
    else if (ev.key === 'ArrowUp') f = Math.max(0, focus - 1)
    else if (ev.key === 'Home') f = 0
    else if (ev.key === 'End') f = rows.length - 1
    else if (ev.key === 'Enter' || ev.key === ' ') {
      const r = rows[focus]
      if (r) select(r.e.id, ev.shiftKey || ev.ctrlKey || ev.metaKey)
      ev.preventDefault()
      return
    } else return
    ev.preventDefault()
    setFocus(f)
    const el = listRef.current
    if (el) {
      if (f * ROW < el.scrollTop) el.scrollTop = f * ROW
      if ((f + 1) * ROW > el.scrollTop + VIEW) el.scrollTop = (f + 1) * ROW - VIEW
    }
  }
  const focusedId = rows[focus]?.e.id
  return (
    <section className="dl-panel" aria-labelledby="dl-objects-h">
      <h2 id="dl-objects-h">
        {t('panel.objects')} <span className="dl-muted">({t('objects.count', { n: doc?.entities.length ?? 0 })})</span>
      </h2>
      <input
        className="dl-input"
        type="search"
        placeholder={t('objects.filter')}
        aria-label={t('objects.filter')}
        value={filter}
        onChange={(e) => {
          setFilter(e.target.value)
          setFocus(0)
        }}
        style={{ marginBottom: 6 }}
      />
      {rows.length === 0 ? (
        <p className="dl-muted">{t('objects.empty')}</p>
      ) : (
        <div
          ref={listRef}
          className="dl-list"
          style={{ height: Math.min(VIEW, rows.length * ROW + 2) }}
          role="listbox"
          aria-multiselectable="true"
          aria-label={t('panel.objects')}
          tabIndex={0}
          aria-activedescendant={focusedId !== undefined ? `dl-obj-${focusedId}` : undefined}
          onScroll={(e) => setScroll(e.currentTarget.scrollTop)}
          onKeyDown={onKey}
        >
          <div style={{ height: rows.length * ROW }} />
          {rows.slice(first, last).map((r, i) => {
            const idx = first + i
            return (
              <div
                key={r.e.id}
                id={`dl-obj-${r.e.id}`}
                role="option"
                tabIndex={-1}
                aria-selected={sel.has(r.e.id)}
                className="dl-list-item"
                style={{ top: idx * ROW, outline: idx === focus ? '1px dashed var(--dl-focus)' : undefined }}
                onClick={(ev) => {
                  setFocus(idx)
                  select(r.e.id, ev.shiftKey || ev.ctrlKey || ev.metaKey)
                }}
                onKeyDown={() => undefined}
              >
                <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{r.text}</span>
                {r.e.locked && <span className="dl-badge">🔒</span>}
                {r.e.hidden && <span className="dl-badge">hidden</span>}
                {r.readOnly && <span className="dl-badge">{t('objects.readOnly')}</span>}
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}
