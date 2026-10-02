/** Modify tools: move, rotate, scale, split, trim, extend. */

import { visibleBounds } from '../../camera.js'
import type { Affine, EntityId, Point } from '../../types.js'
import { angleOf, dist, parseValues, rotateAbout, scaleAbout } from '../geom.js'
import type { Tool, ToolContext, ToolPointer } from '../types.js'
import { PICK_RADIUS_PX } from './select.js'

async function pick(ctx: ToolContext, p: ToolPointer): Promise<EntityId | null> {
  const hits = await ctx.engine.hitTest(p.world, ctx.pxToWorld(PICK_RADIUS_PX))
  return hits[0]?.entity ?? null
}

/** Ensure there is a selection; a click selects the entity under the pointer. */
async function needSelection(ctx: ToolContext, p: ToolPointer): Promise<boolean> {
  if (ctx.selection().length > 0) return true
  const id = await pick(ctx, p)
  if (id !== null) await ctx.setSelection([id])
  return false
}

async function transformSelection(ctx: ToolContext, label: string, t: Affine): Promise<void> {
  const ids = ctx.selection()
  if (ids.length === 0) return
  await ctx.apply({ label, commands: [{ op: 'transform', ids, transform: t, policy: 'convert' }] })
}

/** Move: base point, then destination (live preview through the engine drag). */
export class MoveTool implements Tool {
  readonly id = 'move'
  readonly label = 'tool.move'
  readonly shortcut = 'm'
  readonly cursor = 'move'
  private base: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt(ctx.selection().length > 0 ? 'tool.move.base' : 'tool.move.select')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    if (!(await needSelection(ctx, p))) {
      ctx.setPrompt('tool.move.base')
      return
    }
    if (!this.base) {
      this.base = p.point
      const ids = ctx.selection()
      ctx.setSnapExclude(ids)
      await ctx.engine.beginDrag({ kind: 'move', ids, from: p.point })
      ctx.setState('preview')
      ctx.setPrompt('tool.move.target')
      return
    }
    await this.commit(ctx, p.point)
  }

  async pointerMove(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (!this.base) return
    await ctx.engine.dragTo(p.point)
    ctx.setOverlay({ guides: [{ a: this.base, b: p.point }] })
  }

  private async commit(ctx: ToolContext, to: Point): Promise<void> {
    await ctx.engine.dragTo(to)
    try {
      await ctx.engine.endDrag(true)
    } catch (e) {
      ctx.report(e)
      await ctx.engine.endDrag(false).catch(() => null)
    }
    this.reset(ctx)
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    if (!v || !this.base || v[0] === undefined) return false
    const k = ctx.unitScale()
    const to: Point =
      v.length >= 2
        ? [this.base[0] + v[0] * k, this.base[1] + (v[1] ?? 0) * k]
        : [this.base[0] + v[0] * k, this.base[1]]
    await this.commit(ctx, to)
    return true
  }

  private reset(ctx: ToolContext): void {
    this.base = null
    ctx.setSnapExclude([])
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.move.base')
  }

  async cancel(ctx: ToolContext): Promise<void> {
    if (this.base) await ctx.engine.endDrag(false).catch(() => null)
    this.reset(ctx)
  }
}

/** Rotate: center, reference direction, target direction. Typed angle in degrees. */
export class RotateTool implements Tool {
  readonly id = 'rotate'
  readonly label = 'tool.rotate'
  readonly shortcut = 'o'
  readonly cursor = 'crosshair'
  private c: Point | null = null
  private ref: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt(ctx.selection().length > 0 ? 'tool.rotate.center' : 'tool.rotate.select')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    if (!(await needSelection(ctx, p))) {
      ctx.setPrompt('tool.rotate.center')
      return
    }
    if (!this.c) {
      this.c = p.point
      ctx.setState('preview')
      ctx.setPrompt('tool.rotate.reference')
      return
    }
    if (!this.ref) {
      if (dist(this.c, p.point) <= 0) return
      this.ref = p.point
      ctx.setPrompt('tool.rotate.target')
      return
    }
    const angle = angleOf(this.c, p.point) - angleOf(this.c, this.ref)
    const c = this.c
    this.cancel(ctx)
    await transformSelection(ctx, 'Rotate', rotateAbout(c, angle))
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    if (!this.c) return
    const guides = [{ a: this.c, b: p.point }]
    if (this.ref) guides.push({ a: this.c, b: this.ref })
    ctx.setOverlay({ guides })
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    if (!v || !this.c || v[0] === undefined) return false
    const c = this.c
    this.cancel(ctx)
    await transformSelection(ctx, 'Rotate', rotateAbout(c, (v[0] * Math.PI) / 180))
    return true
  }

  cancel(ctx: ToolContext): void {
    this.c = null
    this.ref = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.rotate.center')
  }
}

/** Scale: base point, reference distance, target distance. Typed factor. */
export class ScaleTool implements Tool {
  readonly id = 'scale'
  readonly label = 'tool.scale'
  readonly shortcut = 's'
  readonly cursor = 'crosshair'
  private base: Point | null = null
  private ref: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt(ctx.selection().length > 0 ? 'tool.scale.base' : 'tool.scale.select')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    if (!(await needSelection(ctx, p))) {
      ctx.setPrompt('tool.scale.base')
      return
    }
    if (!this.base) {
      this.base = p.point
      ctx.setState('preview')
      ctx.setPrompt('tool.scale.reference')
      return
    }
    if (!this.ref) {
      if (dist(this.base, p.point) <= 0) return
      this.ref = p.point
      ctx.setPrompt('tool.scale.target')
      return
    }
    const k = dist(this.base, p.point) / dist(this.base, this.ref)
    const base = this.base
    this.cancel(ctx)
    if (k > 0 && Number.isFinite(k)) await transformSelection(ctx, 'Scale', scaleAbout(base, k))
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    if (!this.base) return
    const guides = [{ a: this.base, b: p.point }]
    if (this.ref) guides.push({ a: this.base, b: this.ref })
    ctx.setOverlay({ guides })
  }

  async value(ctx: ToolContext, text: string): Promise<boolean> {
    const v = parseValues(text)
    if (!v || !this.base || v[0] === undefined || !(v[0] > 0)) return false
    const base = this.base
    this.cancel(ctx)
    await transformSelection(ctx, 'Scale', scaleAbout(base, v[0]))
    return true
  }

  cancel(ctx: ToolContext): void {
    this.base = null
    this.ref = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('tool.scale.base')
  }
}

/** Split: click a curve where it should be cut. */
export class SplitTool implements Tool {
  readonly id = 'split'
  readonly label = 'tool.split'
  readonly shortcut = 'k'
  readonly cursor = 'crosshair'

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.split.pick')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return
    const id = await pick(ctx, p)
    if (id === null) return
    await ctx.apply({ label: 'Split', commands: [{ op: 'split', id, at: p.point }] })
  }
}

async function others(ctx: ToolContext, around: EntityId): Promise<EntityId[]> {
  const sel = ctx.selection().filter((x) => x !== around)
  if (sel.length > 0) return sel
  const vb = visibleBounds(ctx.viewport.camera, ctx.viewport.size)
  const ids = await ctx.engine.selectInRect(vb.min, vb.max, true)
  return ids.filter((x) => x !== around)
}

/** Trim: click the piece to remove; cutting edges are the selection or everything visible. */
export class TrimTool implements Tool {
  readonly id = 'trim'
  readonly label = 'tool.trim'
  readonly shortcut = 'x'
  readonly cursor = 'crosshair'
  readonly snaps = false

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.trim.pick')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return
    const id = await pick(ctx, p)
    if (id === null) return
    const cutters = await others(ctx, id)
    await ctx.apply({ label: 'Trim', commands: [{ op: 'trim', id, cutters, pick: p.world }] })
  }
}

/** Extend: click near the end to extend; boundaries are the selection or everything visible. */
export class ExtendTool implements Tool {
  readonly id = 'extend'
  readonly label = 'tool.extend'
  readonly shortcut = 'e'
  readonly cursor = 'crosshair'
  readonly snaps = false

  activate(ctx: ToolContext): void {
    ctx.setPrompt('tool.extend.pick')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return
    const id = await pick(ctx, p)
    if (id === null) return
    const info = await ctx.engine.entityInfo(id)
    const start = info.anchors.find((a) => a.name === 'start')?.point
    const end = info.anchors.find((a) => a.name === 'end')?.point
    if (!start || !end) {
      ctx.report(new Error('only open curves can be extended'))
      return
    }
    const which = dist(p.world, start) < dist(p.world, end) ? 'start' : 'end'
    const boundaries = await others(ctx, id)
    await ctx.apply({ label: 'Extend', commands: [{ op: 'extend', id, end: which, boundaries }] })
  }
}
