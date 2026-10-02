/**
 * `DotloomEngine`: the asynchronous client of a Dotloom engine.
 *
 * The engine (Rust/WASM) owns the only editable copy of the document. This class
 * sends versioned requests, tracks revisions and forwards scene deltas and events.
 * It never exposes engine internals; every edit is a transaction of commands.
 */

import { DotloomError, type ErrorPayload, type HostMessage, type MethodName, PROTOCOL_VERSION } from './protocol.js'
import { type Transport, WorkerTransport } from './transport.js'
import type {
  Capabilities,
  Clipboard,
  Command,
  CommitReport,
  ConversionReport,
  DiagnosticReport,
  DocumentJson,
  DragPreview,
  DragSpec,
  EngineEvent,
  EntityId,
  EntityInfo,
  EntityTypeDef,
  Hit,
  LoadReport,
  Point,
  Snap,
  SnapQuery,
  SolveStatus,
  Transaction,
} from './types.js'

export interface CallOptions {
  /** Reject unless the document is at this revision (stale-request protection). */
  expectedRevision?: number
  /** Cancels the request (a running solve is cancelled between solver steps). */
  signal?: AbortSignal
}

export interface EngineOptions {
  /** `'worker'` (default in browsers), `'inline'` (default in Node), or a transport. */
  transport?: 'worker' | Transport
  /** Override the worker script URL (bundlers normally resolve it). */
  workerUrl?: string | URL
  /** Override the engine `.wasm` URL (e.g. served from a CDN or a sub-path). */
  engineWasmUrl?: string | URL
  /** Milliseconds before an unanswered request rejects with `timeout` (0 = never). */
  requestTimeoutMs?: number
}

export interface SceneMessage {
  delta: ArrayBuffer
  revision: number
  preview: boolean
}

type EventMap = {
  scene: SceneMessage
  committed: Extract<EngineEvent, { type: 'committed' }>
  selectionChanged: Extract<EngineEvent, { type: 'selectionChanged' }>
  historyChanged: Extract<EngineEvent, { type: 'historyChanged' }>
  pluginsChanged: Extract<EngineEvent, { type: 'pluginsChanged' }>
  crash: { message: string }
  /** A long solve is running (`iterations` so far); useful for progress UI. */
  progress: { iterations: number }
}

export type EngineEventName = keyof EventMap
type Listener<K extends EngineEventName> = (ev: EventMap[K]) => void

interface PendingCall {
  method: MethodName | 'init' | 'dispose'
  resolve: (v: unknown) => void
  reject: (e: unknown) => void
  timer?: ReturnType<typeof setTimeout>
}

/** Default URL of the engine wasm next to this module (resolved lazily). */
export function defaultEngineWasmUrl(): URL {
  return new URL('./wasm/engine/dotloom_wasm_bg.wasm', import.meta.url)
}

export class DotloomEngine {
  private readonly transport: Transport
  private readonly pending = new Map<number, PendingCall>()
  private readonly listeners = new Map<EngineEventName, Set<Listener<EngineEventName>>>()
  private nextId = 1
  private timeoutMs: number
  private disposed = false
  private crashed: string | null = null
  /** Buffers created by the SDK (safe to transfer to the worker). */
  private readonly owned = new WeakSet<ArrayBuffer>()
  /** Latest committed revision reported by the engine. */
  revision = 0
  /** Versions and capabilities reported at initialization. */
  capabilities: Capabilities | null = null

  private constructor(transport: Transport, timeoutMs: number) {
    this.transport = transport
    this.timeoutMs = timeoutMs
    transport.onMessage((m) => this.onMessage(m))
    transport.onCrash((reason) => this.onCrash(reason))
  }

  /** Create and initialize an engine. In browsers the engine runs in a Web Worker. */
  static async create(options: EngineOptions = {}): Promise<DotloomEngine> {
    const transport = typeof options.transport === 'object' ? options.transport : new WorkerTransport(options.workerUrl)
    const engine = new DotloomEngine(transport, options.requestTimeoutMs ?? 0)
    const wasm = options.engineWasmUrl ?? (typeof options.transport === 'object' ? undefined : defaultEngineWasmUrl())
    engine.capabilities = (await engine.request('init', [], { engineWasm: wasm?.toString() })) as Capabilities
    if (engine.capabilities.protocol !== PROTOCOL_VERSION) {
      engine.dispose()
      throw new DotloomError({
        code: 'protocol',
        message: `engine protocol ${engine.capabilities.protocol} != SDK protocol ${PROTOCOL_VERSION}`,
      })
    }
    return engine
  }

  /** Subscribe to an event; returns an unsubscribe function. */
  on<K extends EngineEventName>(name: K, cb: Listener<K>): () => void {
    let set = this.listeners.get(name)
    if (!set) {
      set = new Set()
      this.listeners.set(name, set)
    }
    set.add(cb as Listener<EngineEventName>)
    return () => set?.delete(cb as Listener<EngineEventName>)
  }

  private emit<K extends EngineEventName>(name: K, ev: EventMap[K]): void {
    const set = this.listeners.get(name)
    if (!set) return
    for (const cb of [...set]) {
      try {
        ;(cb as Listener<K>)(ev)
      } catch (err) {
        // Listener errors must not break the engine; surface them asynchronously.
        queueMicrotask(() => {
          throw err
        })
      }
    }
  }

  private onMessage(m: HostMessage): void {
    switch (m.kind) {
      case 'result': {
        const p = this.pending.get(m.id)
        if (!p) return
        this.pending.delete(m.id)
        if (p.timer) clearTimeout(p.timer)
        if (m.revision > this.revision) this.revision = m.revision
        if (m.ok) p.resolve(m.value)
        else p.reject(new DotloomError(m.error))
        return
      }
      case 'scene':
        if (!m.preview && m.revision > this.revision) this.revision = m.revision
        this.emit('scene', { delta: m.delta, revision: m.revision, preview: m.preview })
        return
      case 'events':
        if (m.revision > this.revision) this.revision = m.revision
        // Events are dispatched after the commit completed; listeners may call the
        // engine again — those calls are queued as new requests, never nested.
        for (const ev of m.events) this.emit(ev.type, ev as never)
        return
      case 'progress':
        this.emit('progress', { iterations: m.iterations })
        return
      case 'crashed':
        this.onCrash(m.message)
        return
    }
  }

  private onCrash(reason: string): void {
    if (this.crashed) return
    this.crashed = reason
    for (const [, p] of this.pending) {
      if (p.timer) clearTimeout(p.timer)
      p.reject(new DotloomError({ code: 'crashed', message: reason }))
    }
    this.pending.clear()
    this.emit('crash', { message: reason })
  }

  /** Whether the engine crashed (create a new one and reload the document). */
  get isCrashed(): boolean {
    return this.crashed !== null
  }

  private request(
    kind: 'init' | 'call' | 'dispose',
    args: unknown[],
    extra: { method?: MethodName; engineWasm?: string | undefined; opts?: CallOptions } = {},
  ): Promise<unknown> {
    if (this.disposed && kind !== 'dispose') {
      return Promise.reject(new DotloomError({ code: 'disposed', message: 'engine disposed' }))
    }
    if (this.crashed) return Promise.reject(new DotloomError({ code: 'crashed', message: this.crashed }))
    const id = this.nextId++
    const signal = extra.opts?.signal
    if (signal?.aborted) return Promise.reject(new DotloomError({ code: 'cancelled', message: 'aborted' }))
    return new Promise((resolve, reject) => {
      const entry: PendingCall = { method: extra.method ?? (kind as 'init' | 'dispose'), resolve, reject }
      if (this.timeoutMs > 0) {
        entry.timer = setTimeout(() => {
          this.pending.delete(id)
          this.transport.send({ v: 1, kind: 'cancel', id: this.nextId++, target: id })
          reject(new DotloomError({ code: 'timeout', message: `${entry.method} timed out` }))
        }, this.timeoutMs)
      }
      this.pending.set(id, entry)
      signal?.addEventListener(
        'abort',
        () => {
          if (!this.pending.has(id)) return
          this.transport.send({ v: 1, kind: 'cancel', id: this.nextId++, target: id })
        },
        { once: true },
      )
      if (kind === 'init') {
        const msg: { v: 1; kind: 'init'; id: number; engineWasm?: string } = { v: 1, kind: 'init', id }
        if (extra.engineWasm !== undefined) msg.engineWasm = extra.engineWasm
        this.transport.send(msg)
      } else if (kind === 'dispose') {
        this.transport.send({ v: 1, kind: 'dispose', id })
      } else {
        // Transfer only buffers the SDK created itself; caller-owned bytes are
        // copied (structured clone) so the caller's array stays usable.
        const transfer: Transferable[] = []
        for (const a of args) {
          if (a instanceof Uint8Array && this.owned.has(a.buffer as ArrayBuffer)) transfer.push(a.buffer as ArrayBuffer)
        }
        const msg: {
          v: 1
          kind: 'call'
          id: number
          method: MethodName
          args: unknown[]
          expectedRevision?: number
        } = { v: 1, kind: 'call', id, method: extra.method ?? 'capabilities', args }
        if (extra.opts?.expectedRevision !== undefined) msg.expectedRevision = extra.opts.expectedRevision
        this.transport.send(msg, transfer)
      }
    })
  }

  private call<T>(method: MethodName, args: unknown[] = [], opts?: CallOptions): Promise<T> {
    const extra: { method: MethodName; opts?: CallOptions } = { method }
    if (opts) extra.opts = opts
    return this.request('call', args, extra) as Promise<T>
  }

  // --- documents --------------------------------------------------------------

  /** Start an empty document. */
  newDocument(): Promise<number> {
    return this.call('newDocument')
  }

  /**
   * Open a `.dotl` file. The caller's bytes are copied to the engine, never
   * transferred: the array stays usable afterwards.
   */
  async load(file: Uint8Array | ArrayBuffer | Blob): Promise<LoadReport> {
    let bytes: Uint8Array
    if (file instanceof Uint8Array) bytes = file
    else if (file instanceof ArrayBuffer) bytes = new Uint8Array(file)
    else {
      bytes = new Uint8Array(await file.arrayBuffer())
      this.owned.add(bytes.buffer as ArrayBuffer)
    }
    return this.call('loadDotl', [bytes])
  }

  /** Load a document from its JSON form. */
  loadJson(doc: DocumentJson | string): Promise<number> {
    return this.call('loadJson', [typeof doc === 'string' ? doc : JSON.stringify(doc)])
  }

  /** Save as `.dotl` bytes; `view` is stored as view state (not geometry). */
  save(view?: unknown): Promise<Uint8Array> {
    return this.call('saveDotl', [view === undefined ? null : JSON.stringify(view)])
  }

  /** The document as JSON (a derived copy; edit through transactions). */
  documentJson(): Promise<DocumentJson> {
    return this.call('documentJson')
  }

  // --- transactions -----------------------------------------------------------

  /** Apply commands atomically. Rejects with `solve` diagnostics when hard rules fail. */
  apply(tx: Transaction | Command[], opts?: CallOptions): Promise<CommitReport> {
    const t: Transaction = Array.isArray(tx) ? { commands: tx } : tx
    return this.call('apply', [t], opts)
  }

  /** Undo the last commit. */
  undo(opts?: CallOptions): Promise<number> {
    return this.call('undo', [], opts)
  }

  /** Redo the last undone commit. */
  redo(opts?: CallOptions): Promise<number> {
    return this.call('redo', [], opts)
  }

  /** Undo/redo availability. */
  historyState(): Promise<{ canUndo: boolean; canRedo: boolean; bytes: number }> {
    return this.call('historyState')
  }

  /** Reserve IDs for entities/constraints created in one transaction. */
  reserveIds(n: number): Promise<number[]> {
    return this.call('reserveIds', [n])
  }

  // --- plugins ----------------------------------------------------------------

  /** Register plugin entity types (data definitions evaluated in Rust). */
  registerTypes(defs: EntityTypeDef | EntityTypeDef[], plugin = 'plugin'): Promise<string[]> {
    return this.call('registerTypes', [JSON.stringify(defs), plugin])
  }

  /** Unregister a plugin type (its entities become read-only; data is kept). */
  unregisterType(typeId: string): Promise<void> {
    return this.call('unregisterType', [typeId])
  }

  /** Enable or disable a plugin type. */
  setTypeEnabled(typeId: string, enabled: boolean): Promise<void> {
    return this.call('setTypeEnabled', [typeId, enabled])
  }

  /** Registered plugin types. */
  pluginTypes(): Promise<{ typeId: string; enabled: boolean; plugin: string; definition: EntityTypeDef }[]> {
    return this.call('pluginTypes')
  }

  // --- selection & queries ----------------------------------------------------

  setSelection(ids: EntityId[]): Promise<void> {
    return this.call('setSelection', [JSON.stringify(ids)])
  }

  selection(): Promise<EntityId[]> {
    return this.call('selection')
  }

  /** Entities under a model point within `radius` model units. */
  hitTest(p: Point, radius: number): Promise<Hit[]> {
    return this.call('hitTest', [p[0], p[1], radius])
  }

  /** Window (inside) or crossing (touching) selection. */
  selectInRect(a: Point, b: Point, crossing: boolean): Promise<EntityId[]> {
    return this.call('selectInRect', [a[0], a[1], b[0], b[1], crossing])
  }

  snap(q: SnapQuery): Promise<Snap | null> {
    return this.call('snap', [JSON.stringify(q)])
  }

  entityInfo(id: EntityId): Promise<EntityInfo> {
    return this.call('entityInfo', [id])
  }

  /** Solve status and diagnostics of the whole document (nothing changes). */
  analyze(): Promise<{ status: SolveStatus; diagnostics: DiagnosticReport[] }> {
    return this.call('analyze')
  }

  /** Hard rules violated by the stored values (independent check). */
  verify(): Promise<{ rule: string; residual: number; tolerance: number }[]> {
    return this.call('verify')
  }

  // --- drags ------------------------------------------------------------------

  beginDrag(spec: DragSpec): Promise<void> {
    return this.call('beginDrag', [JSON.stringify(spec)])
  }

  /**
   * Move the drag. Resolves to `null` when a newer position superseded this one
   * (only the newest position is solved).
   */
  async dragTo(p: Point): Promise<DragPreview | null> {
    try {
      return await this.call<DragPreview>('dragTo', [p[0], p[1]])
    } catch (err) {
      if (err instanceof DotloomError && err.code === 'superseded') return null
      throw err
    }
  }

  /** Commit (one history entry) or cancel the drag. */
  endDrag(commit: boolean): Promise<CommitReport | null> {
    return this.call('endDrag', [commit])
  }

  // --- files ------------------------------------------------------------------

  exportSvg(opts: { foreground?: string; background?: string; margin?: number } = {}): Promise<{
    svg: string
    report: ConversionReport
  }> {
    return this.call('exportSvg', [opts.foreground ?? null, opts.background ?? null, opts.margin ?? null])
  }

  exportDxf(): Promise<{ dxf: string; report: ConversionReport }> {
    return this.call('exportDxf')
  }

  /** Import SVG as one undoable transaction. */
  importSvg(text: string): Promise<{ report: ConversionReport; commit: CommitReport }> {
    return this.call('importSvg', [text])
  }

  /** Import ASCII DXF as one undoable transaction. */
  importDxf(bytes: Uint8Array): Promise<{ report: ConversionReport; commit: CommitReport }> {
    return this.call('importDxf', [bytes])
  }

  /** Copy entities (plus internal constraints/groups) for `paste`. */
  /**
   * Engine WebAssembly memory in bytes. Linear memory never shrinks, so this is the
   * high-water mark (e.g. the peak memory of opening a file).
   */
  memory(): Promise<{ wasmBytes: number }> {
    return this.call('memory')
  }

  copy(ids: EntityId[]): Promise<Clipboard> {
    return this.call('copy', [JSON.stringify(ids)])
  }

  /** Request a full scene (delivered through the `scene` event). */
  requestFullScene(): Promise<void> {
    return this.call('fullScene')
  }

  /**
   * Test hook: crash the engine instance on purpose (verifies crash handling).
   * Afterwards every call rejects with `crashed`; create a new engine.
   */
  debugCrashForTesting(): Promise<void> {
    return this.call('debugTrap')
  }

  /** Stop the engine and release its resources. Pending requests reject. */
  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    void this.request('dispose', []).catch(() => undefined)
    for (const [id, p] of this.pending) {
      if (p.method === 'dispose') continue
      if (p.timer) clearTimeout(p.timer)
      p.reject(new DotloomError({ code: 'disposed', message: 'engine disposed' }))
      this.pending.delete(id)
    }
    this.listeners.clear()
    // Give the dispose message a chance to reach the worker before termination.
    setTimeout(() => this.transport.terminate(), 0)
  }

  /** Whether `dispose` was called. */
  get isDisposed(): boolean {
    return this.disposed
  }
}

export type { ErrorPayload }
