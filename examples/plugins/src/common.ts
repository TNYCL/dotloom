import type { Point, Tool, ToolContext, ToolPointer } from '@dotloomjs/sdk'

/** A tool that places one entity of a plugin type at the clicked point. */
export function placeTool(id: string, label: string, typeId: string, shortcut?: string): () => Tool {
  return () => ({
    id,
    label,
    ...(shortcut ? { shortcut } : {}),
    cursor: 'copy',
    activate(ctx: ToolContext) {
      ctx.setPrompt(`${label}: click to place.`)
    },
    async pointerDown(ctx: ToolContext, p: ToolPointer) {
      if (p.button !== 0) return
      await ctx.apply({
        label: `Place ${label}`,
        commands: [{ op: 'createEntity', entity: { type: typeId, transform: [1, 0, 0, 1, p.point[0], p.point[1]] } }],
      })
    },
  })
}

export function project(p: Point, a: Point, b: Point): { t: number; dist: number } {
  const dx = b[0] - a[0]
  const dy = b[1] - a[1]
  const len2 = dx * dx + dy * dy
  if (len2 === 0) return { t: 0, dist: Math.hypot(p[0] - a[0], p[1] - a[1]) }
  const t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2
  const q: Point = [a[0] + t * dx, a[1] + t * dy]
  return { t, dist: Math.hypot(p[0] - q[0], p[1] - q[1]) }
}
