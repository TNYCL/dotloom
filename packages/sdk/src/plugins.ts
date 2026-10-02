/**
 * Host-side plugins (DL-PLUGIN).
 *
 * A plugin has two layers:
 *
 * 1. **Model definitions** (`types`): serializable entity types evaluated by the
 *    Rust engine (expressions, anchors, primitives, constraint templates,
 *    migrations). No JavaScript runs inside the solver.
 * 2. **Host code**: trusted TypeScript that turns interaction into commands —
 *    tools, named commands, snap providers, panels, importers/exporters and
 *    storage adapters.
 *
 * Lifecycle: `register` → enabled (default) ⇄ `disable`/`enable` → `unregister`.
 * Disabling keeps documents intact: the engine shows the plugin's entities
 * read-only from their stored fallback until the plugin is enabled again.
 */

import type { EditorCore } from './editor/core.js'
import type { Tool } from './editor/types.js'
import type { DotloomEngine } from './engine.js'
import { DotloomError } from './protocol.js'
import type { StorageAdapter } from './storage.js'
import type { ConstraintSpec, EntityId, EntityTypeDef, Point, Snap, Transaction } from './types.js'
import { SDK_VERSION, satisfies } from './version.js'

/** Context passed to plugin callbacks. */
export interface PluginApi {
  readonly engine: DotloomEngine
  readonly editor: EditorCore | null
  readonly pluginId: string
  /** Subscribe to engine events; removed automatically when the plugin is disabled. */
  on: DotloomEngine['on']
  /** Register a cleanup run on disable/unregister (GPU resources, DOM, timers). */
  onDispose(fn: () => void): void
}

export interface PluginCommand {
  label: string
  /** Build a transaction from arguments (pure host code). */
  run(api: PluginApi, args: unknown): Transaction | Promise<Transaction>
}

export interface SnapCandidate {
  point: Point
  kind?: Snap['kind']
  entity?: EntityId | null
}

/** Extra snap points (e.g. a plugin's guide lines), consulted after the engine. */
export interface SnapProvider {
  id: string
  snap(point: Point, radius: number): SnapCandidate[]
}

/** UI contribution rendered by a host (the React editor mounts them). */
export interface PanelContribution {
  id: string
  title: string
  /** Only shown when the selection contains entities of these types. */
  forTypes?: string[]
  /** Mount into a DOM element; returns an unmount function. */
  mount(el: HTMLElement, api: PluginApi & { selection: () => EntityId[] }): () => void
}

export interface Importer {
  id: string
  /** Lower-case extensions including the dot, e.g. `.csv`. */
  extensions: string[]
  import(bytes: Uint8Array, api: PluginApi): Transaction | Promise<Transaction>
}

export interface Exporter {
  id: string
  extension: string
  mime: string
  export(api: PluginApi): Uint8Array | Promise<Uint8Array>
}

export interface ConstraintTemplate {
  id: string
  label: string
  /** Number of selected entities the template needs. */
  arity: number
  build(ids: EntityId[]): ConstraintSpec[]
}

export interface DotloomPlugin {
  /** Namespaced ID, e.g. `acme.shelves`. */
  id: string
  /** Plugin version (semver). */
  version: string
  /** Semver range of `@dotloom/sdk` the plugin supports, e.g. `^1.0.0`. */
  sdk?: string
  types?: EntityTypeDef[]
  /** Tool factories (fresh instances on every enable). */
  tools?: (() => Tool)[]
  commands?: Record<string, PluginCommand>
  snapProviders?: SnapProvider[]
  panels?: PanelContribution[]
  importers?: Importer[]
  exporters?: Exporter[]
  storage?: StorageAdapter[]
  constraintTemplates?: ConstraintTemplate[]
  /** Called when enabled; may return a cleanup function. */
  activate?(api: PluginApi): (() => void) | undefined
}

export interface PluginInfo {
  id: string
  version: string
  enabled: boolean
  types: string[]
  tools: string[]
  commands: string[]
}

interface Entry {
  plugin: DotloomPlugin
  enabled: boolean
  cleanups: (() => void)[]
  /** IDs of the tools registered while enabled. */
  tools: string[]
}

const ID_RE = /^[a-z][a-z0-9-]*(\.[a-z0-9-]+)+$/

export class PluginHost {
  private readonly entries = new Map<string, Entry>()
  private readonly listeners = new Set<() => void>()

  constructor(
    private readonly engine: DotloomEngine,
    private readonly editor: EditorCore | null = null,
  ) {}

  /** Subscribe to registry changes. */
  subscribe(cb: () => void): () => void {
    this.listeners.add(cb)
    return () => this.listeners.delete(cb)
  }

  private changed(): void {
    for (const cb of [...this.listeners]) cb()
  }

  private api(entry: Entry): PluginApi {
    return {
      engine: this.engine,
      editor: this.editor,
      pluginId: entry.plugin.id,
      on: ((name: never, cb: never) => {
        const off = this.engine.on(name, cb)
        entry.cleanups.push(off)
        return off
      }) as DotloomEngine['on'],
      onDispose: (fn) => entry.cleanups.push(fn),
    }
  }

  /** Register (and by default enable) a plugin. Errors leave nothing registered. */
  async register(plugin: DotloomPlugin, opts: { enabled?: boolean } = {}): Promise<void> {
    if (!ID_RE.test(plugin.id)) {
      throw new DotloomError({
        code: 'plugin',
        message: `invalid plugin id "${plugin.id}" (use a namespace like acme.shelves)`,
      })
    }
    if (this.entries.has(plugin.id)) {
      throw new DotloomError({ code: 'plugin', message: `plugin "${plugin.id}" is already registered` })
    }
    if (plugin.sdk && !satisfies(SDK_VERSION, plugin.sdk)) {
      throw new DotloomError({
        code: 'plugin',
        message: `plugin "${plugin.id}" requires @dotloom/sdk ${plugin.sdk}, this is ${SDK_VERSION}`,
      })
    }
    for (const t of plugin.types ?? []) {
      if (!t.typeId.startsWith(`${plugin.id}.`) && !t.typeId.startsWith(`${plugin.id.split('.')[0]}.`)) {
        throw new DotloomError({
          code: 'plugin',
          message: `type "${t.typeId}" must use the plugin namespace "${plugin.id.split('.')[0]}."`,
        })
      }
    }
    const entry: Entry = { plugin, enabled: false, cleanups: [], tools: [] }
    if (plugin.types?.length) {
      // The engine validates definitions and reports conflicts as typed errors.
      await this.engine.registerTypes(plugin.types, plugin.id)
    }
    this.entries.set(plugin.id, entry)
    if (opts.enabled === false) {
      for (const t of plugin.types ?? []) await this.engine.setTypeEnabled(t.typeId, false)
    } else {
      try {
        await this.activate(entry)
      } catch (e) {
        await this.unregister(plugin.id).catch(() => undefined)
        throw e
      }
    }
    this.changed()
  }

  private async activate(entry: Entry): Promise<void> {
    const p = entry.plugin
    for (const t of p.types ?? []) await this.engine.setTypeEnabled(t.typeId, true)
    if (this.editor) {
      for (const make of p.tools ?? []) {
        try {
          const tool = make()
          entry.cleanups.push(this.editor.registerTool(tool))
          entry.tools.push(tool.id)
        } catch (e) {
          this.runCleanups(entry)
          throw e
        }
      }
      for (const sp of p.snapProviders ?? []) entry.cleanups.push(this.editor.addSnapProvider(sp))
    }
    const cleanup = p.activate?.(this.api(entry))
    if (typeof cleanup === 'function') entry.cleanups.push(cleanup)
    entry.enabled = true
  }

  private runCleanups(entry: Entry): void {
    entry.tools = []
    for (const f of entry.cleanups.splice(0).reverse()) {
      try {
        f()
      } catch {
        // a failing cleanup must not block the others
      }
    }
  }

  /** Disable: tools/commands disappear, the plugin's entities become read-only (data kept). */
  async disable(id: string): Promise<void> {
    const e = this.get(id)
    if (!e.enabled) return
    this.runCleanups(e)
    for (const t of e.plugin.types ?? []) await this.engine.setTypeEnabled(t.typeId, false)
    e.enabled = false
    this.changed()
  }

  async enable(id: string): Promise<void> {
    const e = this.get(id)
    if (e.enabled) return
    await this.activate(e)
    this.changed()
  }

  /** Remove the plugin. Documents keep its entities (read-only until registered again). */
  async unregister(id: string): Promise<void> {
    const e = this.entries.get(id)
    if (!e) return
    this.runCleanups(e)
    for (const t of e.plugin.types ?? []) await this.engine.unregisterType(t.typeId).catch(() => undefined)
    this.entries.delete(id)
    this.changed()
  }

  private get(id: string): Entry {
    const e = this.entries.get(id)
    if (!e) throw new DotloomError({ code: 'notFound', message: `plugin "${id}" is not registered` })
    return e
  }

  list(): PluginInfo[] {
    return [...this.entries.values()].map((e) => ({
      id: e.plugin.id,
      version: e.plugin.version,
      enabled: e.enabled,
      types: (e.plugin.types ?? []).map((t) => t.typeId),
      tools: [...e.tools],
      commands: Object.keys(e.plugin.commands ?? {}),
    }))
  }

  /** Run a named plugin command (`pluginId:command`) as one transaction. */
  async run(qualified: string, args?: unknown): Promise<unknown> {
    const [pid, name] = qualified.split(':')
    const e = this.get(pid ?? '')
    if (!e.enabled) throw new DotloomError({ code: 'plugin', message: `plugin "${pid}" is disabled` })
    const cmd = name ? e.plugin.commands?.[name] : undefined
    if (!cmd) throw new DotloomError({ code: 'notFound', message: `unknown command "${qualified}"` })
    const tx = await cmd.run(this.api(e), args)
    return this.editor ? this.editor.apply(tx) : this.engine.apply(tx)
  }

  /** Importer for a file name (enabled plugins only). */
  importerFor(fileName: string): { plugin: string; importer: Importer } | null {
    const lower = fileName.toLowerCase()
    for (const e of this.entries.values()) {
      if (!e.enabled) continue
      for (const imp of e.plugin.importers ?? []) {
        if (imp.extensions.some((x) => lower.endsWith(x))) return { plugin: e.plugin.id, importer: imp }
      }
    }
    return null
  }

  /** Run an importer as one undoable transaction. */
  async importFile(fileName: string, bytes: Uint8Array): Promise<unknown> {
    const found = this.importerFor(fileName)
    if (!found) throw new DotloomError({ code: 'import', message: `no importer for ${fileName}` })
    const e = this.get(found.plugin)
    const tx = await found.importer.import(bytes, this.api(e))
    return this.editor ? this.editor.apply(tx) : this.engine.apply(tx)
  }

  exporters(): { plugin: string; exporter: Exporter }[] {
    return [...this.entries.values()]
      .filter((e) => e.enabled)
      .flatMap((e) => (e.plugin.exporters ?? []).map((exporter) => ({ plugin: e.plugin.id, exporter })))
  }

  async exportWith(plugin: string, exporterId: string): Promise<Uint8Array> {
    const e = this.get(plugin)
    const ex = e.plugin.exporters?.find((x) => x.id === exporterId)
    if (!ex || !e.enabled)
      throw new DotloomError({ code: 'notFound', message: `unknown exporter ${plugin}:${exporterId}` })
    return ex.export(this.api(e))
  }

  panels(): { plugin: string; panel: PanelContribution }[] {
    return [...this.entries.values()]
      .filter((e) => e.enabled)
      .flatMap((e) => (e.plugin.panels ?? []).map((panel) => ({ plugin: e.plugin.id, panel })))
  }

  constraintTemplates(): { plugin: string; template: ConstraintTemplate }[] {
    return [...this.entries.values()]
      .filter((e) => e.enabled)
      .flatMap((e) => (e.plugin.constraintTemplates ?? []).map((template) => ({ plugin: e.plugin.id, template })))
  }

  /** Storage adapters contributed by enabled plugins, in registration order. */
  storageAdapters(): StorageAdapter[] {
    return [...this.entries.values()].filter((e) => e.enabled).flatMap((e) => e.plugin.storage ?? [])
  }

  /**
   * Mount a plugin panel with an API that tracks the selection. The returned
   * unmount function is idempotent; disabling or unregistering the plugin also
   * unmounts it.
   */
  mountPanel(plugin: string, panelId: string, el: HTMLElement): () => void {
    const e = this.get(plugin)
    if (!e.enabled) throw new DotloomError({ code: 'plugin', message: `plugin "${plugin}" is disabled` })
    const panel = e.plugin.panels?.find((p) => p.id === panelId)
    if (!panel) throw new DotloomError({ code: 'notFound', message: `unknown panel ${plugin}:${panelId}` })
    const unmount = panel.mount(el, {
      ...this.api(e),
      selection: () => this.editor?.state.getSnapshot().selection ?? [],
    })
    let done = false
    const off = (): void => {
      if (done) return
      done = true
      const i = e.cleanups.indexOf(off)
      if (i >= 0) e.cleanups.splice(i, 1)
      unmount()
    }
    e.cleanups.push(off)
    return off
  }

  /** Unregister everything. */
  async dispose(): Promise<void> {
    for (const id of [...this.entries.keys()]) await this.unregister(id)
    this.listeners.clear()
  }
}
