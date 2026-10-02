import { type ReactNode, useState } from 'react'
import { useDocument, useEditor, useEditorState } from '../context.js'
import { useT } from '../i18n.js'
import { Icon } from './icons.js'

/** Layers: visibility, lock, rename, active layer for new objects, add/remove. */
export function LayersPanel(): ReactNode {
  const { core } = useEditor()
  const t = useT()
  const doc = useDocument()
  const active = useEditorState((s) => s.activeLayer)
  const [editing, setEditing] = useState<number | null>(null)
  const layers = doc?.layers ?? []
  const apply = (commands: Parameters<typeof core.apply>[0]['commands'], label: string): void => {
    void core.apply({ label, commands })
  }
  return (
    <section className="dl-panel" aria-labelledby="dl-layers-h">
      <h2 id="dl-layers-h">{t('panel.layers')}</h2>
      <ul style={{ listStyle: 'none', margin: 0, padding: 0 }}>
        {[...layers].reverse().map((l) => (
          <li key={l.id} className="dl-row">
            <input
              type="radio"
              name="dl-active-layer"
              aria-label={`${t('layers.active')}: ${l.name}`}
              checked={active === l.id || (active === null && l.id === layers[0]?.id)}
              onChange={() => core.setActiveLayer(l.id)}
            />
            {editing === l.id ? (
              <input
                className="dl-input"
                aria-label={t('layers.rename')}
                defaultValue={l.name}
                // biome-ignore lint/a11y/noAutofocus: focus follows the explicit rename action
                autoFocus
                onBlur={(e) => {
                  setEditing(null)
                  const name = e.currentTarget.value.trim()
                  if (name && name !== l.name) apply([{ op: 'updateLayer', id: l.id, patch: { name } }], 'Rename layer')
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') e.currentTarget.blur()
                  if (e.key === 'Escape') setEditing(null)
                }}
              />
            ) : (
              <button
                type="button"
                className="dl-btn"
                style={{ flex: 1, justifyContent: 'flex-start', border: 'none', background: 'transparent' }}
                title={t('layers.rename')}
                onDoubleClick={() => setEditing(l.id)}
                onKeyDown={(e) => {
                  if (e.key === 'F2' || e.key === 'Enter') setEditing(l.id)
                }}
              >
                {l.name}
              </button>
            )}
            <button
              type="button"
              className="dl-btn dl-icon-btn"
              aria-label={`${t('layers.visible')}: ${l.name}`}
              aria-pressed={l.visible !== false}
              onClick={() =>
                apply([{ op: 'updateLayer', id: l.id, patch: { visible: l.visible === false } }], 'Layer visibility')
              }
            >
              <Icon name="eye" />
            </button>
            <button
              type="button"
              className="dl-btn dl-icon-btn"
              aria-label={`${t('layers.locked')}: ${l.name}`}
              aria-pressed={l.locked === true}
              onClick={() => apply([{ op: 'updateLayer', id: l.id, patch: { locked: !l.locked } }], 'Layer lock')}
            >
              <Icon name={l.locked ? 'lock' : 'unlock'} />
            </button>
            <button
              type="button"
              className="dl-btn dl-icon-btn"
              aria-label={`${t('layers.remove')}: ${l.name}`}
              disabled={layers.length <= 1}
              onClick={() => apply([{ op: 'removeLayer', id: l.id }], 'Remove layer')}
            >
              <Icon name="trash" />
            </button>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className="dl-btn"
        onClick={() => apply([{ op: 'addLayer', name: t('layers.newName', { n: layers.length + 1 }) }], 'Add layer')}
      >
        <Icon name="plus" /> {t('layers.add')}
      </button>
    </section>
  )
}
