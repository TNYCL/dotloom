// A third-party plugin written against the published packages only.

/** @type {import('@dotloom/sdk').EntityTypeDef} */
export const tableType = {
  typeId: 'acme.table',
  version: 1,
  label: 'Table',
  props: {
    seats: { type: 'number', dim: 'scalar', default: 4, min: 1, solve: false, label: 'Seats' },
    width: { type: 'number', dim: 'length', default: '120cm', label: 'Width', stay: 'low' },
    depth: { type: 'number', dim: 'length', default: '80cm', min: '60cm', label: 'Depth' },
  },
  derived: [{ name: 'perSide', expr: 'seats / 2' }],
  anchors: [
    { name: 'origin', expr: 'vec(0mm, 0mm)', kind: 'corner' },
    { name: 'center', expr: 'vec(width / 2, depth / 2)', kind: 'centroid' },
  ],
  primitives: [
    { kind: 'polygon', points: ['vec(0mm, 0mm)', 'vec(width, 0mm)', 'vec(width, depth)', 'vec(0mm, depth)'], style: { fill: '#8b5cf640' } },
    { kind: 'text', position: 'vec(width / 2, depth / 2)', content: 'Table', height: '10cm', halign: 'center', valign: 'middle' },
  ],
  constraints: [{ lhs: 'width', op: '>=', rhs: 'perSide * 60cm', label: 'every seat gets 60 cm' }],
}

/** @returns {import('@dotloom/sdk').Tool} */
function tableTool() {
  return {
    id: 'acme.table',
    label: 'Table',
    shortcut: 'n',
    async pointerDown(ctx, p) {
      if (p.button !== 0) return
      await ctx.apply({
        label: 'Table',
        commands: [{ op: 'createEntity', entity: { type: 'acme.table', transform: [1, 0, 0, 1, p.point[0], p.point[1]] } }],
      })
    },
  }
}

/** @type {import('@dotloom/sdk').DotloomPlugin} */
export const acmePlugin = {
  id: 'acme.furniture',
  version: '1.0.0',
  sdk: '^1.0.0',
  types: [tableType],
  tools: [tableTool],
  constraintTemplates: [
    {
      id: 'sameSeats',
      label: 'Same number of seats',
      arity: 2,
      build: ([a, b]) => (a === undefined || b === undefined ? [] : [{ rule: { kind: 'equal', a: { entity: a, prop: 'seats' }, b: { entity: b, prop: 'seats' } } }]),
    },
  ],
}
