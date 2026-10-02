/**
 * Dotloom playground: the reference React editor with the example plugins.
 * `?example=shelf|floorplan|timeline` loads an example on start.
 */

import {
  floorplanPlugin,
  loadFloorplanExample,
  loadShelfExample,
  loadTimelineExample,
  shelfPlugin,
  timelinePlugin,
} from '@dotloomjs/example-plugins'
import { DotloomEditor, type EditorHandle, useEditor, useT } from '@dotloomjs/react'
import '@dotloomjs/react/styles.css'
import type { DotloomEngine } from '@dotloomjs/sdk'
import { type ReactNode, StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'

const EXAMPLES: Record<string, { label: string; tr: string; load: (e: DotloomEngine) => Promise<unknown> }> = {
  shelf: { label: 'Shelf configurator', tr: 'Raf konfigüratörü', load: loadShelfExample },
  floorplan: { label: 'Room planner', tr: 'Oda planlayıcı', load: loadFloorplanExample },
  timeline: { label: 'Timeline', tr: 'Zaman çizelgesi', load: loadTimelineExample },
}

const turkish = typeof navigator !== 'undefined' && navigator.language.toLowerCase().startsWith('tr')

async function loadExample(h: EditorHandle, key: string): Promise<void> {
  const ex = EXAMPLES[key]
  if (!ex) return
  await ex.load(h.engine)
  await h.autosave?.markSaved()
  await h.viewport.fit()
}

function ExamplesPanel(): ReactNode {
  const h = useEditor()
  const t = useT()
  const [busy, setBusy] = useState(false)
  return (
    <section className="dl-panel" aria-labelledby="pg-examples">
      <h2 id="pg-examples">{turkish ? 'Örnekler' : 'Examples'}</h2>
      <div className="dl-row" style={{ flexWrap: 'wrap' }}>
        {Object.entries(EXAMPLES).map(([key, ex]) => (
          <button
            key={key}
            type="button"
            className="dl-btn"
            disabled={busy}
            onClick={() => {
              setBusy(true)
              loadExample(h, key)
                .catch((e) => h.core.report(e))
                .finally(() => setBusy(false))
            }}
          >
            {turkish ? ex.tr : ex.label}
          </button>
        ))}
      </div>
      <p className="dl-muted" style={{ marginBottom: 0 }}>
        {t('menu.commands')}: Ctrl+K
      </p>
    </section>
  )
}

const params = new URLSearchParams(location.search)
const root = document.getElementById('root')
if (root) {
  createRoot(root).render(
    <StrictMode>
      <DotloomEditor
        plugins={[shelfPlugin, floorplanPlugin, timelinePlugin]}
        locale={turkish ? 'tr' : 'en'}
        theme="system"
        name="playground.dotl"
        onReady={(h) => {
          const ex = params.get('example')
          if (ex) void loadExample(h, ex)
          ;(window as unknown as { dotloom: EditorHandle }).dotloom = h
        }}
      >
        <ExamplesPanel />
      </DotloomEditor>
    </StrictMode>,
  )
}
