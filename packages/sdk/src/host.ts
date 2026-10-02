/**
 * Engine host: owns the WASM engine and serves protocol messages.
 *
 * Runs inside a dedicated Web Worker in browsers (see `worker.ts`) or in the same
 * thread for Node and tests (`InlineTransport`). Calls are processed in order.
 * Long solves run as budgeted `step` calls separated by real event-loop yields, so
 * `cancel` messages and newer drag positions are received while a solve runs.
 * Queued drag updates are coalesced: only the newest position is solved.
 */

import {
  type ClientMessage,
  type ErrorCode,
  type ErrorPayload,
  type HostMessage,
  type MethodName,
  MUTATING,
  PROTOCOL_VERSION,
} from './protocol.js'
import { isEmptyDelta } from './scene.js'
import type { WasmEngine } from './wasm/engine/dotloom_wasm.js'

export type Post = (msg: HostMessage, transfer?: Transferable[]) => void
export type EngineFactory = (engineWasm?: string) => Promise<new () => WasmEngine>

interface QueuedCall {
  id: number
  method: MethodName
  args: unknown[]
  expectedRevision?: number
}

/** Yield to the event loop (macrotask), so incoming messages get processed. */
export function createYield(): () => Promise<void> {
  const g = globalThis as { setImmediate?: (cb: () => void) => unknown; MessageChannel?: typeof MessageChannel }
  if (typeof g.setImmediate === 'function') {
    const si = g.setImmediate
    return () => new Promise<void>((r) => si(r))
  }
  if (typeof g.MessageChannel === 'function') {
    const ch = new g.MessageChannel()
    const waiting: (() => void)[] = []
    ch.port1.onmessage = () => waiting.shift()?.()
    return () =>
      new Promise<void>((r) => {
        waiting.push(r)
        ch.port2.postMessage(0)
      })
  }
  return () => new Promise<void>((r) => setTimeout(r, 0))
}

function parseError(err: unknown): ErrorPayload | null {
  if (typeof err !== 'string') return null
  try {
    const v = JSON.parse(err) as Partial<ErrorPayload>
    if (typeof v.code === 'string' && typeof v.message === 'string') {
      return { code: v.code as ErrorCode, message: v.message, details: v.details }
    }
  } catch {
    // not one of our JSON errors
  }
  return { code: 'invalid', message: err }
}

function num(v: unknown): number {
  return typeof v === 'number' ? v : Number.NaN
}

function str(v: unknown): string {
  return typeof v === 'string' ? v : JSON.stringify(v)
}

/** Solver iterations per step between yields. */
export const DEFAULT_STEP_BUDGET = 1

export class EngineHost {
  private engine: WasmEngine | null = null
  private queue: QueuedCall[] = []
  private running = false
  private current: number | null = null
  private cancelled = new Set<number>()
  private dead = false
  private readonly yieldNow: () => Promise<void>

  constructor(
    private readonly post: Post,
    private readonly factory: EngineFactory,
    private readonly stepBudget = DEFAULT_STEP_BUDGET,
  ) {
    this.yieldNow = createYield()
  }

  private revision(): number {
    try {
      return this.engine ? this.engine.revision() : 0
    } catch {
      return 0
    }
  }

  private reply(id: number, value: unknown, transfer?: Transferable[]): void {
    this.post({ v: 1, kind: 'result', id, ok: true, value, revision: this.revision() }, transfer)
  }

  private fail(id: number, error: ErrorPayload): void {
    this.post({ v: 1, kind: 'result', id, ok: false, error, revision: this.revision() })
  }

  /** Handle one client message. */
  receive(msg: ClientMessage): void {
    if (!msg || msg.v !== PROTOCOL_VERSION) {
      const id = typeof (msg as { id?: unknown })?.id === 'number' ? (msg as { id: number }).id : -1
      this.fail(id, { code: 'protocol', message: `unsupported protocol version (expected ${PROTOCOL_VERSION})` })
      return
    }
    if (this.dead && msg.kind !== 'dispose') {
      this.fail(msg.id, { code: 'crashed', message: 'the engine crashed; create a new engine' })
      return
    }
    switch (msg.kind) {
      case 'init':
        void this.init(msg.id, msg.engineWasm)
        return
      case 'dispose':
        this.queue = []
        try {
          this.engine?.free()
        } catch {
          // already gone
        }
        this.engine = null
        this.reply(msg.id, null)
        return
      case 'cancel': {
        const i = this.queue.findIndex((c) => c.id === msg.target)
        if (i >= 0) {
          const [c] = this.queue.splice(i, 1)
          if (c) this.fail(c.id, { code: 'cancelled', message: 'cancelled before it started' })
        } else if (this.current === msg.target) {
          this.cancelled.add(msg.target)
        }
        this.reply(msg.id, null)
        return
      }
      case 'call': {
        if (msg.method === 'dragTo') {
          // Latest wins: drop queued (not started) drag updates.
          this.queue = this.queue.filter((c) => {
            if (c.method !== 'dragTo') return true
            this.fail(c.id, { code: 'superseded', message: 'a newer drag position arrived' })
            return false
          })
        }
        const call: QueuedCall = { id: msg.id, method: msg.method, args: msg.args }
        if (msg.expectedRevision !== undefined) call.expectedRevision = msg.expectedRevision
        this.queue.push(call)
        void this.pump()
        return
      }
    }
  }

  private async init(id: number, engineWasm?: string): Promise<void> {
    try {
      const Ctor = await this.factory(engineWasm)
      this.engine?.free()
      this.engine = new Ctor()
      this.reply(id, JSON.parse(this.engine.capabilities()))
    } catch (err) {
      this.fail(id, { code: 'init', message: `cannot initialize the engine: ${String(err)}` })
    }
  }

  private async pump(): Promise<void> {
    if (this.running) return
    this.running = true
    try {
      while (this.queue.length > 0) {
        const c = this.queue.shift()
        if (!c) break
        this.current = c.id
        await this.execute(c)
        this.current = null
        this.cancelled.delete(c.id)
      }
    } finally {
      this.running = false
    }
  }

  private flush(): void {
    const e = this.engine
    if (!e) return
    const delta = e.take_scene_delta()
    if (!isEmptyDelta(delta)) {
      const buf = delta.buffer.slice(delta.byteOffset, delta.byteOffset + delta.byteLength) as ArrayBuffer
      this.post({ v: 1, kind: 'scene', delta: buf, revision: this.revision(), preview: false }, [buf])
    }
    const events = JSON.parse(e.take_events()) as unknown[]
    if (events.length > 0) {
      this.post({ v: 1, kind: 'events', events: events as never, revision: this.revision() })
    }
  }

  private crash(err: unknown): void {
    this.dead = true
    const message = err instanceof Error ? err.message : String(err)
    this.post({ v: 1, kind: 'crashed', message })
    for (const c of this.queue) this.fail(c.id, { code: 'crashed', message })
    this.queue = []
  }

  private async execute(c: QueuedCall): Promise<void> {
    const e = this.engine
    if (!e) {
      this.fail(c.id, { code: 'init', message: 'engine not initialized' })
      return
    }
    const a = c.args
    const rev = c.expectedRevision
    try {
      let value: unknown = null
      let transfer: Transferable[] | undefined
      switch (c.method) {
        case 'capabilities':
          value = JSON.parse(e.capabilities())
          break
        case 'apply': {
          e.begin_apply(str(a[0]), rev)
          let iterations = 0
          for (;;) {
            if (this.cancelled.has(c.id)) {
              e.cancel_pending()
              this.fail(c.id, { code: 'cancelled', message: 'solve cancelled' })
              return
            }
            const r = JSON.parse(e.step(this.stepBudget)) as {
              state: string
              ok?: boolean
              report?: unknown
              error?: ErrorPayload
            }
            if (r.state === 'running') {
              iterations += this.stepBudget
              if (iterations === this.stepBudget || iterations % 8 === 0) {
                this.post({ v: 1, kind: 'progress', id: c.id, iterations })
              }
              await this.yieldNow()
              continue
            }
            this.flush()
            if (r.ok) this.reply(c.id, r.report)
            else this.fail(c.id, r.error ?? { code: 'invalid', message: 'unknown failure' })
            return
          }
        }
        case 'undo':
          value = e.undo(rev)
          break
        case 'redo':
          value = e.redo(rev)
          break
        case 'newDocument':
          value = e.new_document()
          break
        case 'loadDotl':
          value = JSON.parse(e.load_dotl(a[0] as Uint8Array))
          break
        case 'loadJson':
          value = e.load_json(str(a[0]))
          break
        case 'saveDotl': {
          const bytes = e.save_dotl(a[0] === undefined || a[0] === null ? undefined : str(a[0]))
          value = bytes
          transfer = [bytes.buffer as ArrayBuffer]
          break
        }
        case 'documentJson':
          value = JSON.parse(e.document_json())
          break
        case 'registerTypes':
          value = JSON.parse(e.register_types(str(a[0]), String(a[1] ?? 'plugin')))
          break
        case 'unregisterType':
          e.unregister_type(String(a[0]))
          break
        case 'setTypeEnabled':
          e.set_type_enabled(String(a[0]), Boolean(a[1]))
          break
        case 'pluginTypes':
          value = JSON.parse(e.plugin_types())
          break
        case 'reserveIds':
          value = JSON.parse(e.reserve_ids(num(a[0])))
          break
        case 'setSelection':
          e.set_selection(str(a[0]))
          break
        case 'selection':
          value = JSON.parse(e.selection())
          break
        case 'hitTest':
          value = JSON.parse(e.hit_test(num(a[0]), num(a[1]), num(a[2])))
          break
        case 'selectInRect':
          value = JSON.parse(e.select_in_rect(num(a[0]), num(a[1]), num(a[2]), num(a[3]), Boolean(a[4])))
          break
        case 'snap':
          value = JSON.parse(e.snap(str(a[0])))
          break
        case 'beginDrag':
          e.begin_drag(str(a[0]))
          break
        case 'dragTo': {
          value = JSON.parse(e.drag_to(num(a[0]), num(a[1])))
          const delta = e.take_preview()
          if (delta.byteLength > 0 && !isEmptyDelta(delta)) {
            const buf = delta.buffer.slice(delta.byteOffset, delta.byteOffset + delta.byteLength) as ArrayBuffer
            this.post({ v: 1, kind: 'scene', delta: buf, revision: this.revision(), preview: true }, [buf])
          }
          break
        }
        case 'endDrag':
          value = JSON.parse(e.end_drag(Boolean(a[0])))
          break
        case 'entityInfo':
          value = JSON.parse(e.entity_info(num(a[0])))
          break
        case 'analyze':
          value = JSON.parse(e.analyze())
          break
        case 'verify':
          value = JSON.parse(e.verify())
          break
        case 'exportSvg':
          value = JSON.parse(
            e.export_svg(
              (a[0] as string | undefined) ?? undefined,
              (a[1] as string | undefined) ?? undefined,
              typeof a[2] === 'number' ? a[2] : undefined,
            ),
          )
          break
        case 'exportDxf':
          value = JSON.parse(e.export_dxf())
          break
        case 'importSvg':
          value = JSON.parse(e.import_svg(String(a[0])))
          break
        case 'importDxf':
          value = JSON.parse(e.import_dxf(a[0] as Uint8Array))
          break
        case 'copy':
          value = JSON.parse(e.copy(str(a[0])))
          break
        case 'fullScene': {
          const bytes = e.full_scene()
          const buf = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer
          this.post({ v: 1, kind: 'scene', delta: buf, revision: this.revision(), preview: false }, [buf])
          break
        }
        case 'historyState':
          value = JSON.parse(e.history_state())
          break
        case 'debugTrap':
          e.debug_trap()
          break
      }
      if (MUTATING.has(c.method)) this.flush()
      this.reply(c.id, value, transfer)
    } catch (err) {
      const payload = parseError(err)
      if (payload) {
        this.fail(c.id, payload)
        return
      }
      // Not a typed engine error: the WASM instance trapped (e.g. a Rust panic).
      this.fail(c.id, { code: 'crashed', message: err instanceof Error ? err.message : String(err) })
      this.crash(err)
    }
  }
}
