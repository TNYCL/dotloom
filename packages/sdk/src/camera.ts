/**
 * Camera math (pure, DOM-free). Mirrors `dotloom_render::View`:
 * world coordinates are model units (mm, Y up); screen coordinates are CSS pixels
 * relative to the canvas' top-left corner (Y down). Device pixels are only used by
 * the renderer, so DOM coordinates never need DPR corrections (no double-DPI).
 */

import type { Aabb, Point } from './types.js'

/** Zoom limits (CSS px per model unit). */
export const MIN_SCALE = 1e-6
export const MAX_SCALE = 1e6

export interface Camera {
  /** World point at the canvas center. */
  center: Point
  /** CSS pixels per model unit. */
  scale: number
}

export interface ViewSize {
  /** CSS pixels. */
  width: number
  /** CSS pixels. */
  height: number
  /** Device pixels per CSS pixel (the canvas' actual ratio). */
  dpr: number
}

export const DEFAULT_CAMERA: Camera = { center: [0, 0], scale: 1 }

function clampScale(s: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, s))
}

export function isValidCamera(c: Camera): boolean {
  return (
    Number.isFinite(c.center[0]) &&
    Number.isFinite(c.center[1]) &&
    Number.isFinite(c.scale) &&
    c.scale >= MIN_SCALE &&
    c.scale <= MAX_SCALE
  )
}

/** World → CSS pixels. */
export function worldToScreen(c: Camera, size: ViewSize, p: Point): Point {
  return [(p[0] - c.center[0]) * c.scale + size.width / 2, size.height / 2 - (p[1] - c.center[1]) * c.scale]
}

/** CSS pixels → world. */
export function screenToWorld(c: Camera, size: ViewSize, x: number, y: number): Point {
  return [c.center[0] + (x - size.width / 2) / c.scale, c.center[1] - (y - size.height / 2) / c.scale]
}

/** Convert a CSS pixel length to model units. */
export function pxToWorld(c: Camera, px: number): number {
  return px / c.scale
}

/** Pan by a CSS pixel delta (content follows the pointer). */
export function panBy(c: Camera, dx: number, dy: number): Camera {
  return { center: [c.center[0] - dx / c.scale, c.center[1] + dy / c.scale], scale: c.scale }
}

/** Zoom by `factor`, keeping the world point under CSS position (x, y) fixed. */
export function zoomAt(c: Camera, size: ViewSize, x: number, y: number, factor: number): Camera {
  if (!Number.isFinite(factor) || factor <= 0) return c
  const anchor = screenToWorld(c, size, x, y)
  const scale = clampScale(c.scale * factor)
  const moved = screenToWorld({ center: c.center, scale }, size, x, y)
  return { center: [c.center[0] + anchor[0] - moved[0], c.center[1] + anchor[1] - moved[1]], scale }
}

/** Camera showing `b` with `margin` CSS pixels around it. */
export function fitBounds(size: ViewSize, b: Aabb, margin = 24): Camera {
  const w = Math.max(1, size.width - 2 * margin)
  const h = Math.max(1, size.height - 2 * margin)
  const bw = Math.max(1e-9, b.max[0] - b.min[0])
  const bh = Math.max(1e-9, b.max[1] - b.min[1])
  const scale = clampScale(Math.min(w / bw, h / bh))
  return { center: [(b.min[0] + b.max[0]) / 2, (b.min[1] + b.max[1]) / 2], scale }
}

/** Visible world rectangle. */
export function visibleBounds(c: Camera, size: ViewSize): Aabb {
  const hw = size.width / 2 / c.scale
  const hh = size.height / 2 / c.scale
  return { min: [c.center[0] - hw, c.center[1] - hh], max: [c.center[0] + hw, c.center[1] + hh] }
}

/** Wheel delta → zoom factor (normalizes pixel/line/page modes). */
export function wheelZoomFactor(deltaY: number, deltaMode: number): number {
  const px = deltaMode === 1 ? deltaY * 16 : deltaMode === 2 ? deltaY * 400 : deltaY
  return Math.exp(-px * 0.0015)
}
