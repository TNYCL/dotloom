/**
 * `createEditor`: engine (Worker) + viewport (wgpu) + tools + DOM input in one
 * call. Every part is also usable on its own.
 */

import { DotloomEngine, type EngineOptions } from '../engine.js'
import { Viewport, type ViewportOptions } from '../viewport.js'
import { EditorCore, type EditorCoreOptions } from './core.js'
import { bindDom, type DomBindingOptions } from './dom.js'
import { builtinTools } from './tools/index.js'

export { DEFAULT_SNAP, EditorCore, type EditorCoreOptions } from './core.js'
export { bindDom, type DomBindingOptions, isEditableTarget } from './dom.js'
export * as geom from './geom.js'
export { NullViewport } from './null-viewport.js'
export { comboOf, compileShortcuts, DEFAULT_SHORTCUTS, type ShortcutAction, type ShortcutMap } from './shortcuts.js'
export { Store } from './store.js'
export * from './tools/index.js'
export type * from './types.js'

export interface EditorOptions {
  engine?: EngineOptions | DotloomEngine
  viewport?: ViewportOptions
  core?: EditorCoreOptions
  dom?: DomBindingOptions
  /** Register the built-in tools (default true). */
  builtinTools?: boolean
}

export interface DotloomEditor {
  readonly engine: DotloomEngine
  readonly viewport: Viewport
  readonly core: EditorCore
  /** Dispose input bindings, viewport and (if created here) the engine. */
  dispose(): void
}

/** Create a complete editor inside `container` (which must have a size). */
export async function createEditor(container: HTMLElement, opts: EditorOptions = {}): Promise<DotloomEditor> {
  const ownEngine = !(opts.engine instanceof DotloomEngine)
  const engine = opts.engine instanceof DotloomEngine ? opts.engine : await DotloomEngine.create(opts.engine ?? {})
  let viewport: Viewport
  try {
    viewport = await Viewport.create(container, engine, opts.viewport)
  } catch (e) {
    if (ownEngine) engine.dispose()
    throw e
  }
  const core = new EditorCore(engine, viewport, opts.core)
  if (opts.builtinTools !== false) for (const t of builtinTools()) core.registerTool(t)
  core.start()
  const unbind = bindDom(core, viewport, opts.dom)
  let disposed = false
  return {
    engine,
    viewport,
    core,
    dispose(): void {
      if (disposed) return
      disposed = true
      unbind()
      core.dispose()
      viewport.dispose()
      if (ownEngine) engine.dispose()
    },
  }
}
