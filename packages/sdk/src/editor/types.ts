/** Public types of the editor layer (tools, input, state). */

import type { DotloomEngine } from '../engine.js'
import type { DotloomError } from '../protocol.js'
import type { CommitReport, DiagnosticReport, EntityId, Point, Snap, SnapOptions, Transaction } from '../types.js'
import type { OverlayState, ViewportLike } from '../viewport.js'

/** Pointer input in CSS pixels relative to the canvas. */
export interface InputPointer {
  x: number
  y: number
  button: number
  buttons: number
  shift: boolean
  /** Ctrl on Windows/Linux, Cmd on macOS. */
  mod: boolean
  alt: boolean
  pointerId: number
  pointerType: 'mouse' | 'pen' | 'touch'
}

/** Keyboard input (from a focused editor, never from form fields). */
export interface InputKey {
  key: string
  code: string
  shift: boolean
  mod: boolean
  alt: boolean
  repeat: boolean
}

/** What tools receive for pointer events. */
export interface ToolPointer {
  /** CSS pixels. */
  screen: Point
  /** Raw world position. */
  world: Point
  /** Snapped position (or `world` when nothing snapped). */
  point: Point
  snap: Snap | null
  button: number
  shift: boolean
  mod: boolean
  alt: boolean
}

export type CancelReason = 'escape' | 'capture-lost' | 'blur' | 'tool-change' | 'dispose'

export interface ToolContext {
  readonly engine: DotloomEngine
  readonly viewport: ViewportLike
  /** Draw the tool's preview (merged with the snap marker and grips). */
  setOverlay(o: OverlayState): void
  /** Status prompt key (UI translates it), e.g. `tool.line.start`. */
  setPrompt(key: string): void
  /** Tool state for the UI and tests: `idle`, `start`, `preview`, … */
  setState(state: string): void
  /** Apply a transaction; failures are reported in the editor state and return `null`. */
  apply(tx: Transaction): Promise<CommitReport | null>
  /** Current selection. */
  selection(): EntityId[]
  setSelection(ids: EntityId[]): Promise<void>
  /** CSS pixels → model units at the current zoom. */
  pxToWorld(px: number): number
  /** Entities ignored by snapping (e.g. the ones being dragged). */
  setSnapExclude(ids: EntityId[]): void
  /** Report an error without throwing. */
  report(err: unknown): void
  /** Model units per typed display unit. */
  unitScale(): number
  /** Show extra markers (e.g. grips). */
  setMarkers(markers: OverlayState['markers']): void
}

/**
 * A tool: a small state machine driven by pointer and keyboard input
 * (idle → start → preview → commit | cancel). Tools only issue engine commands.
 */
export interface Tool {
  readonly id: string
  /** i18n key or plain label. */
  readonly label: string
  /** Single-key shortcut (e.g. `l`). */
  readonly shortcut?: string
  readonly cursor?: string
  /** Whether pointer positions are snapped (default true). */
  readonly snaps?: boolean
  activate?(ctx: ToolContext): void
  deactivate?(ctx: ToolContext): void
  pointerDown?(ctx: ToolContext, p: ToolPointer): void | Promise<void>
  pointerMove?(ctx: ToolContext, p: ToolPointer): void | Promise<void>
  pointerUp?(ctx: ToolContext, p: ToolPointer): void | Promise<void>
  doubleClick?(ctx: ToolContext, p: ToolPointer): void | Promise<void>
  /** While true, every key goes to `key` first (text entry); shortcuts are suspended. */
  capturesKeyboard?(): boolean
  /** Keys not consumed by global shortcuts (Enter, Backspace, …). Return true if handled. */
  key?(ctx: ToolContext, key: InputKey): boolean | Promise<boolean>
  /** Typed value (numbers like `1200` or `600,400`), submitted with Enter. Return true if accepted. */
  value?(ctx: ToolContext, text: string): boolean | Promise<boolean>
  /** Abort the current operation and return to idle. */
  cancel?(ctx: ToolContext, reason: CancelReason): void | Promise<void>
}

export interface EditorErrorState {
  code: string
  message: string
  diagnostics: DiagnosticReport[]
}

/** Editor UI state (changes rarely; pointer position lives in `PointerState`). */
export interface EditorState {
  tool: string
  toolState: string
  prompt: string
  selection: EntityId[]
  canUndo: boolean
  canRedo: boolean
  undoLabel: string | null
  redoLabel: string | null
  revision: number
  snapEnabled: boolean
  snapOptions: SnapOptions
  /** Typed value being entered. */
  input: string
  busy: boolean
  error: EditorErrorState | null
  lastCommit: CommitReport | null
  /** Layer that receives new entities (`null` = default layer). */
  activeLayer: number | null
  /** Incremented when tools are registered or removed. */
  toolsVersion: number
  gridVisible: boolean
}

/** Pointer feedback (changes on every move). */
export interface PointerState {
  world: Point | null
  snap: Snap | null
}

export type ErrorLike = DotloomError | Error
