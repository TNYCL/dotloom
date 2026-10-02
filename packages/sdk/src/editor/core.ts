/**
 * `EditorCore`: framework- and DOM-independent editor logic.
 *
 * Pointer and keyboard input arrive as plain objects (the DOM binder in `dom.ts`
 * produces them). Events are processed strictly in order; pointer moves are
 * coalesced (only the newest pending move is processed) and snap queries are
 * issued for the processed position only, so a slow solve never builds a backlog.
 */

import { pxToWorld as camPxToWorld, wheelZoomFactor } from '../camera.js'
import type { DotloomEngine } from '../engine.js'
import type { SnapProvider } from '../plugins.js'
import { DotloomError } from '../protocol.js'
import type { Clipboard, CommitReport, EntityId, Point, Snap, SnapOptions, Transaction } from '../types.js'
import type { MarkerKind, OverlayState, ViewportLike } from '../viewport.js'
import { comboOf, compileShortcuts, DEFAULT_SHORTCUTS, type ShortcutAction, type ShortcutMap } from './shortcuts.js'
import { Store } from './store.js'
import type {
  CancelReason,
  EditorState,
  InputKey,
  InputPointer,
  PointerState,
  Tool,
  ToolContext,
  ToolPointer,
} from './types.js'

export interface EditorCoreOptions {
  /** Snap search radius in CSS pixels. */
  snapRadiusPx?: number
  snap?: SnapOptions
  snapEnabled?: boolean
  shortcuts?: Partial<ShortcutMap>
  /** Initial tool id (default `select`). */
  tool?: string
  /** Model units per display unit for typed values (default 1 = millimetres). */
  unitScale?: number
}

export const DEFAULT_SNAP: SnapOptions = {
  endpoint: true,
  midpoint: true,
  center: true,
  quadrant: true,
  intersection: true,
  anchor: true,
  nearest: true,
  grid: true,
}

const MARKER_OF: Record<string, MarkerKind> = {
  endpoint: 'endpoint',
  intersection: 'intersection',
  center: 'center',
  midpoint: 'midpoint',
  quadrant: 'anchor',
  anchor: 'anchor',
  nearest: 'nearest',
  grid: 'grid',
}

type Task = () => Promise<void>

export class EditorCore {
  readonly engine: DotloomEngine
  readonly viewport: ViewportLike
  readonly state: Store<EditorState>
  readonly pointer: Store<PointerState>
  private readonly tools = new Map<string, Tool>()
  private active: Tool | null = null
  private ctx: ToolContext
  private queue: Task[] = []
  private running = false
  private pendingMove: InputPointer | null = null
  private lastSnap: Snap | null = null
  private snapExclude: EntityId[] = []
  private toolOverlay: OverlayState = {}
  private extraMarkers: OverlayState['markers'] = []
  private shortcutTable: Map<string, ShortcutAction>
  private clipboard: Clipboard | null = null
  private pasteCount = 0
  private readonly snapRadiusPx: number
  private unitScale: number
  private disposed = false
  private readonly unsubscribe: (() => void)[] = []
  private buttonsDown = 0
  private readonly snapProviders = new Set<SnapProvider>()

  constructor(engine: DotloomEngine, viewport: ViewportLike, options: EditorCoreOptions = {}) {
    this.engine = engine
    this.viewport = viewport
    this.snapRadiusPx = options.snapRadiusPx ?? 10
    this.unitScale = options.unitScale ?? 1
    this.shortcutTable = compileShortcuts({ ...DEFAULT_SHORTCUTS, ...options.shortcuts })
    this.state = new Store<EditorState>({
      tool: '',
      toolState: 'idle',
      prompt: '',
      selection: [],
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
      revision: engine.revision,
      snapEnabled: options.snapEnabled ?? true,
      snapOptions: { ...DEFAULT_SNAP, ...options.snap },
      input: '',
      busy: false,
      error: null,
      lastCommit: null,
      activeLayer: null,
      toolsVersion: 0,
      gridVisible: viewport.grid.visible,
    })
    this.pointer = new Store<PointerState>({ world: null, snap: null })
    this.ctx = this.makeContext()
    this.unsubscribe.push(
      engine.on('selectionChanged', (e) => this.state.set({ selection: e.selection })),
      engine.on('historyChanged', (e) =>
        this.state.set({ canUndo: e.canUndo, canRedo: e.canRedo, undoLabel: e.undoLabel, redoLabel: e.redoLabel }),
      ),
      engine.on('committed', (e) => this.state.set({ revision: e.revision })),
    )
    this.pendingTool = options.tool ?? 'select'
  }

  private pendingTool: string

  /** Activate the initial tool once built-in tools are registered. */
  start(): void {
    if (!this.active) this.setTool(this.pendingTool)
  }

  // --- tool registry -------------------------------------------------------

  /** Register a tool; returns an unregister function. Duplicate IDs throw. */
  registerTool(tool: Tool): () => void {
    if (this.tools.has(tool.id)) {
      throw new DotloomError({ code: 'plugin', message: `tool "${tool.id}" is already registered` })
    }
    this.tools.set(tool.id, tool)
    this.bumpTools()
    return () => {
      if (this.tools.get(tool.id) !== tool) return
      if (this.active === tool) this.setTool('select')
      this.tools.delete(tool.id)
      this.bumpTools()
    }
  }

  private bumpTools(): void {
    this.state.set({ toolsVersion: this.state.getSnapshot().toolsVersion + 1 })
  }

  /** Registered tools. */
  listTools(): Tool[] {
    return [...this.tools.values()]
  }

  get tool(): Tool | null {
    return this.active
  }

  /** Switch tools (the current operation is cancelled). */
  setTool(id: string): void {
    const next = this.tools.get(id)
    if (!next || next === this.active) return
    const prev = this.active
    this.enqueue(async () => {
      if (prev) {
        await prev.cancel?.(this.ctx, 'tool-change')
        prev.deactivate?.(this.ctx)
      }
      this.toolOverlay = {}
      this.extraMarkers = []
      this.snapExclude = []
      this.active = next
      this.state.set({ tool: next.id, toolState: 'idle', prompt: '', input: '' })
      next.activate?.(this.ctx)
      this.renderOverlay()
    })
  }

  // --- context for tools ---------------------------------------------------

  private makeContext(): ToolContext {
    return {
      engine: this.engine,
      viewport: this.viewport,
      setOverlay: (o) => {
        this.toolOverlay = o
        this.renderOverlay()
      },
      setPrompt: (key) => this.state.set({ prompt: key }),
      setState: (s) => this.state.set({ toolState: s }),
      apply: (tx) => this.apply(tx),
      selection: () => this.state.getSnapshot().selection,
      setSelection: (ids) => this.engine.setSelection(ids),
      pxToWorld: (px) => camPxToWorld(this.viewport.camera, px),
      setSnapExclude: (ids) => {
        this.snapExclude = ids
      },
      report: (err) => this.report(err),
      unitScale: () => this.unitScale,
      setMarkers: (m) => this.setMarkers(m),
    }
  }

  /** Model units per typed display unit (e.g. 1000 for metres). */
  setUnitScale(scale: number): void {
    if (Number.isFinite(scale) && scale > 0) this.unitScale = scale
  }

  get typedUnitScale(): number {
    return this.unitScale
  }

  /** Extra overlay markers (e.g. selection grips from the select tool). */
  setMarkers(markers: OverlayState['markers']): void {
    this.extraMarkers = markers ?? []
    this.renderOverlay()
  }

  private renderOverlay(): void {
    const markers = [...(this.extraMarkers ?? []), ...(this.toolOverlay.markers ?? [])]
    const s = this.pointer.getSnapshot().snap
    if (s) markers.push({ at: s.point, kind: MARKER_OF[s.kind] ?? 'anchor' })
    this.viewport.setOverlay({ ...this.toolOverlay, markers })
  }

  /** Report an error in the editor state. */
  report(err: unknown): void {
    if (err instanceof DotloomError) {
      const d = err.details as { failure?: { diagnostics?: unknown } } | undefined
      const diagnostics = Array.isArray(d?.failure?.diagnostics) ? (d?.failure?.diagnostics as never[]) : []
      this.state.set({ error: { code: err.code, message: err.message, diagnostics } })
    } else {
      this.state.set({
        error: { code: 'internal', message: err instanceof Error ? err.message : String(err), diagnostics: [] },
      })
    }
  }

  /** Clear the reported error. */
  clearError(): void {
    this.state.set({ error: null })
  }

  private pending: AbortController | null = null

  /**
   * Apply a transaction, tracking busy/error state. Returns `null` on failure.
   * New entities without a layer go to the active layer.
   */
  async apply(tx: Transaction): Promise<CommitReport | null> {
    try {
      const r = await this.applyOrThrow(tx)
      this.state.set({ error: null })
      return r
    } catch (e) {
      this.report(e)
      return null
    }
  }

  /**
   * Like `apply`, but failures are thrown to the caller (e.g. a form field that
   * shows the error next to itself) instead of being reported in the state.
   */
  async applyOrThrow(tx: Transaction): Promise<CommitReport> {
    const layer = this.state.getSnapshot().activeLayer
    const t: Transaction =
      layer === null
        ? tx
        : {
            ...tx,
            commands: tx.commands.map((c) =>
              c.op === 'createEntity' && c.entity.layer === undefined ? { ...c, entity: { ...c.entity, layer } } : c,
            ),
          }
    const ctrl = new AbortController()
    this.pending = ctrl
    this.state.set({ busy: true })
    try {
      const r = await this.engine.apply(t, { signal: ctrl.signal })
      this.state.set({ lastCommit: r })
      return r
    } finally {
      if (this.pending === ctrl) this.pending = null
      this.state.set({ busy: this.pending !== null })
    }
  }

  /** Cancel the running solve (the document stays unchanged). */
  cancelSolve(): void {
    this.pending?.abort()
  }

  /** Show or hide the grid (kept in the state for UIs). */
  setGridVisible(visible: boolean): void {
    this.viewport.setGrid({ visible })
    this.state.set({ gridVisible: visible })
  }

  /** Layer for new entities (`null` = the engine's default layer). */
  setActiveLayer(layer: number | null): void {
    this.state.set({ activeLayer: layer })
  }

  // --- event queue ---------------------------------------------------------

  private enqueue(task: Task): Promise<void> {
    return new Promise<void>((resolve) => {
      this.queue.push(async () => {
        try {
          await task()
        } catch (e) {
          this.report(e)
        } finally {
          resolve()
        }
      })
      void this.pump()
    })
  }

  private async pump(): Promise<void> {
    if (this.running) return
    this.running = true
    try {
      while (this.queue.length > 0 && !this.disposed) {
        const t = this.queue.shift()
        if (t) await t()
      }
    } finally {
      this.running = false
    }
  }

  /** Resolves when all queued input has been processed (tests, scripted input). */
  idle(): Promise<void> {
    return this.enqueue(async () => undefined)
  }

  private async toolPointer(p: InputPointer, snap: boolean): Promise<ToolPointer> {
    const world = this.viewport.screenToWorld(p.x, p.y)
    let s: Snap | null = null
    const st = this.state.getSnapshot()
    const wantsSnap = snap && st.snapEnabled && (this.active?.snaps ?? true) && !p.alt
    if (wantsSnap) {
      try {
        s = await this.engine.snap({
          point: world,
          radius: camPxToWorld(this.viewport.camera, this.snapRadiusPx),
          options: { ...st.snapOptions, gridSpacing: this.viewport.grid.spacing },
          exclude: this.snapExclude,
          previous: this.lastSnap,
        })
      } catch {
        s = null
      }
      s = this.providerSnap(world, s)
    }
    this.lastSnap = s
    this.pointer.set({ world, snap: s })
    this.renderOverlay()
    return {
      screen: [p.x, p.y],
      world,
      point: s ? s.point : world,
      snap: s,
      button: p.button,
      shift: p.shift,
      mod: p.mod,
      alt: p.alt,
    }
  }

  /** Add a snap provider (plugins); returns a remover. */
  addSnapProvider(p: SnapProvider): () => void {
    this.snapProviders.add(p)
    return () => {
      this.snapProviders.delete(p)
    }
  }

  /** Engine snaps win unless a provider candidate is strictly closer. */
  private providerSnap(world: Point, engineSnap: Snap | null): Snap | null {
    if (this.snapProviders.size === 0) return engineSnap
    const radius = camPxToWorld(this.viewport.camera, this.snapRadiusPx)
    let best = engineSnap
    let bd = best ? Math.hypot(best.point[0] - world[0], best.point[1] - world[1]) : Number.POSITIVE_INFINITY
    for (const p of this.snapProviders) {
      let cands: ReturnType<SnapProvider['snap']> = []
      try {
        cands = p.snap(world, radius)
      } catch (e) {
        this.report(e)
      }
      for (const c of cands) {
        const d = Math.hypot(c.point[0] - world[0], c.point[1] - world[1])
        if (Number.isFinite(d) && d <= radius && d < bd) {
          bd = d
          best = { point: c.point, kind: c.kind ?? 'anchor', entity: c.entity ?? null, anchor: null, other: null }
        }
      }
    }
    return best
  }

  // --- input entry points --------------------------------------------------

  pointerDown(p: InputPointer): Promise<void> {
    this.buttonsDown = p.buttons
    this.pendingMove = null
    return this.enqueue(async () => {
      const tp = await this.toolPointer(p, true)
      await this.active?.pointerDown?.(this.ctx, tp)
    })
  }

  pointerMove(p: InputPointer): Promise<void> {
    if (this.pendingMove) {
      // A move is already waiting: replace it with the newer position.
      this.pendingMove = p
      return this.idle()
    }
    this.pendingMove = p
    return this.enqueue(async () => {
      const latest = this.pendingMove
      this.pendingMove = null
      if (!latest) return
      const tp = await this.toolPointer(latest, true)
      await this.active?.pointerMove?.(this.ctx, tp)
    })
  }

  pointerUp(p: InputPointer): Promise<void> {
    this.buttonsDown = p.buttons
    const move = this.pendingMove
    this.pendingMove = null
    return this.enqueue(async () => {
      if (move) {
        const mp = await this.toolPointer(move, true)
        await this.active?.pointerMove?.(this.ctx, mp)
      }
      const tp = await this.toolPointer(p, true)
      await this.active?.pointerUp?.(this.ctx, tp)
    })
  }

  doubleClick(p: InputPointer): Promise<void> {
    return this.enqueue(async () => {
      const tp = await this.toolPointer(p, true)
      await this.active?.doubleClick?.(this.ctx, tp)
    })
  }

  /** Pointer capture lost, window blurred, … — abort any in-progress gesture. */
  cancel(reason: CancelReason): Promise<void> {
    this.pendingMove = null
    this.buttonsDown = 0
    return this.enqueue(async () => {
      await this.active?.cancel?.(this.ctx, reason)
      this.state.set({ input: '' })
    })
  }

  /** Pointer left the canvas: clear hover and snap feedback. */
  pointerLeave(): void {
    this.pendingMove = null
    this.lastSnap = null
    this.pointer.set({ world: null, snap: null })
    this.viewport.setHover(null)
    this.renderOverlay()
  }

  /** Whether a pointer button is held (for capture-loss handling). */
  get pressed(): boolean {
    return this.buttonsDown !== 0
  }

  /** Handle a key from a focused editor. Returns true if it was consumed. */
  keyDown(k: InputKey): boolean {
    const tool = this.active
    const st = this.state.getSnapshot()
    if (tool?.capturesKeyboard?.()) {
      void this.enqueue(async () => {
        await tool.key?.(this.ctx, k)
      })
      return true
    }
    // Typed values for tools that accept them.
    if (tool?.value && !k.mod && !k.alt) {
      if (/^[0-9.,\-\s]$/.test(k.key) && (st.input.length > 0 || k.key !== ' ')) {
        this.state.set({ input: st.input + k.key })
        return true
      }
      if (st.input.length > 0 && k.key === 'Backspace') {
        this.state.set({ input: st.input.slice(0, -1) })
        return true
      }
      if (st.input.length > 0 && k.key === 'Enter') {
        const text = st.input
        this.state.set({ input: '' })
        void this.enqueue(async () => {
          const ok = await tool.value?.(this.ctx, text)
          if (!ok) this.state.set({ error: { code: 'input', message: `invalid value "${text}"`, diagnostics: [] } })
        })
        return true
      }
      if (st.input.length > 0 && k.key === 'Escape') {
        this.state.set({ input: '' })
        return true
      }
    }
    const combo = comboOf(k)
    const action = this.shortcutTable.get(combo)
    // Tool-specific keys first for Enter/Backspace/Escape-like keys.
    if (tool?.key && (k.key === 'Enter' || k.key === 'Backspace' || k.key === 'Escape' || k.key === 'Tab')) {
      let handled = false
      const p = this.enqueue(async () => {
        handled = (await tool.key?.(this.ctx, k)) ?? false
        if (!handled && action) await this.run(action, k)
      })
      void p
      return true
    }
    if (action) {
      void this.enqueue(() => this.run(action, k))
      return true
    }
    if (!k.mod && !k.alt && k.key.length === 1) {
      const t = this.listTools().find((x) => x.shortcut === k.key.toLowerCase())
      if (t) {
        this.setTool(t.id)
        return true
      }
    }
    if (tool?.key) {
      void this.enqueue(async () => {
        await tool.key?.(this.ctx, k)
      })
    }
    return false
  }

  /** Run a shortcut action. */
  async run(action: ShortcutAction, k?: InputKey): Promise<void> {
    const sel = this.state.getSnapshot().selection
    const step = (k?.shift ? 10 : 1) * this.viewport.grid.spacing
    switch (action) {
      case 'undo':
        await this.undo()
        return
      case 'redo':
        await this.redo()
        return
      case 'delete':
        await this.deleteSelection()
        return
      case 'copy':
        await this.copy()
        return
      case 'cut':
        await this.copy()
        await this.deleteSelection()
        return
      case 'paste':
        await this.paste()
        return
      case 'selectAll':
        await this.selectAll()
        return
      case 'cancel':
        if (this.active) await this.active.cancel?.(this.ctx, 'escape')
        if (this.state.getSnapshot().toolState === 'idle' && sel.length > 0) await this.engine.setSelection([])
        return
      case 'fit':
        await this.viewport.fit()
        return
      case 'toggleGrid':
        this.setGridVisible(!this.viewport.grid.visible)
        return
      case 'toggleSnap':
        this.state.set({ snapEnabled: !this.state.getSnapshot().snapEnabled })
        return
      case 'group':
        if (sel.length > 1) await this.apply({ label: 'Group', commands: [{ op: 'group', members: sel }] })
        return
      case 'ungroup':
        await this.ungroupSelection()
        return
      case 'nudgeLeft':
        await this.nudge(-step, 0)
        return
      case 'nudgeRight':
        await this.nudge(step, 0)
        return
      case 'nudgeUp':
        await this.nudge(0, step)
        return
      case 'nudgeDown':
        await this.nudge(0, -step)
        return
      case 'zoomIn':
        this.zoomBy(1.25)
        return
      case 'zoomOut':
        this.zoomBy(0.8)
        return
    }
  }

  private zoomBy(f: number): void {
    const v = this.viewport
    const c = v.camera
    v.setCamera({ center: c.center, scale: c.scale * f })
  }

  /** Wheel zoom at a CSS position. */
  wheel(x: number, y: number, deltaY: number, deltaMode: number): void {
    const v = this.viewport
    const f = wheelZoomFactor(deltaY, deltaMode)
    const anchor = v.screenToWorld(x, y)
    const scale = v.camera.scale * f
    const moved: Point = [
      v.camera.center[0] + (x - v.size.width / 2) / scale,
      v.camera.center[1] - (y - v.size.height / 2) / scale,
    ]
    v.setCamera({
      center: [v.camera.center[0] + anchor[0] - moved[0], v.camera.center[1] + anchor[1] - moved[1]],
      scale,
    })
  }

  // --- commands --------------------------------------------------------------

  async undo(): Promise<void> {
    try {
      await this.engine.undo()
    } catch (e) {
      if (!(e instanceof DotloomError && e.code === 'nothingToUndo')) this.report(e)
    }
  }

  async redo(): Promise<void> {
    try {
      await this.engine.redo()
    } catch (e) {
      if (!(e instanceof DotloomError && e.code === 'nothingToRedo')) this.report(e)
    }
  }

  async deleteSelection(): Promise<void> {
    const sel = this.state.getSnapshot().selection
    if (sel.length === 0) return
    await this.apply({ label: 'Delete', commands: [{ op: 'delete', ids: sel, policy: 'cascade' }] })
  }

  async selectAll(): Promise<void> {
    const doc = await this.engine.documentJson()
    await this.engine.setSelection(doc.entities.filter((e) => !e.hidden).map((e) => e.id))
  }

  async copy(): Promise<void> {
    const sel = this.state.getSnapshot().selection
    if (sel.length === 0) return
    this.clipboard = await this.engine.copy(sel)
    this.pasteCount = 0
    // Best effort: share with other tabs/apps as JSON text.
    const nav = (globalThis as { navigator?: { clipboard?: { writeText?: (s: string) => Promise<void> } } }).navigator
    try {
      await nav?.clipboard?.writeText?.(JSON.stringify({ dotloomClipboard: 1, ...this.clipboard }))
    } catch {
      // permissions may deny clipboard access; the in-memory clipboard still works
    }
  }

  /** Whether something can be pasted. */
  get hasClipboard(): boolean {
    return this.clipboard !== null
  }

  async paste(): Promise<void> {
    if (!this.clipboard) return
    this.pasteCount += 1
    const d = this.viewport.grid.spacing * this.pasteCount
    const r = await this.apply({
      label: 'Paste',
      commands: [{ op: 'paste', clipboard: this.clipboard, offset: [d, -d] }],
    })
    if (r) await this.engine.setSelection(r.created)
  }

  private async nudge(dx: number, dy: number): Promise<void> {
    const sel = this.state.getSnapshot().selection
    if (sel.length === 0) return
    await this.apply({
      label: 'Move',
      commands: [{ op: 'transform', ids: sel, transform: [1, 0, 0, 1, dx, dy], policy: 'convert' }],
    })
  }

  private async ungroupSelection(): Promise<void> {
    const sel = new Set(this.state.getSnapshot().selection)
    if (sel.size === 0) return
    const doc = await this.engine.documentJson()
    const groups = doc.groups.filter((g) => g.members.some((m) => sel.has(m)))
    if (groups.length === 0) return
    await this.apply({ label: 'Ungroup', commands: groups.map((g) => ({ op: 'ungroup' as const, id: g.id })) })
  }

  /** Stop processing input and remove listeners. */
  dispose(): void {
    if (this.disposed) return
    void this.active?.cancel?.(this.ctx, 'dispose')
    this.active?.deactivate?.(this.ctx)
    this.disposed = true
    this.queue = []
    for (const u of this.unsubscribe.splice(0)) u()
  }
}
