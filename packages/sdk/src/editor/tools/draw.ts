/** Drawing tools: line, polyline, rectangle, circle, arc, path, text, dimension. */

import type { AnchorRef, PathEl, Point, PropValue, Shape } from '../../types.js'
import { add, angleOf, arcThrough, dist, parseValues, rectFrom, sub } from '../geom.js'
import type { CancelReason, InputKey, Tool, ToolContext, ToolPointer } from '../types.js'

const CLOSE_PX = 8

/** Constrain to horizontal/vertical from `from` (Shift). */
function ortho(from: Point, p: Point): Point {
  const dx = Math.abs(p[0] - from[0])
  const dy = Math.abs(p[1] - from[1])
  return dx >= dy ? [p[0], from[1]] : [from[0], p[1]]
}

function pointOf(from: Point | null, p: ToolPointer): Point {
  return from && p.shift ? ortho(from, p.point) : p.point
}

async function create(ctx: ToolContext, label: string, geometry: Shape): Promise<boolean> {
  const r = await ctx.apply({ label, commands: [{ op: 'createEntity', entity: { geometry } }] })
  return r !== null
}

/** Line: click start, click end; continues from the end until Escape/right-click. */
export class LineTool implements Tool {
  readonly id = 'line'
  readonly label = 'tool.line'
  readonly shortcut = 'l'
  readonly cursor = 'crosshair'
  private start: Point | null = null
  private last: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.line.start')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button === 2) return this.cancel(ctx)
    if (p.button !== 0) return
    if (!this.start) {
      this.start = p.point
      ctx.setState('preview')
      ctx.setPrompt('tool.line.end')
      return
    }
    await this.finish(ctx, pointOf(this.start, p))
  }

  private async finish(ctx: ToolContext, end: Point): Promise<void> {
    const start = this.start
    if (!start || dist(start, end) <= 0) return
    if (await create(ctx, 'Line', { type: 'line', a: start, b: end })) {
      this.start = end
      ctx.setOverlay({})
    }
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    this.last = p.point
    if (!this.start) return
    ctx.setOverlay({ sketch: [{ type: 'line', a: this.start, b: pointOf(this.start, p) }] })
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    if (!v || !this.start || v[0] === undefined || v[0] <= 0) return false
    const len = v[0] * ctx.unitScale()
    const angle = v[1] !== undefined ? (v[1] * Math.PI) / 180 : this.last ? angleOf(this.start, this.last) : 0
    await this.finish(ctx, [this.start[0] + len * Math.cos(angle), this.start[1] + len * Math.sin(angle)])
    return true
  }

  cancel(ctx: ToolContext): void {
    this.start = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.line.start')
  }
}

/** Polyline: click vertices; double-click/Enter finishes; click the first point to close. */
export class PolylineTool implements Tool {
  readonly id = 'polyline'
  readonly label = 'tool.polyline'
  readonly shortcut = 'p'
  readonly cursor = 'crosshair'
  private pts: Point[] = []
  private last: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.polyline.start')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button === 2) return this.finish(ctx, false)
    if (p.button !== 0) return
    const first = this.pts[0]
    const prev = this.pts[this.pts.length - 1] ?? null
    const pt = pointOf(prev, p)
    if (first && this.pts.length >= 3 && dist(first, pt) <= ctx.pxToWorld(CLOSE_PX)) {
      return this.finish(ctx, true)
    }
    // Ignore the duplicate press of a double-click.
    if (prev && dist(prev, pt) <= ctx.pxToWorld(1)) return
    this.pts.push(pt)
    ctx.setState('preview')
    ctx.setPrompt('tool.polyline.next')
    this.preview(ctx, pt)
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    this.last = p.point
    if (this.pts.length > 0) this.preview(ctx, pointOf(this.pts[this.pts.length - 1] ?? null, p))
  }

  private preview(ctx: ToolContext, cur: Point): void {
    ctx.setOverlay({ sketch: [{ type: 'polyline', points: [...this.pts, cur] }] })
  }

  doubleClick(ctx: ToolContext): Promise<void> {
    return this.finish(ctx, false)
  }

  async key(ctx: ToolContext, k: InputKey): Promise<boolean> {
    if (k.key === 'Enter') {
      await this.finish(ctx, false)
      return true
    }
    if (k.key === 'Backspace' && this.pts.length > 0) {
      this.pts.pop()
      if (this.pts.length === 0) this.cancel(ctx)
      else if (this.last) this.preview(ctx, this.last)
      return true
    }
    return false
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    const prev = this.pts[this.pts.length - 1]
    if (!v || !prev || v[0] === undefined || v[0] <= 0) return false
    const len = v[0] * ctx.unitScale()
    const angle = v[1] !== undefined ? (v[1] * Math.PI) / 180 : this.last ? angleOf(prev, this.last) : 0
    this.pts.push([prev[0] + len * Math.cos(angle), prev[1] + len * Math.sin(angle)])
    if (this.last) this.preview(ctx, this.last)
    return true
  }

  private async finish(ctx: ToolContext, closed: boolean): Promise<void> {
    const pts = this.pts
    this.cancel(ctx)
    if (pts.length < 2) return
    await create(ctx, 'Polyline', { type: 'polyline', points: pts, closed })
  }

  async cancel(ctx: ToolContext, reason?: CancelReason): Promise<void> {
    // Escape finishes an open polyline with the points placed so far.
    if (reason === 'escape' && this.pts.length >= 2) return this.finish(ctx, false)
    this.pts = []
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.polyline.start')
  }
}

/** Rectangle: two corners (click-click or drag). Typed `width,height`. */
export class RectTool implements Tool {
  readonly id = 'rect'
  readonly label = 'tool.rect'
  readonly shortcut = 'r'
  readonly cursor = 'crosshair'
  private a: Point | null = null
  private downScreen: Point | null = null
  private last: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.rect.start')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    if (!this.a) {
      this.a = p.point
      this.downScreen = p.screen
      ctx.setState('preview')
      ctx.setPrompt('tool.rect.end')
      return
    }
    await this.finish(ctx, p.point)
  }

  async pointerUp(ctx: ToolContext, p: ToolPointer): Promise<void> {
    // Press-drag-release draws in one gesture.
    if (this.a && this.downScreen && dist(this.downScreen, p.screen) > 4) await this.finish(ctx, p.point)
    this.downScreen = null
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    this.last = p.point
    if (this.a) ctx.setOverlay({ sketch: [rectFrom(this.a, p.point)] })
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    if (!v || !this.a || v[0] === undefined) return false
    const w = v[0] * ctx.unitScale()
    const h = (v[1] ?? v[0]) * ctx.unitScale()
    if (!(w > 0 && h > 0)) return false
    const sx = this.last && this.last[0] < this.a[0] ? -1 : 1
    const sy = this.last && this.last[1] < this.a[1] ? -1 : 1
    await this.finish(ctx, [this.a[0] + sx * w, this.a[1] + sy * h])
    return true
  }

  private async finish(ctx: ToolContext, b: Point): Promise<void> {
    const a = this.a
    this.cancel(ctx)
    if (!a) return
    const r = rectFrom(a, b)
    if (r.width > 0 && r.height > 0) await create(ctx, 'Rectangle', r)
  }

  cancel(ctx: ToolContext): void {
    this.a = null
    this.downScreen = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.rect.start')
  }
}

/** Circle: center, then a point on the circle. Typed radius. */
export class CircleTool implements Tool {
  readonly id = 'circle'
  readonly label = 'tool.circle'
  readonly shortcut = 'c'
  readonly cursor = 'crosshair'
  private c: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.circle.center')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    if (!this.c) {
      this.c = p.point
      ctx.setState('preview')
      ctx.setPrompt('tool.circle.radius')
      return
    }
    await this.finish(ctx, dist(this.c, p.point))
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    if (!this.c) return
    const r = dist(this.c, p.point)
    if (r > 0) ctx.setOverlay({ sketch: [{ type: 'circle', center: this.c, radius: r }] })
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    if (!v || !this.c || v[0] === undefined || v[0] <= 0) return false
    await this.finish(ctx, v[0] * ctx.unitScale())
    return true
  }

  private async finish(ctx: ToolContext, r: number): Promise<void> {
    const c = this.c
    this.cancel(ctx)
    if (c && r > 0) await create(ctx, 'Circle', { type: 'circle', center: c, radius: r })
  }

  cancel(ctx: ToolContext): void {
    this.c = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.circle.center')
  }
}

/** Arc through three points: start, a point on the arc, end. */
export class ArcTool implements Tool {
  readonly id = 'arc'
  readonly label = 'tool.arc'
  readonly shortcut = 'a'
  readonly cursor = 'crosshair'
  private pts: Point[] = []

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.arc.start')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    const prev = this.pts[this.pts.length - 1]
    if (prev && dist(prev, p.point) <= ctx.pxToWorld(1)) return
    this.pts.push(p.point)
    ctx.setState('preview')
    if (this.pts.length === 1) ctx.setPrompt('tool.arc.mid')
    if (this.pts.length === 2) ctx.setPrompt('tool.arc.end')
    if (this.pts.length === 3) {
      const [a, m, b] = this.pts as [Point, Point, Point]
      this.cancel(ctx)
      const arc = arcThrough(a, m, b)
      if (!arc) {
        ctx.report(new Error('the three points are collinear'))
        return
      }
      await create(ctx, 'Arc', arc)
    }
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    const [a, m] = this.pts
    if (a && !m) ctx.setOverlay({ sketch: [{ type: 'line', a, b: p.point }] })
    if (a && m) {
      const arc = arcThrough(a, m, p.point)
      ctx.setOverlay({ sketch: arc ? [arc] : [{ type: 'polyline', points: [a, m, p.point] }] })
    }
  }

  cancel(ctx: ToolContext): void {
    this.pts = []
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.arc.start')
  }
}

/**
 * Path: click for straight segments; press-and-drag to pull a smooth cubic
 * handle; click the first point to close; Enter or double-click to finish.
 */
export class PathTool implements Tool {
  readonly id = 'path'
  readonly label = 'tool.path'
  readonly shortcut = 'b'
  readonly cursor = 'crosshair'
  private els: PathEl[] = []
  private first: Point | null = null
  private lastPt: Point | null = null
  /** Outgoing handle of the last point (mirrors the drag). */
  private outHandle: Point | null = null
  private down: { at: Point; screen: Point } | null = null
  private hover: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.path.start')
  }

  private segmentTo(end: Point, inHandle: Point | null): PathEl {
    const from = this.lastPt
    if (from && (this.outHandle || inHandle)) {
      return { C: [this.outHandle ?? from, inHandle ?? end, end] }
    }
    return { L: end }
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.finish(ctx, false)
    if (this.first && this.els.length >= 2 && dist(this.first, p.point) <= ctx.pxToWorld(CLOSE_PX)) {
      return this.finish(ctx, true)
    }
    if (this.lastPt && dist(this.lastPt, p.point) <= ctx.pxToWorld(1)) return
    this.down = { at: p.point, screen: p.screen }
    ctx.setState('preview')
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    this.hover = p.point
    this.preview(ctx)
  }

  pointerUp(ctx: ToolContext, p: ToolPointer): void {
    const d = this.down
    this.down = null
    if (!d) return
    const dragged = dist(d.screen, p.screen) > 4
    const handle = dragged ? p.point : null
    // Mirror the drag for the incoming handle so the curve is smooth.
    const inHandle = handle ? sub(d.at, sub(handle, d.at)) : null
    if (!this.first) {
      this.first = d.at
      this.els.push({ M: d.at })
    } else {
      this.els.push(this.segmentTo(d.at, inHandle))
    }
    this.lastPt = d.at
    this.outHandle = handle
    ctx.setPrompt('tool.path.next')
    this.preview(ctx)
  }

  private preview(ctx: ToolContext): void {
    if (!this.first) return
    const els: PathEl[] = [...this.els]
    const cur = this.down ? this.down.at : this.hover
    if (cur && this.lastPt && dist(cur, this.lastPt) > 0) els.push(this.segmentTo(cur, null))
    const sketch: Shape[] = els.length > 1 ? [{ type: 'path', elements: els }] : []
    const guides =
      this.outHandle && this.lastPt
        ? [{ a: sub(this.lastPt, sub(this.outHandle, this.lastPt)), b: this.outHandle }]
        : []
    ctx.setOverlay({ sketch, guides })
  }

  doubleClick(ctx: ToolContext): Promise<void> {
    return this.finish(ctx, false)
  }

  async key(ctx: ToolContext, k: InputKey): Promise<boolean> {
    if (k.key === 'Enter') {
      await this.finish(ctx, false)
      return true
    }
    return false
  }

  private async finish(ctx: ToolContext, closed: boolean): Promise<void> {
    const els = [...this.els]
    if (closed && this.first) {
      els.push(this.segmentTo(this.first, null))
      els.push('Z')
    }
    this.cancel(ctx)
    if (els.length >= 2) await create(ctx, 'Path', { type: 'path', elements: els })
  }

  cancel(ctx: ToolContext): void {
    this.els = []
    this.first = null
    this.lastPt = null
    this.outHandle = null
    this.down = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.path.start')
  }
}

/** Text: click the insertion point, type, Enter to place. */
export class TextTool implements Tool {
  readonly id = 'text'
  readonly label = 'tool.text'
  readonly shortcut = 't'
  readonly cursor = 'text'
  private at: Point | null = null
  private text = ''
  /** Text height in CSS pixels at the current zoom when placed. */
  constructor(private readonly heightPx = 16) {}

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.text.position')
  }

  capturesKeyboard(): boolean {
    return this.at !== null
  }

  pointerDown(ctx: ToolContext, p: ToolPointer): void {
    if (p.button !== 0) {
      this.cancel(ctx)
      return
    }
    this.at = p.point
    this.text = ''
    ctx.setState('typing')
    ctx.setPrompt('tool.text.type')
    this.preview(ctx)
  }

  private height(ctx: ToolContext): number {
    return ctx.pxToWorld(this.heightPx)
  }

  private preview(ctx: ToolContext): void {
    if (!this.at) return
    const marker = { at: this.at, kind: 'anchor' as const }
    ctx.setOverlay({
      markers: [marker],
      sketch: this.text ? [{ type: 'text', position: this.at, content: this.text, height: this.height(ctx) }] : [],
    })
  }

  async key(ctx: ToolContext, k: InputKey): Promise<boolean> {
    if (!this.at) return false
    if (k.key === 'Escape') {
      this.cancel(ctx)
    } else if (k.key === 'Enter' && k.shift) {
      this.text += '\n'
    } else if (k.key === 'Enter') {
      const at = this.at
      const content = this.text
      const height = this.height(ctx)
      this.cancel(ctx)
      if (content.trim()) await create(ctx, 'Text', { type: 'text', position: at, content, height })
    } else if (k.key === 'Backspace') {
      this.text = [...this.text].slice(0, -1).join('')
    } else if (k.key.length === 1 && !k.mod) {
      this.text += k.key
    } else {
      return false
    }
    this.preview(ctx)
    return true
  }

  cancel(ctx: ToolContext): void {
    this.at = null
    this.text = ''
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.text.position')
  }
}

/**
 * Linear dimension: two points (associative when snapped to an entity anchor),
 * then the position of the dimension line.
 */
export class DimensionTool implements Tool {
  readonly id = 'dimension'
  readonly label = 'tool.dimension'
  readonly shortcut = 'd'
  readonly cursor = 'crosshair'
  private refs: { point: Point; ref: PropValue }[] = []
  constructor(private readonly textPx = 12) {}

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.dimension.first')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) {
      this.cancel(ctx)
      return
    }
    if (this.refs.length < 2) {
      const s = p.snap
      const ref: PropValue =
        s && s.entity !== null && s.anchor !== null ? ({ entity: s.entity, anchor: s.anchor } as AnchorRef) : p.point
      this.refs.push({ point: p.point, ref })
      ctx.setState('preview')
      ctx.setPrompt(this.refs.length === 1 ? 'tool.dimension.second' : 'tool.dimension.place')
      return
    }
    const [a, b] = this.refs as [{ point: Point; ref: PropValue }, { point: Point; ref: PropValue }]
    const textHeight = ctx.pxToWorld(this.textPx)
    this.cancel(ctx)
    await ctx.apply({
      label: 'Dimension',
      commands: [
        {
          op: 'createEntity',
          entity: {
            type: 'dotloom.dimension',
            props: { kind: 'linear', a: a.ref, b: b.ref, through: p.point, textHeight, orientation: 'aligned' },
          },
        },
      ],
    })
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    const [a, b] = this.refs
    if (a && !b) ctx.setOverlay({ guides: [{ a: a.point, b: p.point }] })
    if (a && b) {
      // Offset the measured segment through the pointer.
      const d = sub(b.point, a.point)
      const len = Math.hypot(d[0], d[1]) || 1
      const n: Point = [-d[1] / len, d[0] / len]
      const off = (p.point[0] - a.point[0]) * n[0] + (p.point[1] - a.point[1]) * n[1]
      const o: Point = [n[0] * off, n[1] * off]
      ctx.setOverlay({
        sketch: [{ type: 'line', a: add(a.point, o), b: add(b.point, o) }],
        guides: [
          { a: a.point, b: add(a.point, o) },
          { a: b.point, b: add(b.point, o) },
        ],
      })
    }
  }

  cancel(ctx: ToolContext): void {
    this.refs = []
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.dimension.first')
  }
}
