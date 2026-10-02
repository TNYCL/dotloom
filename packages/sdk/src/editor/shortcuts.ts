/** Configurable keyboard shortcuts. `Mod` is Ctrl (Windows/Linux) or Cmd (macOS). */

import type { InputKey } from './types.js'

export type ShortcutAction =
  | 'undo'
  | 'redo'
  | 'delete'
  | 'copy'
  | 'cut'
  | 'paste'
  | 'selectAll'
  | 'cancel'
  | 'fit'
  | 'toggleGrid'
  | 'toggleSnap'
  | 'group'
  | 'ungroup'
  | 'nudgeLeft'
  | 'nudgeRight'
  | 'nudgeUp'
  | 'nudgeDown'
  | 'zoomIn'
  | 'zoomOut'

export type ShortcutMap = Record<ShortcutAction, string[]>

export const DEFAULT_SHORTCUTS: ShortcutMap = {
  undo: ['Mod+z'],
  redo: ['Mod+Shift+z', 'Mod+y'],
  delete: ['Delete', 'Backspace'],
  copy: ['Mod+c'],
  cut: ['Mod+x'],
  paste: ['Mod+v'],
  selectAll: ['Mod+a'],
  cancel: ['Escape'],
  fit: ['Shift+f', 'Home'],
  toggleGrid: ['F7'],
  toggleSnap: ['F3'],
  group: ['Mod+g'],
  ungroup: ['Mod+Shift+g'],
  nudgeLeft: ['ArrowLeft', 'Shift+ArrowLeft'],
  nudgeRight: ['ArrowRight', 'Shift+ArrowRight'],
  nudgeUp: ['ArrowUp', 'Shift+ArrowUp'],
  nudgeDown: ['ArrowDown', 'Shift+ArrowDown'],
  zoomIn: ['+', '='],
  zoomOut: ['-'],
}

/** Normalized combo of an input key, e.g. `Mod+Shift+z`. */
export function comboOf(k: InputKey): string {
  const key = k.key.length === 1 ? k.key.toLowerCase() : k.key
  const parts: string[] = []
  if (k.mod) parts.push('Mod')
  if (k.alt) parts.push('Alt')
  // Shift is implied by the character for printable symbols like "+".
  if (k.shift && (k.key.length > 1 || /[a-z]/i.test(k.key))) parts.push('Shift')
  parts.push(key)
  return parts.join('+')
}

function normalize(combo: string): string {
  const parts = combo.split('+')
  const key = parts.pop() ?? ''
  const mods = new Set(parts.map((p) => (p === 'Ctrl' || p === 'Cmd' || p === 'Meta' ? 'Mod' : p)))
  const ordered = ['Mod', 'Alt', 'Shift'].filter((m) => mods.has(m))
  ordered.push(key.length === 1 ? key.toLowerCase() : key)
  return ordered.join('+')
}

/** Lookup table from combo to action. */
export function compileShortcuts(map: ShortcutMap): Map<string, ShortcutAction> {
  const out = new Map<string, ShortcutAction>()
  for (const [action, combos] of Object.entries(map) as [ShortcutAction, string[]][]) {
    for (const c of combos) out.set(normalize(c), action)
  }
  return out
}
