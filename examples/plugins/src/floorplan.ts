/**
 * Room planner: walls and doors hosted on walls. Doors reference their wall; the
 * engine keeps them on it (or reports a conflict), deletes them with the wall and
 * preserves the reference through save/load. No BIM claims.
 */

import type {
  DotloomEngine,
  DotloomPlugin,
  EntityId,
  EntityTypeDef,
  Point,
  Tool,
  ToolContext,
  ToolPointer,
} from '@dotloom/sdk'
import { project } from './common.js'

export const wallType: EntityTypeDef = {
  typeId: 'floorplan.wall',
  version: 2,
  label: 'Wall',
  props: {
    start: { type: 'point', label: 'Start' },
    end: { type: 'point', default: [4000, 0], label: 'End' },
    thickness: { type: 'number', dim: 'length', default: '20cm', min: '5cm', label: 'Thickness' },
  },
  derived: [
    { name: 'dir', expr: 'norm(end - start)' },
    { name: 'side', expr: 'perp(dir) * (thickness / 2)' },
  ],
  anchors: [
    { name: 'start', expr: 'start', kind: 'endpoint' },
    { name: 'end', expr: 'end', kind: 'endpoint' },
    { name: 'mid', expr: 'lerp(start, end, 0.5)', kind: 'midpoint' },
  ],
  primitives: [
    {
      kind: 'polygon',
      points: ['start + side', 'end + side', 'end - side', 'start - side'],
      style: { fill: '#9aa5b180' },
    },
  ],
  constraints: [{ lhs: 'dist(start, end)', op: '>=', rhs: 'thickness', label: 'wall longer than it is thick' }],
  migrations: [{ from: 1, rename: { thick: 'thickness' } }],
}

export const doorType: EntityTypeDef = {
  typeId: 'floorplan.door',
  version: 1,
  label: 'Door',
  props: {
    host: { type: 'ref', target: 'floorplan.wall', onDelete: 'cascade', required: true, label: 'Wall' },
    offset: { type: 'number', dim: 'length', default: '50cm', stay: 'low', label: 'Offset from wall start' },
    width: { type: 'number', dim: 'length', default: '90cm', min: '60cm', max: '200cm', stay: 'high', label: 'Width' },
  },
  derived: [
    { name: 'dir', expr: 'norm(host.end - host.start)' },
    { name: 'hingePoint', expr: 'host.start + dir * offset' },
    { name: 'latchPoint', expr: 'host.start + dir * (offset + width)' },
  ],
  anchors: [
    { name: 'hinge', expr: 'hingePoint', kind: 'endpoint' },
    { name: 'latch', expr: 'latchPoint', kind: 'endpoint' },
    { name: 'mid', expr: 'lerp(hingePoint, latchPoint, 0.5)', kind: 'midpoint' },
  ],
  primitives: [
    { kind: 'line', from: 'hingePoint', to: 'hingePoint + perp(dir) * width', style: { width: 2 } },
    {
      kind: 'arc',
      center: 'hingePoint',
      radius: 'width',
      start: 'angle(dir)',
      sweep: '90deg',
      style: { dash: [4, 3] },
    },
  ],
  constraints: [
    { lhs: 'offset', op: '>=', rhs: '0mm', label: 'door starts on the wall' },
    { lhs: 'offset + width', op: '<=', rhs: 'dist(host.start, host.end)', label: 'door fits in the wall' },
  ],
}

/** Draw connected walls: click points; Escape/right-click ends; click the first point to close. */
class WallTool implements Tool {
  readonly id = 'floorplan.wall'
  readonly label = 'Wall'
  readonly shortcut = 'w'
  readonly cursor = 'crosshair'
  private first: { id: EntityId; at: Point } | null = null
  private last: { id: EntityId; end: Point } | null = null
  private start: Point | null = null

  activate(ctx: ToolContext): void {
    ctx.setPrompt('Wall: click the start point.')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return this.cancel(ctx)
    if (!this.start) {
      this.start = p.point
      ctx.setState('preview')
      ctx.setPrompt('Wall: click the end point (Escape to finish).')
      return
    }
    const a = this.start
    const b = p.point
    if (Math.hypot(b[0] - a[0], b[1] - a[1]) <= 0) return
    const closing =
      this.first !== null && Math.hypot(b[0] - this.first.at[0], b[1] - this.first.at[1]) < ctx.pxToWorld(8)
    const [id] = await ctx.engine.reserveIds(1)
    if (id === undefined) return
    const commands: Parameters<ToolContext['apply']>[0]['commands'] = [
      {
        op: 'createEntity',
        id,
        entity: { type: 'floorplan.wall', props: { start: a, end: closing && this.first ? this.first.at : b } },
      },
    ]
    if (this.last) {
      commands.push({
        op: 'addConstraint',
        constraint: {
          rule: { kind: 'coincident', a: { entity: this.last.id, anchor: 'end' }, b: { entity: id, anchor: 'start' } },
          label: 'walls connected',
        },
      })
    }
    if (closing && this.first) {
      commands.push({
        op: 'addConstraint',
        constraint: {
          rule: { kind: 'coincident', a: { entity: id, anchor: 'end' }, b: { entity: this.first.id, anchor: 'start' } },
          label: 'walls connected',
        },
      })
    }
    const r = await ctx.apply({ label: 'Wall', commands })
    if (!r) return
    if (closing) {
      this.cancel(ctx)
      return
    }
    this.first ??= { id, at: a }
    this.last = { id, end: b }
    this.start = b
    ctx.setOverlay({})
  }

  pointerMove(ctx: ToolContext, p: ToolPointer): void {
    if (this.start) ctx.setOverlay({ sketch: [{ type: 'line', a: this.start, b: p.point }] })
  }

  cancel(ctx: ToolContext): void {
    this.first = null
    this.last = null
    this.start = null
    ctx.setOverlay({})
    ctx.setState('idle')
    ctx.setPrompt('Wall: click the start point.')
  }
}

/** Place a door: click on a wall. */
class DoorTool implements Tool {
  readonly id = 'floorplan.door'
  readonly label = 'Door'
  readonly shortcut = 'q'
  readonly cursor = 'copy'
  readonly snaps = false

  activate(ctx: ToolContext): void {
    ctx.setPrompt('Door: click on a wall.')
  }

  async pointerDown(ctx: ToolContext, p: ToolPointer): Promise<void> {
    if (p.button !== 0) return
    const hits = await ctx.engine.hitTest(p.world, ctx.pxToWorld(8))
    for (const h of hits) {
      const info = await ctx.engine.entityInfo(h.entity)
      if (info.entity.type !== 'floorplan.wall') continue
      const a = info.anchors.find((x) => x.name === 'start')?.point
      const b = info.anchors.find((x) => x.name === 'end')?.point
      if (!a || !b) continue
      const len = Math.hypot(b[0] - a[0], b[1] - a[1])
      const width = Math.min(900, Math.max(600, len - 1))
      const { t } = project(p.world, a, b)
      const offset = Math.min(Math.max(0, t * len - width / 2), Math.max(0, len - width))
      await ctx.apply({
        label: 'Door',
        commands: [
          { op: 'createEntity', entity: { type: 'floorplan.door', props: { host: { ref: h.entity }, offset, width } } },
        ],
      })
      return
    }
    ctx.report(new Error('Click on a wall to place a door.'))
  }
}

export const floorplanPlugin: DotloomPlugin = {
  id: 'floorplan.rooms',
  version: '1.0.0',
  sdk: '^0.1.0',
  types: [wallType, doorType],
  tools: [() => new WallTool(), () => new DoorTool()],
}

/** A 5 m × 4 m room: four connected walls, a door, an associative dimension, layers. */
export async function loadFloorplanExample(
  engine: DotloomEngine,
): Promise<{ walls: EntityId[]; door: EntityId; dimension: EntityId }> {
  await engine.newDocument()
  const ids = await engine.reserveIds(8)
  const [w1, w2, w3, w4, door, dim, lWalls, lDims] = ids as [
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
  ]
  const corners: Point[] = [
    [0, 0],
    [5000, 0],
    [5000, 4000],
    [0, 4000],
  ]
  const walls = [w1, w2, w3, w4]
  const commands: Parameters<DotloomEngine['apply']>[0] = {
    label: 'Room example',
    commands: [
      { op: 'setSettings', patch: { displayUnit: 'metre', title: 'ev-plani', gridSpacing: 100 } },
      { op: 'addLayer', id: lWalls, name: 'Walls' },
      { op: 'addLayer', id: lDims, name: 'Dimensions' },
    ],
  }
  walls.forEach((id, i) => {
    commands.commands.push({
      op: 'createEntity',
      id,
      entity: {
        type: 'floorplan.wall',
        layer: lWalls,
        name: `Wall ${i + 1}`,
        props: { start: corners[i] as Point, end: corners[(i + 1) % 4] as Point },
      },
    })
  })
  walls.forEach((id, i) => {
    commands.commands.push({
      op: 'addConstraint',
      constraint: {
        rule: {
          kind: 'coincident',
          a: { entity: id, anchor: 'end' },
          b: { entity: walls[(i + 1) % 4] as number, anchor: 'start' },
        },
        label: 'walls connected',
      },
    })
  })
  commands.commands.push(
    {
      op: 'createEntity',
      id: door,
      entity: {
        type: 'floorplan.door',
        layer: lWalls,
        name: 'Entrance',
        props: { host: { ref: w1 }, offset: 1000, width: 900 },
      },
    },
    {
      op: 'createEntity',
      id: dim,
      entity: {
        type: 'dotloom.dimension',
        layer: lDims,
        props: {
          kind: 'linear',
          a: { entity: w3, anchor: 'end' },
          b: { entity: w3, anchor: 'start' },
          through: [2500, 4600],
          textHeight: 200,
          precision: 2,
        },
      },
    },
  )
  await engine.apply(commands)
  return { walls, door, dimension: dim }
}
