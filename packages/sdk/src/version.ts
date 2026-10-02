/** Version of `@dotloom/sdk` (kept equal to package.json by a test). */
export const SDK_VERSION = '0.1.0'

function parse(v: string): [number, number, number] | null {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec(v.trim())
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null
}

function cmp(a: [number, number, number], b: [number, number, number]): number {
  return a[0] - b[0] || a[1] - b[1] || a[2] - b[2]
}

/**
 * Minimal semver range check supporting `^x.y.z`, `~x.y.z`, `>=x.y.z`, `x.y.z`
 * and `*`, joined by spaces (AND). Enough for plugin compatibility declarations.
 */
export function satisfies(version: string, range: string): boolean {
  const v = parse(version)
  if (!v) return false
  return range
    .trim()
    .split(/\s+/)
    .every((part) => {
      if (part === '*' || part === '') return true
      const op = /^(\^|~|>=|<=|>|<|=)?(.*)$/.exec(part)
      const r = parse(op?.[2] ?? '')
      if (!r) return false
      switch (op?.[1] ?? '=') {
        case '^':
          // ^0.y.z locks the minor version; ^x.y.z (x > 0) locks the major.
          return cmp(v, r) >= 0 && (r[0] > 0 ? v[0] === r[0] : r[1] > 0 ? v[0] === 0 && v[1] === r[1] : cmp(v, r) === 0)
        case '~':
          return cmp(v, r) >= 0 && v[0] === r[0] && v[1] === r[1]
        case '>=':
          return cmp(v, r) >= 0
        case '<=':
          return cmp(v, r) <= 0
        case '>':
          return cmp(v, r) > 0
        case '<':
          return cmp(v, r) < 0
        default:
          return cmp(v, r) === 0
      }
    })
}
