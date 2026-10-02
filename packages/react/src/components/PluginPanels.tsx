import type { EntityId, PanelContribution } from '@dotloom/sdk'
import { type ReactNode, useEffect, useRef, useState } from 'react'
import { useEditor, useEditorState, useRevisionQuery } from '../context.js'

/** One mounted plugin panel. The plugin renders into the element with its own code. */
function MountedPanel(props: { plugin: string; panel: PanelContribution }): ReactNode {
  const { plugins } = useEditor()
  const el = useRef<HTMLDivElement>(null)
  const { plugin, panel } = props
  useEffect(() => {
    const target = el.current
    if (!target) return
    return plugins.mountPanel(plugin, panel.id, target)
  }, [plugins, plugin, panel])
  const heading = `dl-plugin-${plugin}-${panel.id}`.replace(/[^a-zA-Z0-9_-]/g, '-')
  return (
    <section className="dl-panel" aria-labelledby={heading} data-plugin={plugin}>
      <h2 id={heading}>{panel.title}</h2>
      <div ref={el} />
    </section>
  )
}

/**
 * Panels contributed by enabled plugins (DL-PLUGIN-1). A panel with `forTypes` is
 * shown only while the selection contains an entity of one of those types.
 */
export function PluginPanels(): ReactNode {
  const { plugins } = useEditor()
  const [list, setList] = useState(() => plugins.panels())
  useEffect(() => {
    setList(plugins.panels())
    return plugins.subscribe(() => setList(plugins.panels()))
  }, [plugins])
  const selection = useEditorState((s) => s.selection)
  const needsTypes = list.some((p) => p.panel.forTypes?.length)
  const types = useRevisionQuery(
    async (engine) => {
      if (!needsTypes) return new Set<string>()
      const infos = await Promise.all(selection.map((id: EntityId) => engine.entityInfo(id).catch(() => null)))
      return new Set(infos.flatMap((i) => (i ? [i.entity.type] : [])))
    },
    [selection, needsTypes],
  )
  const shown = list.filter(
    ({ panel }) => !panel.forTypes?.length || panel.forTypes.some((t) => types?.has(t) ?? false),
  )
  return (
    <>
      {shown.map(({ plugin, panel }) => (
        <MountedPanel key={`${plugin}:${panel.id}`} plugin={plugin} panel={panel} />
      ))}
    </>
  )
}
