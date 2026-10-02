/**
 * Dotloom without React. Everything here is plain DOM + the SDK:
 * `createEditor` gives the engine (Web Worker), the wgpu canvas and the tools;
 * the page builds its own toolbar, file buttons and status line.
 */

import { createEditor, DOTL_MIME, DotloomError, downloadBytes, pickFile } from '@dotloom/sdk'

const root = document.getElementById('root') as HTMLDivElement
root.innerHTML = `
  <div style="display:grid;grid-template-rows:auto 1fr auto;height:100%">
    <header id="bar" style="display:flex;gap:4px;padding:6px;border-bottom:1px solid #ccd;flex-wrap:wrap"></header>
    <main id="editor" style="position:relative;min-height:0"></main>
    <footer id="status" role="status" aria-live="polite" style="padding:4px 8px;border-top:1px solid #ccd;font-size:13px"></footer>
  </div>`

const bar = document.getElementById('bar') as HTMLElement
const status = document.getElementById('status') as HTMLElement

function button(label: string, onClick: () => void, extra: Record<string, string> = {}): HTMLButtonElement {
  const b = document.createElement('button')
  b.type = 'button'
  b.textContent = label
  for (const [k, v] of Object.entries(extra)) b.setAttribute(k, v)
  b.addEventListener('click', onClick)
  bar.appendChild(b)
  return b
}

try {
  const editor = await createEditor(document.getElementById('editor') as HTMLElement)
  const { core, engine, viewport } = editor
  const report = (e: unknown): void => {
    status.textContent = e instanceof DotloomError ? `${e.code}: ${e.message}` : String(e)
  }

  // Tools from the registry (built-in tools; plugins would add theirs here too).
  const toolButtons = new Map<string, HTMLButtonElement>()
  for (const tool of core.listTools()) {
    const label = tool.label.replace(/^tool\./, '')
    toolButtons.set(
      tool.id,
      button(label, () => core.setTool(tool.id), { 'aria-pressed': 'false', 'data-tool': tool.id }),
    )
  }
  button('Undo', () => void core.undo())
  button('Redo', () => void core.redo())
  button('Open…', async () => {
    const f = await pickFile('.dotl,.svg,.dxf')
    if (!f) return
    try {
      if (f.kind === 'dotl') await engine.load(f.bytes)
      else if (f.kind === 'svg') {
        await engine.newDocument()
        await engine.importSvg(new TextDecoder().decode(f.bytes))
      } else if (f.kind === 'dxf') {
        await engine.newDocument()
        await engine.importDxf(f.bytes)
      }
      await viewport.fit()
    } catch (e) {
      report(e)
    }
  })
  button('Save', async () => downloadBytes(await engine.save({ camera: viewport.camera }), 'drawing.dotl', DOTL_MIME))
  button('Export SVG', async () => {
    const { svg } = await engine.exportSvg({ background: '#ffffff' })
    downloadBytes(new TextEncoder().encode(svg), 'drawing.svg', 'image/svg+xml')
  })

  // Status line and tool highlighting follow the editor state.
  const render = (): void => {
    const s = core.state.getSnapshot()
    for (const [id, b] of toolButtons) b.setAttribute('aria-pressed', String(id === s.tool))
    const sel = s.selection.length ? ` · ${s.selection.length} selected` : ''
    status.textContent = s.error ? `${s.error.code}: ${s.error.message}` : `${s.prompt || 'Ready'}${sel}`
  }
  core.state.subscribe(render)
  render()
  ;(window as unknown as { dotloom: typeof editor }).dotloom = editor
} catch (e) {
  status.textContent = `Could not start: ${e instanceof Error ? e.message : String(e)}`
}
