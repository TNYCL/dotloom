/**
 * E2E harness: a real editor built from the packaged @dotloom/sdk (dist).
 *
 * URL parameters:
 *   backend=webgpu|webgl2   use exactly this backend (no fallback)
 *   grid=1                  show the grid
 */

import {
  type Backend,
  type Command,
  createEditor,
  type DotloomEditor,
  DotloomEngine,
  type FrameStats,
} from '@dotloom/sdk'

interface Harness {
  ready: Promise<{ ok: boolean; backend?: string; info?: unknown; attempts?: unknown; error?: string }>
  editor: DotloomEditor | null
  frames: number
  events: string[]
  nextFrame(): Promise<FrameStats>
  apply(commands: Command[]): Promise<unknown>
  /** SDK exports for tests that need a second engine. */
  sdk: { DotloomEngine: typeof DotloomEngine }
}

declare global {
  interface Window {
    dl: Harness
  }
}

const params = new URLSearchParams(location.search)
const backend = params.get('backend') as Backend | null
const log = document.getElementById('log') as HTMLPreElement
const waiters: ((s: FrameStats) => void)[] = []

const h: Harness = {
  ready: Promise.resolve({ ok: false }),
  editor: null,
  frames: 0,
  events: [],
  nextFrame(): Promise<FrameStats> {
    const p = new Promise<FrameStats>((r) => waiters.push(r))
    h.editor?.viewport.requestRender()
    return p
  },
  async apply(commands: Command[]): Promise<unknown> {
    if (!h.editor) throw new Error('not ready')
    return h.editor.engine.apply(commands)
  },
  sdk: { DotloomEngine },
}
window.dl = h

h.ready = (async () => {
  const stage = document.getElementById('stage') as HTMLDivElement
  try {
    const editor = await createEditor(stage, {
      viewport: {
        ...(backend ? { backends: [backend] } : {}),
        grid: { visible: params.get('grid') === '1' },
        camera: { center: [0, 0], scale: 1 },
        msaa: params.get('msaa') !== '0',
      },
    })
    h.editor = editor
    editor.viewport.on('frame', (s) => {
      h.frames += 1
      for (const w of waiters.splice(0)) w(s)
    })
    for (const ev of ['lost', 'restored', 'error'] as const) {
      editor.viewport.on(ev, (v) => {
        h.events.push(`${ev}:${JSON.stringify(v)}`)
        log.textContent = h.events.join('\n')
      })
    }
    return {
      ok: true,
      backend: editor.viewport.backend ?? '',
      info: editor.viewport.info,
      attempts: editor.viewport.attempts,
    }
  } catch (e) {
    const err = e as { message?: string; details?: unknown }
    log.textContent = String(err.message)
    return { ok: false, error: String(err.message), attempts: (err.details as { attempts?: unknown })?.attempts }
  }
})()
