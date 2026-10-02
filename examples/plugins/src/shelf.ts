/**
 * Shelf configurator: inner width and three compartments. The rules live in the
 * type definition and are evaluated by the engine (no ad hoc arithmetic here).
 */

import type { DotloomEngine, DotloomPlugin, EntityTypeDef } from '@dotloomjs/sdk'
import { placeTool } from './common.js'

export const shelfType: EntityTypeDef = {
  typeId: 'shelf.unit',
  version: 1,
  label: 'Shelf unit',
  props: {
    width: { type: 'number', dim: 'length', default: '180cm', label: 'Inner width' },
    height: { type: 'number', dim: 'length', default: '200cm', solve: false, label: 'Height' },
    w1: { type: 'number', dim: 'length', default: '60cm', label: 'Left compartment' },
    w2: { type: 'number', dim: 'length', default: '60cm', label: 'Middle compartment' },
    w3: { type: 'number', dim: 'length', default: '60cm', label: 'Right compartment' },
    material: { type: 'enum', values: ['oak', 'birch', 'white'], default: 'oak', label: 'Material' },
  },
  derived: [
    { name: 'x1', expr: 'w1' },
    { name: 'x2', expr: 'w1 + w2' },
  ],
  anchors: [
    { name: 'origin', expr: 'vec(0mm, 0mm)', kind: 'corner' },
    { name: 'topRight', expr: 'vec(width, height)', kind: 'corner' },
    { name: 'divider1', expr: 'vec(x1, 0mm)', kind: 'vertex' },
    { name: 'divider2', expr: 'vec(x2, 0mm)', kind: 'vertex' },
  ],
  primitives: [
    {
      kind: 'polygon',
      points: ['vec(0mm, 0mm)', 'vec(width, 0mm)', 'vec(width, height)', 'vec(0mm, height)'],
      style: { fill: '#c8a27a40' },
    },
    { kind: 'line', from: 'vec(x1, 0mm)', to: 'vec(x1, height)' },
    { kind: 'line', from: 'vec(x2, 0mm)', to: 'vec(x2, height)' },
    {
      kind: 'text',
      position: 'vec(w1 / 2, height / 2)',
      content: 'A',
      height: '8cm',
      halign: 'center',
      valign: 'middle',
    },
    {
      kind: 'text',
      position: 'vec(x1 + w2 / 2, height / 2)',
      content: 'B',
      height: '8cm',
      halign: 'center',
      valign: 'middle',
    },
    {
      kind: 'text',
      position: 'vec(x2 + w3 / 2, height / 2)',
      content: 'C',
      height: '8cm',
      halign: 'center',
      valign: 'middle',
    },
  ],
  constraints: [
    { lhs: 'w1 + w2 + w3', op: '=', rhs: 'width', label: 'compartments fill the inner width' },
    { lhs: 'w2', op: '=', rhs: 'w3', label: 'middle and right compartments are equal' },
    { lhs: 'w2', op: '>=', rhs: '40cm', label: 'middle compartment at least 40 cm' },
    { lhs: 'w3', op: '>=', rhs: '40cm', label: 'right compartment at least 40 cm' },
  ],
}

export const shelfPlugin: DotloomPlugin = {
  id: 'shelf.configurator',
  version: '1.0.0',
  sdk: '^1.0.0',
  types: [shelfType],
  tools: [placeTool('shelf.place', 'Shelf', 'shelf.unit')],
  constraintTemplates: [
    {
      id: 'lockLeft',
      label: 'Lock left compartment',
      arity: 1,
      build: ([id]) =>
        id === undefined
          ? []
          : [
              {
                rule: { kind: 'fix', param: { entity: id, prop: 'w1' }, value: 600 },
                label: 'left compartment locked at 60 cm',
              },
            ],
    },
  ],
}

/**
 * Load the reference example: 180 cm inner width, three compartments, the left
 * one locked at 60 cm. Returns the shelf entity ID.
 */
export async function loadShelfExample(engine: DotloomEngine): Promise<number> {
  await engine.newDocument()
  const [id] = await engine.reserveIds(1)
  if (id === undefined) throw new Error('no id')
  await engine.apply({
    label: 'Shelf example',
    commands: [
      { op: 'setSettings', patch: { displayUnit: 'centimetre', title: 'raf-tasarimi', gridSpacing: 100 } },
      { op: 'createEntity', id, entity: { type: 'shelf.unit', name: 'Shelf' } },
      {
        op: 'addConstraint',
        constraint: {
          rule: { kind: 'fix', param: { entity: id, prop: 'w1' }, value: 600 },
          label: 'left compartment locked at 60 cm',
        },
      },
    ],
  })
  return id
}
