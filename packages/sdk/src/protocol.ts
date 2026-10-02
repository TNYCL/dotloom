/**
 * Versioned messages between the SDK client (main thread) and the engine host
 * (Web Worker or in-thread). The protocol version is independent from the package
 * version and the document schema version.
 */

import type { EngineEvent } from './types.js'

export const PROTOCOL_VERSION = 1

/** Engine methods callable through the protocol. */
export type MethodName =
  | 'capabilities'
  | 'apply'
  | 'undo'
  | 'redo'
  | 'newDocument'
  | 'loadDotl'
  | 'loadJson'
  | 'saveDotl'
  | 'documentJson'
  | 'registerTypes'
  | 'unregisterType'
  | 'setTypeEnabled'
  | 'pluginTypes'
  | 'reserveIds'
  | 'setSelection'
  | 'selection'
  | 'hitTest'
  | 'selectInRect'
  | 'snap'
  | 'beginDrag'
  | 'dragTo'
  | 'endDrag'
  | 'entityInfo'
  | 'analyze'
  | 'verify'
  | 'exportSvg'
  | 'exportDxf'
  | 'importSvg'
  | 'importDxf'
  | 'copy'
  | 'fullScene'
  | 'historyState'
  | 'memory'
  | 'debugTrap'

/** Machine-readable error codes. */
export type ErrorCode =
  | 'stale'
  | 'busy'
  | 'command'
  | 'invalid'
  | 'solve'
  | 'validation'
  | 'cancelled'
  | 'nothingToUndo'
  | 'nothingToRedo'
  | 'notActive'
  | 'load'
  | 'plugin'
  | 'file'
  | 'import'
  | 'protocol'
  | 'notFound'
  | 'superseded'
  | 'crashed'
  | 'disposed'
  | 'timeout'
  | 'init'
  | 'render'

export interface ErrorPayload {
  code: ErrorCode
  message: string
  details?: unknown
}

export type ClientMessage =
  | { v: 1; kind: 'init'; id: number; engineWasm?: string }
  | { v: 1; kind: 'call'; id: number; method: MethodName; args: unknown[]; expectedRevision?: number }
  | { v: 1; kind: 'cancel'; id: number; target: number }
  | { v: 1; kind: 'dispose'; id: number }

export type HostMessage =
  | { v: 1; kind: 'result'; id: number; ok: true; value: unknown; revision: number }
  | { v: 1; kind: 'result'; id: number; ok: false; error: ErrorPayload; revision: number }
  | { v: 1; kind: 'events'; events: EngineEvent[]; revision: number }
  | { v: 1; kind: 'scene'; delta: ArrayBuffer; revision: number; preview: boolean }
  | { v: 1; kind: 'progress'; id: number; iterations: number }
  | { v: 1; kind: 'crashed'; message: string }

/** Methods that change the document (followed by scene deltas and events). */
export const MUTATING: ReadonlySet<MethodName> = new Set<MethodName>([
  'apply',
  'undo',
  'redo',
  'newDocument',
  'loadDotl',
  'loadJson',
  'registerTypes',
  'unregisterType',
  'setTypeEnabled',
  'setSelection',
  'endDrag',
  'importSvg',
  'importDxf',
])

/** Error raised by SDK calls. */
export class DotloomError extends Error {
  readonly code: ErrorCode
  readonly details: unknown
  constructor(payload: ErrorPayload) {
    super(payload.message)
    this.name = 'DotloomError'
    this.code = payload.code
    this.details = payload.details
  }
}
