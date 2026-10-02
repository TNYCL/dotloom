import type { ReactNode } from 'react'
import { useEditor, useEditorState } from '../context.js'
import { useT } from '../i18n.js'
import { Icon } from './icons.js'

const GROUPS = [
  ['select', 'pan'],
  ['line', 'polyline', 'rect', 'circle', 'arc', 'path', 'text', 'dimension'],
  ['move', 'rotate', 'scale', 'split', 'trim', 'extend'],
]

/** Vertical tool bar with every registered tool (built-in and plugin tools). */
export function Toolbar(): ReactNode {
  const { core } = useEditor()
  const t = useT()
  const active = useEditorState((s) => s.tool)
  useEditorState((s) => s.toolsVersion)
  const tools = core.listTools()
  const known = new Set(GROUPS.flat())
  const ordered = [
    ...GROUPS.flat().flatMap((id) => tools.filter((x) => x.id === id)),
    ...tools.filter((x) => !known.has(x.id)),
  ]
  return (
    <nav className="dl-toolbar" role="toolbar" aria-orientation="vertical" aria-label="Tools">
      {ordered.map((tool) => {
        const label = t(tool.label)
        const title = tool.shortcut ? `${label} (${tool.shortcut.toUpperCase()})` : label
        return (
          <button
            key={tool.id}
            type="button"
            className="dl-btn dl-icon-btn"
            aria-pressed={active === tool.id}
            aria-label={label}
            aria-keyshortcuts={tool.shortcut?.toUpperCase()}
            title={title}
            data-tool={tool.id}
            onClick={() => core.setTool(tool.id)}
          >
            <Icon name={tool.id} />
          </button>
        )
      })}
    </nav>
  )
}
