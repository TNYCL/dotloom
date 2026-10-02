/**
 * Room planner: the reference editor with the floor plan plugin. Wall (W) and
 * Door (Q) tools come from the plugin; dimensions, layers and snapping from the
 * framework. Doors stay on their walls; deleting a wall deletes its doors.
 */

import { floorplanPlugin, loadFloorplanExample } from '@dotloomjs/example-plugins'
import { DotloomEditor, type EditorHandle } from '@dotloomjs/react'
import '@dotloomjs/react/styles.css'
import type { ReactNode } from 'react'
import { createRoot } from 'react-dom/client'

function Help(): ReactNode {
  return (
    <section className="dl-panel">
      <h2>Room planner</h2>
      <p className="dl-muted" style={{ margin: 0 }}>
        W: draw connected walls (click the first point to close). Q: click a wall to add a door. Drag wall corners: the
        walls stay connected, doors slide along their wall, dimensions follow.
      </p>
    </section>
  )
}

const root = document.getElementById('root')
if (root) {
  createRoot(root).render(
    <DotloomEditor
      plugins={[floorplanPlugin]}
      autosave={{ key: 'example-floorplan' }}
      name="ev-plani.dotl"
      theme="system"
      onReady={(h: EditorHandle) => {
        ;(window as unknown as { dotloom: EditorHandle }).dotloom = h
        void h.autosave?.recoverable().then((rec) => {
          if (rec) return
          return loadFloorplanExample(h.engine).then(() => h.viewport.fit())
        })
      }}
    >
      <Help />
    </DotloomEditor>,
  )
}
