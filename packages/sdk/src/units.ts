/**
 * Display units. The model is always millimetres / radians / seconds; units only
 * change how values are shown and typed (ADR-0002).
 */

import type { LengthUnit } from './types.js'

export interface UnitInfo {
  symbol: string
  /** Millimetres per unit. */
  mm: number
  /** Default decimals when formatting. */
  decimals: number
}

export const LENGTH_UNITS: Record<LengthUnit, UnitInfo> = {
  millimetre: { symbol: 'mm', mm: 1, decimals: 1 },
  centimetre: { symbol: 'cm', mm: 10, decimals: 2 },
  metre: { symbol: 'm', mm: 1000, decimals: 3 },
  inch: { symbol: 'in', mm: 25.4, decimals: 3 },
  foot: { symbol: 'ft', mm: 304.8, decimals: 4 },
}

const SYMBOLS: Record<string, LengthUnit> = {
  mm: 'millimetre',
  cm: 'centimetre',
  m: 'metre',
  in: 'inch',
  '"': 'inch',
  ft: 'foot',
  "'": 'foot',
}

function trimNumber(v: number, decimals: number): string {
  const s = v.toFixed(decimals)
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s
}

/** Format a length in millimetres in `unit` (e.g. `160 cm`). */
export function formatLength(mm: number, unit: LengthUnit, decimals?: number, withSymbol = true): string {
  const u = LENGTH_UNITS[unit]
  const v = mm / u.mm
  const s = Number.isFinite(v) ? trimNumber(Object.is(v, -0) ? 0 : v, decimals ?? u.decimals) : '—'
  return withSymbol ? `${s} ${u.symbol}` : s
}

/**
 * Parse a typed length into millimetres. Accepts `120`, `120 cm`, `1.2m`,
 * `1,2 m` (decimal comma), `-5 mm`. Plain numbers use `unit`. Returns `null`
 * for anything else (no expression evaluation).
 */
export function parseLength(text: string, unit: LengthUnit): number | null {
  const m = /^\s*([+-]?(?:\d+(?:[.,]\d*)?|[.,]\d+)(?:e[+-]?\d+)?)\s*([a-z"']*)\s*$/i.exec(text)
  if (!m?.[1]) return null
  const v = Number(m[1].replace(',', '.'))
  if (!Number.isFinite(v)) return null
  const sym = (m[2] ?? '').toLowerCase()
  const u = sym ? SYMBOLS[sym] : unit
  if (!u) return null
  return v * LENGTH_UNITS[u].mm
}

/** Format radians as degrees. */
export function formatAngle(rad: number, decimals = 2): string {
  return `${trimNumber((rad * 180) / Math.PI, decimals)}°`
}

/** Parse degrees (`45`, `45°`, `45 deg`) or radians (`0.5 rad`) into radians. */
export function parseAngle(text: string): number | null {
  const m = /^\s*([+-]?(?:\d+(?:[.,]\d*)?|[.,]\d+))\s*(°|deg|rad)?\s*$/i.exec(text)
  if (!m?.[1]) return null
  const v = Number(m[1].replace(',', '.'))
  if (!Number.isFinite(v)) return null
  return m[2]?.toLowerCase() === 'rad' ? v : (v * Math.PI) / 180
}

/** Format seconds as `2h 30m`, `45m`, `30s` (zero parts omitted). */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds)) return '—'
  const neg = seconds < 0
  let s = Math.round(Math.abs(seconds))
  const h = Math.floor(s / 3600)
  s -= h * 3600
  const m = Math.floor(s / 60)
  s -= m * 60
  const parts = [h ? `${h}h` : '', m ? `${m}m` : '', s || (!h && !m) ? `${s}s` : ''].filter(Boolean)
  return (neg ? '-' : '') + parts.join(' ')
}

/** Parse `90` (seconds), `1.5h`, `2h 30m`, `45m`, `10s`, `1,5 h` into seconds. */
export function parseDuration(text: string): number | null {
  const t = text.trim().toLowerCase().replace(/,/g, '.')
  if (!t) return null
  if (/^[+-]?\d+(\.\d+)?$/.test(t)) return Number(t)
  const re = /([+-]?\d+(?:\.\d+)?)\s*(hours?|h|minutes?|min|m|seconds?|sec|s)\s*/gy
  let total = 0
  let any = false
  for (const m of t.matchAll(re)) {
    any = true
    const v = Number(m[1])
    const u = m[2] ?? 's'
    total += u.startsWith('h') ? v * 3600 : u.startsWith('m') ? v * 60 : v
  }
  // The sticky regex must consume the whole string.
  const consumed = [...t.matchAll(re)].map((m) => m[0]).join('')
  return any && consumed.length === t.length ? total : null
}
