/** Small geometry helpers for tools (exact math stays in the engine). */

import type { Affine, Point, Shape } from '../types.js'

export const TAU = Math.PI * 2

export function dist(a: Point, b: Point): number {
  return Math.hypot(b[0] - a[0], b[1] - a[1])
}

export function sub(a: Point, b: Point): Point {
  return [a[0] - b[0], a[1] - b[1]]
}

export function add(a: Point, b: Point): Point {
  return [a[0] + b[0], a[1] + b[1]]
}

export function angleOf(from: Point, to: Point): number {
  return Math.atan2(to[1] - from[1], to[0] - from[0])
}

/** Normalize to `[0, 2π)`. */
export function normAngle(a: number): number {
  const r = a % TAU
  return r < 0 ? r + TAU : r
}

/** Translation. */
export function translate(dx: number, dy: number): Affine {
  return [1, 0, 0, 1, dx, dy]
}

/** Rotation by `theta` about `c`. */
export function rotateAbout(c: Point, theta: number): Affine {
  const cos = Math.cos(theta)
  const sin = Math.sin(theta)
  return [cos, sin, -sin, cos, c[0] - cos * c[0] + sin * c[1], c[1] - sin * c[0] - cos * c[1]]
}

/** Uniform scale by `k` about `c`. */
export function scaleAbout(c: Point, k: number): Affine {
  return [k, 0, 0, k, c[0] * (1 - k), c[1] * (1 - k)]
}

export function applyAffine(t: Affine, p: Point): Point {
  return [t[0] * p[0] + t[2] * p[1] + t[4], t[1] * p[0] + t[3] * p[1] + t[5]]
}

/** Circle through three points, or `null` if they are (nearly) collinear. */
export function circumcircle(a: Point, b: Point, c: Point): { center: Point; radius: number } | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]))
  const scale = Math.max(dist(a, b), dist(b, c), dist(a, c))
  if (!(scale > 0) || Math.abs(d) < 1e-12 * scale * scale) return null
  const a2 = a[0] * a[0] + a[1] * a[1]
  const b2 = b[0] * b[0] + b[1] * b[1]
  const c2 = c[0] * c[0] + c[1] * c[1]
  const ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d
  const uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d
  const center: Point = [ux, uy]
  return { center, radius: dist(center, a) }
}

/** Arc from `start` through `mid` to `end`. */
export function arcThrough(start: Point, mid: Point, end: Point): Extract<Shape, { type: 'arc' }> | null {
  const c = circumcircle(start, mid, end)
  if (!c) return null
  const a0 = angleOf(c.center, start)
  const am = angleOf(c.center, mid)
  const a2 = angleOf(c.center, end)
  const ccw = normAngle(a2 - a0)
  const toMid = normAngle(am - a0)
  const sweep = toMid <= ccw ? ccw : -(TAU - ccw)
  return { type: 'arc', center: c.center, radius: c.radius, start: a0, sweep }
}

export function rectFrom(a: Point, b: Point): Extract<Shape, { type: 'rect' }> {
  return {
    type: 'rect',
    origin: [Math.min(a[0], b[0]), Math.min(a[1], b[1])],
    width: Math.abs(b[0] - a[0]),
    height: Math.abs(b[1] - a[1]),
  }
}

/** Parse typed values: `1200`, `600,400`, `600 400`, `-45`. */
export function parseValues(text: string): number[] | null {
  const parts = text
    .trim()
    .split(/[,;\s]+/)
    .filter((s) => s.length > 0)
  if (parts.length === 0) return null
  const out: number[] = []
  for (const p of parts) {
    const v = Number(p)
    if (!Number.isFinite(v)) return null
    out.push(v)
  }
  return out
}
