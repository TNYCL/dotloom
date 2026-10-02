/** Selection, direct manipulation (move / grip drag) and panning. */

import { panBy } from '../../camera.js'
import { DotloomError } from '../../protocol.js'
import type { Aabb, EntityId, Point } from '../../types.js'
import { dist } from '../geom.js'
import type { CancelReason, Tool, ToolContext, ToolPointer } from '../types.js'

/** Screen distance before a press becomes a drag. */
export const DRAG_THRESHOLD_PX = 4
/** Pick radius in CSS pixels. */
export const PICK_RADIUS_PX = 6

type Mode =
  | { kind: 'idle' }
  | { kind: 'press'; at: ToolPointer; hit: EntityId | null; grip: string | null; additive: boolean }
  | { kind: 'marquee'; from: ToolPointer; additive: boolean }
  | { kind: 'drag'; ids: EntityId[] }

function rectOf(a: Point, b: Point): Aabb {
  return { min: [Math.min(a[0], b[0]), Math.min(a[1], b[1])], max: [Math.max(a[0], b[0]), Math.max(a[1], b[1])] }
}

export class SelectTool implements Tool {
  readonly id = 'select'
  readonly label = 'tool.select'
  readonly shortcut = 'v'
  readonly cursor = 'default'
  private mode: Mode = { kind: 'idle' }
  private gripsFor = ''
  private grips: { name: string; point: Point; moves: boolean }[] = []

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.select.prompt')
    void this.refreshGrips(ctx)
  }

  deactivate(ctx: ToolContext): void {
    ctx.setMarkers([])
    ctx.viewport.setHover(null)
    this.gripsFor = ''
  }

  private async refreshGrips(ctx: ToolContext): Promise<void> {
    const sel = ctx.selection()
    const key = sel.join(',')
    if (key === this.gripsFor) return
    this.gripsFor = key
    this.grips = []
    if (sel.length === 1 && sel[0] !== undefined) {
      try {
        const info = await ctx.engine.entityInfo(sel[0])
        if (!info.readOnly && !info.entity.locked) {
          // Midpoint/center grips move the whole entity (CAD convention); others reshape it.
          this.grips = info.anchors.map((a) => ({
            name: a.name,
            point: a.point,
            moves: a.kind === 'midpoint' || a.kind === 'center' || a.kind === 'centroid',
          }))
        }
      } catch {
        this.grips = []
      }
    }
    ctx.setMarkers(this.grips.map((g) => ({ at: g.point, kind: 'handle' as const })))
  }

  private gripAt(ctx: ToolContext, p: ToolPointer): { name: string; moves: boolean } | null {
    const r = ctx.pxToWorld(PICK_RADIUS_PX + 2)
    let best: { name: string; moves: boolean } | null = null
    let bd = Number.POSITIVE_INFINITY
    for (const g of this.grips) {
      const d = dist(g.point, p.world)
      if (d <= r && d < bd) {
        bd = d
        best = { name: g.name, moves: g.moves }
      }
    }
    return best
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return
    await this.refreshGrips(ctx)
    const g = this.gripAt(ctx, p)
    const grip = g && !g.moves ? g.name : null
    const hits = grip ? [] : await ctx.engine.hitTest(p.world, ctx.pxToWorld(PICK_RADIUS_PX))
    const hit = hits[0]?.entity ?? (g?.moves ? (ctx.selection()[0] ?? null) : null)
    const additive = p.shift || p.mod
    const sel = ctx.selection()
    if (hit !== null && !grip) {
      if (additive) {
        const next = sel.includes(hit) ? sel.filter((x) => x !== hit) : [...sel, hit]
        await ctx.setSelection(next)
      } else if (!sel.includes(hit)) {
        await ctx.setSelection([hit])
      }
    }
    this.mode = { kind: 'press', at: p, hit, grip, additive }
    ctx.setState('press')
  }

  async pointerMove(ctx: ToolContext, p: ToolPointer): Promise<void> {
    const m = this.mode
    switch (m.kind) {
      case 'idle': {
        await this.refreshGrips(ctx)
        const hits = await ctx.engine.hitTest(p.world, ctx.pxToWorld(PICK_RADIUS_PX))
        ctx.viewport.setHover(hits[0]?.entity ?? null)
        return
      }
      case 'press': {
        if (dist(m.at.screen, p.screen) < DRAG_THRESHOLD_PX) return
        if (m.grip) {
          const id = ctx.selection()[0]
          if (id === undefined) return
          ctx.setSnapExclude([id])
          await ctx.engine.beginDrag({ kind: 'anchor', entity: id, anchor: m.grip })
          this.mode = { kind: 'drag', ids: [id] }
        } else if (m.hit !== null && !m.additive) {
          const ids = ctx.selection()
          ctx.setSnapExclude(ids)
          await ctx.engine.beginDrag({ kind: 'move', ids, from: m.at.point })
          this.mode = { kind: 'drag', ids }
        } else {
          this.mode = { kind: 'marquee', from: m.at, additive: m.additive }
          ctx.setState('marquee')
          ctx.viewport.setHover(null)
          this.drawMarquee(ctx, m.at, p)
          return
        }
        ctx.setState('drag')
        ctx.viewport.setHover(null)
        await this.dragTo(ctx, p)
        return
      }
      case 'drag':
        await this.dragTo(ctx, p)
        return
      case 'marquee':
        this.drawMarquee(ctx, m.from, p)
        return
    }
  }

  private async dragTo(ctx: ToolContext, p: ToolPointer): Promise<void> {
    const r = await ctx.engine.dragTo(p.point)
    if (r) ctx.setPrompt(r.accepted ? 'tool.select.dragging' : 'tool.select.dragRejected')
  }

  private drawMarquee(ctx: ToolContext, from: ToolPointer, p: ToolPointer): void {
    // Right-to-left = crossing (touching), left-to-right = window (inside).
    ctx.setOverlay({ marquee: rectOf(from.world, p.world), crossing: p.screen[0] < from.screen[0] })
  }

  async pointerUp(ctx: ToolContext, p: ToolPointer): Promise<void> {
    const m = this.mode
    this.mode = { kind: 'idle' }
    ctx.setState('idle')
    ctx.setSnapExclude([])
    switch (m.kind) {
      case 'press':
        if (m.hit === null && !m.grip && !m.additive) await ctx.setSelection([])
        else if (m.hit !== null && !m.additive && ctx.selection().length > 1) await ctx.setSelection([m.hit])
        break
      case 'drag':
        try {
          const r = await ctx.engine.endDrag(true)
          if (r) ctx.setPrompt('tool.select.prompt')
        } catch (e) {
          ctx.report(e)
          await ctx.engine.endDrag(false).catch(() => null)
        }
        break
      case 'marquee': {
        ctx.setOverlay({})
        const crossing = p.screen[0] < m.from.screen[0]
        const r = rectOf(m.from.world, p.world)
        const ids = await ctx.engine.selectInRect(r.min, r.max, crossing)
        const base = m.additive ? ctx.selection() : []
        await ctx.setSelection([...new Set([...base, ...ids])])
        break
      }
      case 'idle':
        break
    }
    this.gripsFor = ''
    await this.refreshGrips(ctx)
  }

  async cancel(ctx: ToolContext, _reason: CancelReason): Promise<void> {
    const m = this.mode
    this.mode = { kind: 'idle' }
    ctx.setState('idle')
    ctx.setOverlay({})
    ctx.setSnapExclude([])
    if (m.kind === 'drag') {
      try {
        await ctx.engine.endDrag(false)
      } catch (e) {
        if (!(e instanceof DotloomError && e.code === 'notActive')) ctx.report(e)
      }
    }
  }
}

export class PanTool implements Tool {
  readonly id = 'pan'
  readonly label = 'tool.pan'
  readonly shortcut = 'h'
  readonly cursor = 'grab'
  readonly snaps = false
  private last: Point | null = null

  pointerDown(ctx: ToolContext, p: ToolPointer): void {
    this.last = p.screen
    ctx.setState('drag')
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    if (!this.last) return
    const v = ctx.viewport
    v.setCamera(panBy(v.camera, p.screen[0] - this.last[0], p.screen[1] - this.last[1]))
    this.last = p.screen
  }

  pointerUp(ctx: ToolContext): void {
    this.last = null
    ctx.setState('idle')
  }

  cancel(ctx: ToolContext): void {
    this.last = null
    ctx.setState('idle')
  }
}
