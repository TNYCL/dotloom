/** Editor actions shared by the menu bar, command palette and shortcuts. */

import { DOTL_MIME, DotloomError, downloadBytes, fileKind, pickFile } from '@dotloomjs/sdk'
import type { EditorHandle } from './context.js'
import type { Translate } from './i18n.js'

export interface Action {
  id: string
  label: string
  shortcut?: string
  run(): void | Promise<void>
}

export interface FileHooks {
  /** Current document name (`ev-plani.dotl`). */
  name: () => string
  setName: (n: string) => void
  /** Report an error to the user. */
  error: (title: string, err: unknown) => void
  /** Missing plugin types after loading. */
  missing: (types: string[]) => void
  /** View state to store in `.dotl`. */
  view: () => unknown
  restoreView: (v: unknown) => void
}

function baseName(name: string): string {
  return name.replace(/\.[^.]+$/, '') || 'drawing'
}

export async function openFileAction(h: EditorHandle, t: Translate, hooks: FileHooks): Promise<void> {
  const f = await pickFile('.dotl,.svg,.dxf')
  if (!f) return
  try {
    if (f.kind === 'dotl') {
      const r = await h.engine.load(f.bytes)
      hooks.missing(r.missingPlugins)
      hooks.restoreView(r.view)
      hooks.setName(f.name)
    } else if (f.kind === 'svg' || f.kind === 'dxf') {
      await h.engine.newDocument()
      if (f.kind === 'svg') await h.engine.importSvg(new TextDecoder().decode(f.bytes))
      else await h.engine.importDxf(f.bytes)
      hooks.missing([])
      hooks.setName(`${baseName(f.name)}.dotl`)
      await h.canvas?.fit()
    } else {
      const imp = h.plugins.importerFor(f.name)
      if (!imp) throw new DotloomError({ code: 'file', message: `unsupported file type: ${f.name}` })
      await h.plugins.importFile(f.name, f.bytes)
    }
    await h.autosave?.markSaved()
  } catch (e) {
    hooks.error(t('error.load'), e)
  }
}

export function buildActions(h: EditorHandle, t: Translate, hooks: FileHooks): Action[] {
  const { core, engine } = h
  const save = async (): Promise<void> => {
    const bytes = await engine.save(hooks.view())
    downloadBytes(bytes, hooks.name(), DOTL_MIME)
    await h.autosave?.markSaved()
  }
  const actions: Action[] = [
    {
      id: 'file.new',
      label: t('menu.new'),
      run: async () => {
        await engine.newDocument()
        hooks.setName('untitled.dotl')
        await h.autosave?.markSaved()
      },
    },
    { id: 'file.open', label: t('menu.open'), shortcut: 'Mod+O', run: () => openFileAction(h, t, hooks) },
    { id: 'file.save', label: t('menu.save'), shortcut: 'Mod+S', run: save },
    {
      id: 'file.exportSvg',
      label: t('menu.exportSvg'),
      run: async () => {
        const r = await engine.exportSvg({ background: '#ffffff' })
        downloadBytes(new TextEncoder().encode(r.svg), `${baseName(hooks.name())}.svg`, 'image/svg+xml')
      },
    },
    {
      id: 'file.exportDxf',
      label: t('menu.exportDxf'),
      run: async () => {
        const r = await engine.exportDxf()
        downloadBytes(new TextEncoder().encode(r.dxf), `${baseName(hooks.name())}.dxf`, 'application/dxf')
      },
    },
    { id: 'edit.undo', label: t('menu.undo'), shortcut: 'Mod+Z', run: () => core.undo() },
    { id: 'edit.redo', label: t('menu.redo'), shortcut: 'Mod+Shift+Z', run: () => core.redo() },
    { id: 'edit.delete', label: t('inspector.delete'), shortcut: 'Delete', run: () => core.deleteSelection() },
    { id: 'edit.selectAll', label: t('action.selectAll'), shortcut: 'Mod+A', run: () => core.selectAll() },
    { id: 'edit.group', label: t('inspector.group'), shortcut: 'Mod+G', run: () => core.run('group') },
    { id: 'view.fit', label: t('action.fit'), shortcut: 'Shift+F', run: () => h.viewport.fit() },
    { id: 'view.grid', label: t('status.grid'), shortcut: 'F7', run: () => core.run('toggleGrid') },
    { id: 'view.snap', label: t('status.snap'), shortcut: 'F3', run: () => core.run('toggleSnap') },
  ]
  if (h.canvas) {
    const canvas = h.canvas
    actions.splice(5, 0, {
      id: 'file.exportPng',
      label: t('menu.exportPng'),
      run: async () => {
        const blob = await canvas.exportPng({ grid: false })
        downloadBytes(new Uint8Array(await blob.arrayBuffer()), `${baseName(hooks.name())}.png`, 'image/png')
      },
    })
  }
  for (const tool of core.listTools()) {
    actions.push({
      id: `tool.${tool.id}`,
      label: t('palette.tool', { name: t(tool.label) }),
      ...(tool.shortcut ? { shortcut: tool.shortcut.toUpperCase() } : {}),
      run: () => core.setTool(tool.id),
    })
  }
  for (const p of h.plugins.list()) {
    for (const c of p.commands)
      actions.push({
        id: `plugin.${p.id}.${c}`,
        label: `${p.id}: ${c}`,
        run: async () => void (await h.plugins.run(`${p.id}:${c}`)),
      })
  }
  for (const { plugin, exporter } of h.plugins.exporters()) {
    actions.push({
      id: `export.${plugin}.${exporter.id}`,
      label: t('action.export', { ext: exporter.extension, plugin }),
      run: async () => {
        const bytes = await h.plugins.exportWith(plugin, exporter.id)
        downloadBytes(bytes, `${baseName(hooks.name())}${exporter.extension}`, exporter.mime)
      },
    })
  }
  return actions
}

export { fileKind }
