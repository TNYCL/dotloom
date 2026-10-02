/** Renderer colors: `#rrggbb`/`#rrggbbaa` strings in the API, `0xRRGGBBAA` on the wire. */

export interface ThemeColors {
  background: string
  foreground: string
  selection: string
  hover: string
  problem: string
  gridMinor: string
  gridMajor: string
  gridAxis: string
  marker: string
  marqueeWindow: string
  marqueeCrossing: string
  guide: string
  sketch: string
}

export type ThemeName = 'light' | 'dark'

/** Theme input: a preset plus optional color overrides. */
export interface ThemeInput {
  preset?: ThemeName
  colors?: Partial<ThemeColors>
}

/** Parse `#rgb`, `#rrggbb` or `#rrggbbaa` to `0xRRGGBBAA`; `null` if invalid. */
export function parseColor(s: string): number | null {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(s.trim())
  if (!m?.[1]) return null
  const h = m[1]
  if (h.length === 3) {
    const r = Number.parseInt(h.charAt(0), 16) * 17
    const g = Number.parseInt(h.charAt(1), 16) * 17
    const b = Number.parseInt(h.charAt(2), 16) * 17
    return ((r << 24) | (g << 16) | (b << 8) | 0xff) >>> 0
  }
  const v = Number.parseInt(h, 16)
  return h.length === 6 ? ((v << 8) | 0xff) >>> 0 : v >>> 0
}

/** `0xRRGGBBAA` → `#rrggbbaa`. */
export function formatColor(v: number): string {
  return `#${(v >>> 0).toString(16).padStart(8, '0')}`
}

/** Merge overrides into a preset (wire format). Invalid colors throw. */
export function themeToWire(preset: Record<string, number>, colors: Partial<ThemeColors> = {}): Record<string, number> {
  const out = { ...preset }
  for (const [k, v] of Object.entries(colors)) {
    if (v === undefined) continue
    const c = parseColor(v)
    if (c === null) throw new TypeError(`invalid color for ${k}: ${v}`)
    out[k] = c
  }
  return out
}
