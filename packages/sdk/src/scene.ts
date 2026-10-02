/**
 * TypeScript decoder of the binary scene delta (`dotloom-scene`, format 1).
 *
 * The default wgpu renderer decodes deltas inside WebAssembly; this decoder lets
 * hosts build their own renderers (or tests) from the same public contract.
 */

import type { Aabb, HAlign, PathEl, Point, Shape, Text, VAlign } from './types.js'

export const SCENE_FORMAT_VERSION = 1

/** Item flags. */
export const SceneFlags = {
  selected: 1,
  preview: 2,
  locked: 4,
  hover: 8,
  readOnly: 16,
  problem: 32,
} as const

export interface Stroke {
  /** `0xRRGGBBAA`; `0` means the theme's foreground color. */
  color: number
  width: number
  dash: number[]
}

export type Primitive =
  | { prim: 'shape'; shape: Shape; stroke: Stroke | null; fill: number | null }
  | { prim: 'text'; text: Required<Text>; color: number }
  | { prim: 'arrow'; tip: Point; direction: Point; size: number; color: number }

export interface SceneItem {
  id: number
  layer: number
  bbox: Aabb
  flags: number
  prims: Primitive[]
}

export interface SceneDelta {
  revision: number
  preview: boolean
  reset: boolean
  upserts: SceneItem[]
  removals: number[]
  order: number[] | null
}

const HALIGN: HAlign[] = ['left', 'center', 'right']
const VALIGN: VAlign[] = ['baseline', 'middle', 'top', 'bottom']

class Reader {
  private i = 0
  private readonly dv: DataView
  private readonly bytes: Uint8Array
  constructor(bytes: Uint8Array) {
    this.bytes = bytes
    this.dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  }
  need(n: number): void {
    if (this.i + n > this.bytes.byteLength) throw new RangeError('scene delta truncated')
  }
  u8(): number {
    this.need(1)
    return this.dv.getUint8(this.i++)
  }
  u16(): number {
    this.need(2)
    const v = this.dv.getUint16(this.i, true)
    this.i += 2
    return v
  }
  u32(): number {
    this.need(4)
    const v = this.dv.getUint32(this.i, true)
    this.i += 4
    return v
  }
  u64(): number {
    this.need(8)
    const v = this.dv.getBigUint64(this.i, true)
    this.i += 8
    return Number(v)
  }
  f32(): number {
    this.need(4)
    const v = this.dv.getFloat32(this.i, true)
    this.i += 4
    return v
  }
  f64(): number {
    this.need(8)
    const v = this.dv.getFloat64(this.i, true)
    this.i += 8
    return v
  }
  count(minSize: number): number {
    const n = this.u32()
    if (n * Math.max(1, minSize) > this.bytes.byteLength - this.i)
      throw new RangeError('scene delta count exceeds input')
    return n
  }
  pt(): Point {
    return [this.f64(), this.f64()]
  }
  str(): string {
    const n = this.count(1)
    this.need(n)
    const s = new TextDecoder().decode(this.bytes.subarray(this.i, this.i + n))
    this.i += n
    return s
  }
  points(): Point[] {
    const n = this.count(16)
    const out: Point[] = []
    for (let k = 0; k < n; k++) out.push(this.pt())
    return out
  }
  text(): Required<Text> {
    const position = this.pt()
    const height = this.f64()
    const rotation = this.f64()
    const halign = HALIGN[this.u8()] ?? 'left'
    const valign = VALIGN[this.u8()] ?? 'baseline'
    const content = this.str()
    return { position, content, height, rotation, halign, valign }
  }
  shape(): Shape {
    const tag = this.u8()
    switch (tag) {
      case 0:
        return { type: 'point', at: this.pt() }
      case 1:
        return { type: 'line', a: this.pt(), b: this.pt() }
      case 2: {
        const closed = this.u8() !== 0
        const points = this.points()
        const nb = this.count(8)
        const bulges: number[] = []
        for (let k = 0; k < nb; k++) bulges.push(this.f64())
        return { type: 'polyline', points, bulges, closed }
      }
      case 3:
        return { type: 'rect', origin: this.pt(), width: this.f64(), height: this.f64() }
      case 4:
        return { type: 'circle', center: this.pt(), radius: this.f64() }
      case 5:
        return { type: 'arc', center: this.pt(), radius: this.f64(), start: this.f64(), sweep: this.f64() }
      case 6: {
        const n = this.count(1)
        const elements: PathEl[] = []
        for (let k = 0; k < n; k++) {
          const op = this.u8()
          if (op === 0) elements.push({ M: this.pt() })
          else if (op === 1) elements.push({ L: this.pt() })
          else if (op === 2) elements.push({ Q: [this.pt(), this.pt()] })
          else if (op === 3) elements.push({ C: [this.pt(), this.pt(), this.pt()] })
          else if (op === 4) elements.push('Z')
          else throw new RangeError(`invalid path tag ${op}`)
        }
        return { type: 'path', elements }
      }
      case 7: {
        const rings = this.count(4)
        if (rings === 0) throw new RangeError('polygon without rings')
        const outer = this.points()
        const holes: Point[][] = []
        for (let k = 1; k < rings; k++) holes.push(this.points())
        return { type: 'polygon', outer, holes }
      }
      case 8:
        return { type: 'text', ...this.text() }
      default:
        throw new RangeError(`invalid shape tag ${tag}`)
    }
  }
  done(): boolean {
    return this.i === this.bytes.byteLength
  }
}

function header(bytes: Uint8Array): { flags: number; upserts: number; removals: number } {
  if (bytes.byteLength < 24 || bytes[0] !== 0x44 || bytes[1] !== 0x4c || bytes[2] !== 0x53 || bytes[3] !== 0x43) {
    throw new RangeError('not a Dotloom scene delta')
  }
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const version = dv.getUint16(4, true)
  if (version !== SCENE_FORMAT_VERSION) throw new RangeError(`unsupported scene format ${version}`)
  return { flags: dv.getUint16(6, true), upserts: dv.getUint32(16, true), removals: dv.getUint32(20, true) }
}

/** Whether a binary delta changes nothing (no reset, items, removals or order). */
export function isEmptyDelta(bytes: Uint8Array): boolean {
  const h = header(bytes)
  return (h.flags & (2 | 4)) === 0 && h.upserts === 0 && h.removals === 0
}

/** Decode a binary scene delta. Throws `RangeError` on malformed input. */
export function decodeSceneDelta(input: ArrayBuffer | Uint8Array): SceneDelta {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input)
  header(bytes)
  const r = new Reader(bytes)
  r.u32()
  r.u16()
  const flags = r.u16()
  const revision = r.u64()
  const nUp = r.count(4)
  const nRm = r.count(8)
  const nOrder = flags & 4 ? r.count(8) : -1
  const removals: number[] = []
  for (let k = 0; k < nRm; k++) removals.push(r.u64())
  let order: number[] | null = null
  if (nOrder >= 0) {
    order = []
    for (let k = 0; k < nOrder; k++) order.push(r.u64())
  }
  const upserts: SceneItem[] = []
  for (let k = 0; k < nUp; k++) {
    const id = r.u64()
    const layer = r.u32()
    const itemFlags = r.u8()
    const bbox: Aabb = { min: r.pt(), max: r.pt() }
    const np = r.count(1)
    const prims: Primitive[] = []
    for (let j = 0; j < np; j++) {
      const tag = r.u8()
      if (tag === 0) {
        const f = r.u8()
        let stroke: Stroke | null = null
        if (f & 1) {
          const color = r.u32()
          const width = r.f32()
          const nd = r.count(4)
          const dash: number[] = []
          for (let d = 0; d < nd; d++) dash.push(r.f32())
          stroke = { color, width, dash }
        }
        const fill = f & 2 ? r.u32() : null
        prims.push({ prim: 'shape', shape: r.shape(), stroke, fill })
      } else if (tag === 1) {
        const text = r.text()
        prims.push({ prim: 'text', text, color: r.u32() })
      } else if (tag === 2) {
        const tip = r.pt()
        const direction: Point = [r.f64(), r.f64()]
        prims.push({ prim: 'arrow', tip, direction, size: r.f64(), color: r.u32() })
      } else {
        throw new RangeError(`invalid primitive tag ${tag}`)
      }
    }
    upserts.push({ id, layer, bbox, flags: itemFlags, prims })
  }
  if (!r.done()) throw new RangeError('trailing bytes in scene delta')
  return { revision, preview: (flags & 1) !== 0, reset: (flags & 2) !== 0, upserts, removals, order }
}

/** Applies deltas to an in-memory scene (useful for custom renderers). */
export class SceneStore {
  readonly items = new Map<number, SceneItem>()
  order: number[] = []
  revision = 0

  apply(delta: SceneDelta): void {
    if (delta.reset) this.items.clear()
    for (const id of delta.removals) this.items.delete(id)
    for (const it of delta.upserts) this.items.set(it.id, it)
    if (delta.order) this.order = delta.order
    if (!delta.preview) this.revision = delta.revision
  }

  /** Items bottom-to-top: by layer, then draw order. */
  ordered(): SceneItem[] {
    const pos = new Map(this.order.map((id, i) => [id, i]))
    return [...this.items.values()].sort((a, b) => a.layer - b.layer || (pos.get(a.id) ?? 0) - (pos.get(b.id) ?? 0))
  }
}
