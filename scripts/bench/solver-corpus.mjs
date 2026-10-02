#!/usr/bin/env node
// Solver benchmark corpus (DL-PERF-4): realistic, mostly nonlinear documents of
// about 200 solver variables each, plus the edits applied to them. Every document
// is consistent by construction; every edit is feasible. Deterministic (seeded).
// Cases marked `stress` are measured and reported but not part of the p95 gate.
//
//   node scripts/bench/solver-corpus.mjs --out bench/solver-corpus.json

import { writeFileSync } from 'node:fs'

let seed = 0x5eed
function rnd() {
  // xorshift32
  seed ^= seed << 13
  seed ^= seed >>> 17
  seed ^= seed << 5
  return ((seed >>> 0) % 1_000_000) / 1_000_000
}
const range = (a, b) => a + (b - a) * rnd()

const line = (id, a, b) => ({ op: 'createEntity', id, entity: { geometry: { type: 'line', a, b } } })
const circle = (id, center, radius) => ({
  op: 'createEntity',
  id,
  entity: { geometry: { type: 'circle', center, radius } },
})
const rule = (r, strength) => ({ op: 'addConstraint', constraint: strength ? { rule: r, strength } : { rule: r } })
const anchor = (entity, name) => ({ entity, anchor: name })
const L = (id) => ({ from: anchor(id, 'start'), to: anchor(id, 'end') })
const set = (entity, param, value, mode = 'prefer') => [{ op: 'setParams', values: [{ entity, param, value }], mode }]

/** A chain of 50 links (200 variables): fixed base, fixed link lengths, free joints. */
function chain(id0, angle) {
  const n = 50
  const cmds = []
  const ids = []
  let p = [0, 0]
  for (let i = 0; i < n; i++) {
    const ang = angle(i)
    const q = [p[0] + 100 * Math.cos(ang), p[1] + 100 * Math.sin(ang)]
    ids.push(id0 + i)
    cmds.push(line(id0 + i, p, q))
    p = q
  }
  cmds.push(rule({ kind: 'fixPoint', a: anchor(id0, 'start'), at: [0, 0] }))
  for (let i = 0; i < n; i++) {
    cmds.push(rule({ kind: 'length', line: L(ids[i]), value: 100 }))
    if (i > 0) cmds.push(rule({ kind: 'coincident', a: anchor(ids[i - 1], 'end'), b: anchor(ids[i], 'start') }))
  }
  return { cmds, last: ids[n - 1], end: p }
}

const moveEnd = (last, x, y) => [
  {
    op: 'setParams',
    values: [
      { entity: last, param: 'b.x', value: x },
      { entity: last, param: 'b.y', value: y },
    ],
    mode: 'prefer',
  },
]

/** Bent linkage: the free end is dragged along an arc, one commit per frame. */
function linkageDrag() {
  const { cmds, last, end } = chain(1000, (i) => 0.9 * Math.sin(i * 0.35) + 0.01 * i)
  const edits = []
  for (let k = 1; k <= 25; k++) {
    // 25 frames along a 400 mm arc around the start point (≈ 50 mm per frame).
    const t = (k / 25) * Math.PI * 0.8
    edits.push(moveEnd(last, end[0] - 160 + 160 * Math.cos(t), end[1] + 160 * Math.sin(t)))
  }
  return { name: 'linkage-drag', setup: [cmds], edits }
}

/** Bent linkage: typed coordinates for the free end, up to 400 mm away. */
function linkageTyped() {
  const { cmds, last, end } = chain(1100, (i) => 0.9 * Math.sin(i * 0.35) + 0.01 * i)
  const edits = []
  for (let k = 0; k < 25; k++) {
    const r = range(50, 400)
    const t = range(0, 2 * Math.PI)
    edits.push(moveEnd(last, end[0] + r * Math.cos(t), end[1] + r * Math.sin(t)))
  }
  return { name: 'linkage-typed', setup: [cmds], edits }
}

/**
 * Stress (reported, not gated): a nearly straight chain whose free end is typed
 * 500–2500 mm inwards. Shortening a straight chain is singular to first order — it
 * must buckle — so local solvers need many steps.
 */
function linkageStraightJump() {
  const { cmds, last } = chain(1200, (i) => 0.25 * Math.sin(i * 0.7))
  const edits = []
  for (let k = 0; k < 25; k++) {
    const r = range(2500, 4500)
    const t = range(-0.6, 0.6)
    edits.push(moveEnd(last, r * Math.cos(t), r * Math.sin(t)))
  }
  return { name: 'linkage-straight-jump', stress: true, setup: [cmds], edits }
}

/** A closed 40-gon with equal sides (160 variables), one side fixed. */
function polygon() {
  const n = 40
  const cmds = []
  const R = 1000
  const pts = Array.from({ length: n }, (_, i) => [
    R * Math.cos((2 * Math.PI * i) / n),
    R * Math.sin((2 * Math.PI * i) / n),
  ])
  for (let i = 0; i < n; i++) cmds.push(line(2000 + i, pts[i], pts[(i + 1) % n]))
  for (let i = 0; i < n; i++)
    cmds.push(rule({ kind: 'coincident', a: anchor(2000 + i, 'end'), b: anchor(2000 + ((i + 1) % n), 'start') }))
  for (let i = 1; i < n; i++) cmds.push(rule({ kind: 'equalLength', a: L(2000), b: L(2000 + i) }))
  cmds.push(rule({ kind: 'fixPoint', a: anchor(2000, 'start'), at: pts[0] }))
  cmds.push(rule({ kind: 'fixPoint', a: anchor(2000, 'end'), at: pts[1] }))
  const edits = []
  for (let k = 0; k < 25; k++) {
    const i = 5 + Math.floor(rnd() * 30)
    const v = pts[(i + 1) % n]
    edits.push(set(2000 + i, 'b.x', v[0] + range(-150, 150)))
  }
  return { name: 'polygon-40', setup: [cmds], edits }
}

/** 12 rectangles built from 48 lines (192 variables) with equal widths and spacing. */
function rectangles() {
  const cmds = []
  const rects = []
  let id = 3000
  for (let r = 0; r < 12; r++) {
    const x = (r % 4) * 600
    const y = Math.floor(r / 4) * 500
    const c = [
      [x, y],
      [x + 400, y],
      [x + 400, y + 300],
      [x, y + 300],
    ]
    const ls = [0, 1, 2, 3].map((k) => {
      cmds.push(line(id, c[k], c[(k + 1) % 4]))
      return id++
    })
    rects.push(ls)
    for (let k = 0; k < 4; k++)
      cmds.push(rule({ kind: 'coincident', a: anchor(ls[k], 'end'), b: anchor(ls[(k + 1) % 4], 'start') }))
    cmds.push(rule({ kind: 'horizontal', a: anchor(ls[0], 'start'), b: anchor(ls[0], 'end') }))
    cmds.push(rule({ kind: 'horizontal', a: anchor(ls[2], 'start'), b: anchor(ls[2], 'end') }))
    cmds.push(rule({ kind: 'vertical', a: anchor(ls[1], 'start'), b: anchor(ls[1], 'end') }))
    cmds.push(rule({ kind: 'vertical', a: anchor(ls[3], 'start'), b: anchor(ls[3], 'end') }))
  }
  for (let r = 1; r < 12; r++) cmds.push(rule({ kind: 'equalLength', a: L(rects[0][0]), b: L(rects[r][0]) }))
  for (let r = 1; r < 12; r++) {
    if (r % 4 === 0) continue
    cmds.push(
      rule({ kind: 'distance', a: anchor(rects[r - 1][1], 'start'), b: anchor(rects[r][0], 'start'), value: 200 }),
    )
  }
  cmds.push(rule({ kind: 'fixPoint', a: anchor(rects[0][0], 'start'), at: [0, 0] }))
  const edits = []
  for (let k = 0; k < 25; k++) {
    const r = Math.floor(rnd() * 12)
    edits.push(set(rects[r][2], 'a.y', Math.floor(r / 4) * 500 + 300 + range(-80, 80)))
  }
  return { name: 'rectangles-12', setup: [cmds], edits }
}

/** 40 circles and 20 tangent lines (200 variables). */
function circles() {
  const cmds = []
  const cs = []
  for (let i = 0; i < 40; i++) {
    const r = 50
    cmds.push(circle(4000 + i, [i * 100, 0], r))
    cs.push(4000 + i)
  }
  for (let i = 1; i < 40; i++) cmds.push(rule({ kind: 'tangentCircles', a: cs[i - 1], b: cs[i] }))
  for (let i = 1; i < 40; i++) cmds.push(rule({ kind: 'equalRadius', a: cs[0], b: cs[i] }))
  for (let i = 0; i < 20; i++) {
    const c = i * 2
    cmds.push(line(5000 + i, [c * 100 - 30, 50], [c * 100 + 30, 50]))
    cmds.push(rule({ kind: 'tangentLineCircle', line: L(5000 + i), circle: cs[c], side: -1 }))
    cmds.push(rule({ kind: 'horizontal', a: anchor(5000 + i, 'start'), b: anchor(5000 + i, 'end') }))
    cmds.push(rule({ kind: 'length', line: L(5000 + i), value: 60 }))
  }
  cmds.push(rule({ kind: 'fixPoint', a: { entity: cs[0], anchor: 'center' }, at: [0, 0] }))
  const edits = []
  for (let k = 0; k < 25; k++) edits.push(set(cs[0], 'r', range(30, 80)))
  return { name: 'circles-40', setup: [cmds], edits }
}

/** A random but consistent sketch: lines, circles, mixed rules (≈200 variables). */
function mixed() {
  const cmds = []
  const lines = []
  let id = 6000
  for (let i = 0; i < 35; i++) {
    const x = range(-2000, 2000)
    const y = range(-2000, 2000)
    const kind = i % 3
    const len = range(100, 400)
    const b = kind === 0 ? [x + len, y] : kind === 1 ? [x, y + len] : [x + len * 0.6, y + len * 0.8]
    cmds.push(line(id, [x, y], b))
    lines.push({ id, kind, len })
    if (kind === 0) cmds.push(rule({ kind: 'horizontal', a: anchor(id, 'start'), b: anchor(id, 'end') }))
    if (kind === 1) cmds.push(rule({ kind: 'vertical', a: anchor(id, 'start'), b: anchor(id, 'end') }))
    cmds.push(rule({ kind: 'length', line: L(id), value: len }))
    id++
  }
  const diag = lines.filter((l) => l.kind === 2)
  for (let i = 1; i < diag.length; i++) cmds.push(rule({ kind: 'parallel', a: L(diag[0].id), b: L(diag[i].id) }))
  for (let i = 0; i < 12; i++) {
    const r = range(40, 120)
    cmds.push(circle(id, [range(-2000, 2000), range(-2000, 2000)], r))
    cmds.push(rule({ kind: 'radius', circle: id, value: r }))
    id++
  }
  cmds.push(rule({ kind: 'fixPoint', a: anchor(lines[0].id, 'start'), at: [0, 0] }, 'medium'))
  const edits = []
  for (let k = 0; k < 25; k++) {
    const l = lines[Math.floor(rnd() * lines.length)]
    edits.push(set(l.id, 'a.x', range(-2000, 2000)))
  }
  return { name: 'mixed-sketch', setup: [cmds], edits }
}

const SHELF = {
  typeId: 'bench.shelf',
  version: 1,
  props: {
    width: { type: 'number', dim: 'length', default: '180cm' },
    w1: { type: 'number', dim: 'length', default: '60cm' },
    w2: { type: 'number', dim: 'length', default: '60cm' },
    w3: { type: 'number', dim: 'length', default: '60cm' },
  },
  constraints: [
    { lhs: 'w1 + w2 + w3', op: '=', rhs: 'width' },
    { lhs: 'w2', op: '=', rhs: 'w3' },
    { lhs: 'w2', op: '>=', rhs: '40cm' },
    { lhs: 'w3', op: '>=', rhs: '40cm' },
  ],
}

/** 50 shelf units (200 linear variables) with equal widths: linear backend. */
function shelves() {
  const cmds = []
  for (let i = 0; i < 50; i++) cmds.push({ op: 'createEntity', id: 7000 + i, entity: { type: 'bench.shelf' } })
  for (let i = 1; i < 50; i++)
    cmds.push(rule({ kind: 'equal', a: { entity: 7000, prop: 'width' }, b: { entity: 7000 + i, prop: 'width' } }))
  cmds.push(rule({ kind: 'fix', param: { entity: 7000, prop: 'w1' }, value: 600 }))
  const edits = []
  for (let k = 0; k < 25; k++) edits.push(set(7000 + Math.floor(rnd() * 50), 'width', range(1450, 2400), 'exact'))
  return { name: 'shelves-50', types: [SHELF], setup: [cmds], edits }
}

export function corpus() {
  seed = 0x5eed
  const all = [
    linkageDrag(),
    linkageTyped(),
    polygon(),
    rectangles(),
    circles(),
    mixed(),
    shelves(),
    linkageStraightJump(),
  ]
  // Entities with explicit IDs first: rules get IDs allocated after them.
  for (const c of all) {
    c.setup = c.setup.map((cmds) => [
      ...cmds.filter((x) => x.op === 'createEntity'),
      ...cmds.filter((x) => x.op !== 'createEntity'),
    ])
  }
  return all
}

const out = (() => {
  const i = process.argv.indexOf('--out')
  return i >= 0 ? process.argv[i + 1] : null
})()
if (out) {
  writeFileSync(out, `${JSON.stringify(corpus(), null, 0)}\n`)
  console.log(`wrote ${out}`)
}
