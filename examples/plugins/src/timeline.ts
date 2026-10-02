/**
 * Timeline: blocks with start, duration (end = start + duration) and lanes.
 * Time maps to drawing coordinates through the document's time axis
 * (`settings.timeAxis`: origin in seconds and millimetres per second).
 * Ordering, equal durations, minimum gaps and locked times are engine rules.
 */

import type {
  ConstraintSpec,
  DotloomEngine,
  DotloomPlugin,
  EntityId,
  EntityTypeDef,
  Tool,
  ToolContext,
  ToolPointer,
} from '@dotloom/sdk'

export const blockType: EntityTypeDef = {
  typeId: 'timeline.block',
  version: 1,
  label: 'Timeline block',
  props: {
    start: { type: 'number', dim: 'time', default: '0h', label: 'Start' },
    duration: { type: 'number', dim: 'time', default: '1h', min: '5min', label: 'Duration' },
    lane: { type: 'number', dim: 'scalar', default: 0, solve: false, label: 'Lane' },
    title: { type: 'text', default: 'Task', maxLen: 200, label: 'Title' },
  },
  derived: [
    { name: 'x0', expr: '(start - axis.origin) * axis.mmPerSecond' },
    { name: 'x1', expr: '(start + duration - axis.origin) * axis.mmPerSecond' },
    { name: 'y0', expr: 'lane * -120mm' },
  ],
  anchors: [
    { name: 'begin', expr: 'vec(x0, y0)', kind: 'endpoint' },
    { name: 'finish', expr: 'vec(x1, y0)', kind: 'endpoint' },
  ],
  primitives: [
    {
      kind: 'polygon',
      points: ['vec(x0, y0)', 'vec(x1, y0)', 'vec(x1, y0 - 100mm)', 'vec(x0, y0 - 100mm)'],
      style: { fill: '#3b82f640' },
    },
    { kind: 'text', position: 'vec(x0 + 10mm, y0 - 50mm)', content: '{title}', height: '30mm', valign: 'middle' },
  ],
}

const LANE_MM = 120

/** `b` starts at least `gap` seconds after `a` ends. */
export function sequenceRule(a: EntityId, b: EntityId, gap = 0): ConstraintSpec {
  return {
    rule: {
      kind: 'linear',
      terms: [
        { coef: 1, param: { entity: b, prop: 'start' } },
        { coef: -1, param: { entity: a, prop: 'start' } },
        { coef: -1, param: { entity: a, prop: 'duration' } },
      ],
      op: '>=',
      rhs: gap,
    },
    label:
      gap > 0 ? `starts ≥ ${Math.round(gap / 60)} min after the previous block` : 'starts after the previous block',
  }
}

export function equalDurationRule(a: EntityId, b: EntityId): ConstraintSpec {
  return {
    rule: { kind: 'equal', a: { entity: a, prop: 'duration' }, b: { entity: b, prop: 'duration' } },
    label: 'equal duration',
  }
}

export function lockStartRule(id: EntityId, start: number): ConstraintSpec {
  return { rule: { kind: 'fix', param: { entity: id, prop: 'start' }, value: start }, label: 'start time locked' }
}

interface Axis {
  origin: number
  mmPerSecond: number
}

async function axisOf(engine: DotloomEngine): Promise<Axis> {
  const doc = await engine.documentJson()
  const a = doc.settings?.timeAxis
  return { origin: a?.origin_s ?? 0, mmPerSecond: a?.mm_per_second ?? 1 / 36 }
}

/** Press and drag horizontally to create a block in a lane. */
class BlockTool implements Tool {
  readonly id = 'timeline.block'
  readonly label = 'Block'
  readonly shortcut = 'k'
  readonly cursor = 'crosshair'
  private from: { x: number; lane: number } | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('Block: drag along a lane to create a block.')
  }

  pointerDown(ctx: ToolContext, p: ToolPointer): void {
    if (p.button !== 0) return
    this.from = { x: p.point[0], lane: Math.max(0, Math.round(-p.world[1] / LANE_MM)) }
    ctx.setState('preview')
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    if (!this.from) return
    const y = -this.from.lane * LANE_MM
    ctx.setOverlay({
      sketch: [
        {
          type: 'rect',
          origin: [Math.min(this.from.x, p.point[0]), y - 100],
          width: Math.abs(p.point[0] - this.from.x),
          height: 100,
        },
      ],
    })
  }

  async pointerUp(ctx: ToolContext, p: ToolPointer): Promise<void> {
    const f = this.from
    this.from = null
    ctx.setOverlay({})
    ctx.setState('idle')
    if (!f) return
    const axis = await axisOf(ctx.engine)
    const x0 = Math.min(f.x, p.point[0])
    const x1 = Math.max(f.x, p.point[0])
    const start = axis.origin + x0 / axis.mmPerSecond
    const duration = Math.max(300, (x1 - x0) / axis.mmPerSecond)
    await ctx.apply({
      label: 'Block',
      commands: [{ op: 'createEntity', entity: { type: 'timeline.block', props: { start, duration, lane: f.lane } } }],
    })
  }

  cancel(ctx: ToolContext): void {
    this.from = null
    ctx.setOverlay({})
    ctx.setState('idle')
  }
}

/**
 * Move blocks in time: drag a block; its start follows the pointer horizontally
 * (lanes stay). Ordering rules push later blocks; locked starts are never moved —
 * the drag stops at the nearest allowed time instead.
 */
class TimeMoveTool implements Tool {
  readonly id = 'timeline.move'
  readonly label = 'Move in time'
  readonly shortcut = 'j'
  readonly cursor = 'ew-resize'
  readonly snaps = false
  private drag: { dx: number; y: number } | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('Move in time: drag a block left or right.')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return
    const hits = await ctx.engine.hitTest(p.world, ctx.pxToWorld(6))
    for (const h of hits) {
      const info = await ctx.engine.entityInfo(h.entity)
      if (info.entity.type !== 'timeline.block') continue
      const begin = info.anchors.find((a) => a.name === 'begin')?.point
      if (!begin) continue
      await ctx.engine.beginDrag({ kind: 'anchor', entity: h.entity, anchor: 'begin' })
      this.drag = { dx: begin[0] - p.world[0], y: begin[1] }
      ctx.setState('drag')
      return
    }
  }

  async pointerMove(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (!this.drag) return
    const r = await ctx.engine.dragTo([p.world[0] + this.drag.dx, this.drag.y])
    if (r)
      ctx.setPrompt(r.accepted ? 'Release to apply.' : 'A rule blocks this time; showing the nearest allowed time.')
  }

  async pointerUp(ctx: ToolContext): Promise<void> {
    if (!this.drag) return
    this.drag = null
    ctx.setState('idle')
    try {
      await ctx.engine.endDrag(true)
    } catch (e) {
      ctx.report(e)
      await ctx.engine.endDrag(false).catch(() => null)
    }
  }

  async cancel(ctx: ToolContext): Promise<void> {
    if (this.drag) await ctx.engine.endDrag(false).catch(() => null)
    this.drag = null
    ctx.setState('idle')
  }
}

export const timelinePlugin: DotloomPlugin = {
  id: 'timeline.blocks',
  version: '1.0.0',
  sdk: '^1.0.0',
  types: [blockType],
  tools: [() => new BlockTool(), () => new TimeMoveTool()],
  constraintTemplates: [
    {
      id: 'sequence',
      label: 'Second starts after first',
      arity: 2,
      build: ([a, b]) => (a !== undefined && b !== undefined ? [sequenceRule(a, b)] : []),
    },
    {
      id: 'gap15',
      label: 'At least 15 min apart',
      arity: 2,
      build: ([a, b]) => (a !== undefined && b !== undefined ? [sequenceRule(a, b, 900)] : []),
    },
    {
      id: 'equalDuration',
      label: 'Equal duration',
      arity: 2,
      build: ([a, b]) => (a !== undefined && b !== undefined ? [equalDurationRule(a, b)] : []),
    },
  ],
}

const H = 3600

/** A small day plan: 08:00 origin, 100 mm per hour. */
export async function loadTimelineExample(
  engine: DotloomEngine,
): Promise<{ design: EntityId; build: EntityId; review: EntityId; release: EntityId }> {
  await engine.newDocument()
  const [design, build, review, release] = (await engine.reserveIds(4)) as [number, number, number, number]
  const block = (id: number, title: string, start: number, duration: number, lane: number) => ({
    op: 'createEntity' as const,
    id,
    entity: { type: 'timeline.block', name: title, props: { title, start, duration, lane } },
  })
  await engine.apply({
    label: 'Timeline example',
    commands: [
      { op: 'setSettings', patch: { title: 'zaman-cizelgesi', timeAxis: { origin_s: 8 * H, mm_per_second: 100 / H } } },
      block(design, 'Design', 9 * H, 2 * H, 0),
      block(build, 'Build', 11 * H, 3 * H, 0),
      block(review, 'Review', 14.5 * H, 1 * H, 1),
      block(release, 'Release', 16 * H, 1 * H, 1),
      { op: 'addConstraint', constraint: sequenceRule(design, build) },
      { op: 'addConstraint', constraint: sequenceRule(build, review, 30 * 60) },
      { op: 'addConstraint', constraint: sequenceRule(review, release) },
      { op: 'addConstraint', constraint: equalDurationRule(review, release) },
      { op: 'addConstraint', constraint: lockStartRule(release, 16 * H) },
    ],
  })
  return { design, build, review, release }
}
