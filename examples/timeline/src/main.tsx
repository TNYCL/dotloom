/**
 * Timeline: blocks with start, duration and end. The document's time axis maps
 * seconds to drawing millimetres (08:00 at x = 0, 100 mm per hour). Ordering,
 * minimum gaps, equal durations and the locked release time are engine rules;
 * the Move-in-time tool (J) drags blocks and later blocks follow.
 */

import { loadTimelineExample, timelinePlugin } from '@dotloom/example-plugins'
import { DotloomEditor, type EditorHandle, useEditorState, useEntityInfo } from '@dotloom/react'
import '@dotloom/react/styles.css'
import { formatDuration } from '@dotloom/sdk'
import type { ReactNode } from 'react'
import { createRoot } from 'react-dom/client'

const clock = (s: number | undefined): string => {
  if (s === undefined) return '—'
  const h = Math.floor(s / 3600)
  const m = Math.round((s - h * 3600) / 60)
  return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`
}

function BlockTimes(): ReactNode {
  const selection = useEditorState((s) => s.selection)
  const info = useEntityInfo(selection.length === 1 ? (selection[0] ?? null) : null)
  if (!info || info.entity.type !== 'timeline.block') {
    return (
      <section className="dl-panel">
        <h2>Timeline</h2>
        <p className="dl-muted" style={{ margin: 0 }}>
          Select a block to see its times. K: draw a block. J: move blocks in time.
        </p>
      </section>
    )
  }
  const start = info.params.start
  const duration = info.params.duration
  return (
    <section className="dl-panel" aria-label="Block times">
      <h2>Timeline</h2>
      <p style={{ margin: 0 }}>
        <strong>{String(info.entity.props?.title ?? '')}</strong>: {clock(start)} –{' '}
        {clock((start ?? 0) + (duration ?? 0))} ({formatDuration(duration ?? 0)})
      </p>
    </section>
  )
}

const root = document.getElementById('root')
if (root) {
  createRoot(root).render(
    <DotloomEditor
      plugins={[timelinePlugin]}
      autosave={{ key: 'example-timeline' }}
      name="zaman-cizelgesi.dotl"
      theme="system"
      onReady={(h: EditorHandle) => {
        ;(window as unknown as { dotloom: EditorHandle }).dotloom = h
        void h.autosave?.recoverable().then((rec) => {
          if (rec) return
          return loadTimelineExample(h.engine).then(() => h.viewport.fit())
        })
      }}
    >
      <BlockTimes />
    </DotloomEditor>,
  )
}
