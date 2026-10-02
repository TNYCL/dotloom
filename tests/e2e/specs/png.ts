/** Minimal PNG decoder (8-bit RGB/RGBA, non-interlaced) for screenshot pixel checks. */

import { inflateSync } from 'node:zlib'

export interface Rgba {
  width: number
  height: number
  data: Uint8Array
  at(x: number, y: number): [number, number, number, number]
}

export function decodePng(buf: Buffer): Rgba {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG')
  let pos = 8
  let width = 0
  let height = 0
  let colorType = 0
  const idat: Buffer[] = []
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos)
    const type = buf.toString('ascii', pos + 4, pos + 8)
    const body = buf.subarray(pos + 8, pos + 8 + len)
    if (type === 'IHDR') {
      width = body.readUInt32BE(0)
      height = body.readUInt32BE(4)
      if (body[8] !== 8 || body[12] !== 0) throw new Error('unsupported PNG (bit depth/interlace)')
      colorType = body[9] ?? 0
    } else if (type === 'IDAT') {
      idat.push(body)
    } else if (type === 'IEND') {
      break
    }
    pos += 12 + len
  }
  const ch = colorType === 6 ? 4 : colorType === 2 ? 3 : 0
  if (!ch) throw new Error(`unsupported PNG color type ${colorType}`)
  const raw = inflateSync(Buffer.concat(idat))
  const stride = width * ch
  const out = new Uint8Array(width * height * 4)
  const prev = new Uint8Array(stride)
  const cur = new Uint8Array(stride)
  for (let y = 0; y < height; y++) {
    const f = raw[y * (stride + 1)] ?? 0
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1))
    for (let i = 0; i < stride; i++) {
      const a = i >= ch ? (cur[i - ch] ?? 0) : 0
      const b = prev[i] ?? 0
      const c = i >= ch ? (prev[i - ch] ?? 0) : 0
      const x = line[i] ?? 0
      let v: number
      switch (f) {
        case 0:
          v = x
          break
        case 1:
          v = x + a
          break
        case 2:
          v = x + b
          break
        case 3:
          v = x + ((a + b) >> 1)
          break
        case 4: {
          const p = a + b - c
          const pa = Math.abs(p - a)
          const pb = Math.abs(p - b)
          const pc = Math.abs(p - c)
          v = x + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c)
          break
        }
        default:
          throw new Error(`bad PNG filter ${f}`)
      }
      cur[i] = v & 0xff
    }
    for (let px = 0; px < width; px++) {
      const o = (y * width + px) * 4
      out[o] = cur[px * ch] ?? 0
      out[o + 1] = cur[px * ch + 1] ?? 0
      out[o + 2] = cur[px * ch + 2] ?? 0
      out[o + 3] = ch === 4 ? (cur[px * ch + 3] ?? 255) : 255
    }
    prev.set(cur)
  }
  return {
    width,
    height,
    data: out,
    at(x, y) {
      const o = (Math.round(y) * width + Math.round(x)) * 4
      return [out[o] ?? 0, out[o + 1] ?? 0, out[o + 2] ?? 0, out[o + 3] ?? 0]
    },
  }
}
