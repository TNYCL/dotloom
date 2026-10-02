/**
 * Native/WASM parity (DL-TEST-6), WebAssembly half: the cases of
 * tests/fixtures/parity/cases.json through the WASM build's `WasmEngine`, FNV-1a
 * digests of every output compared with tests/fixtures/parity/expected.json — the
 * file the native half (crates/wasm/tests/parity.rs) is checked against too.
 */

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { initSync, WasmEngine } from '../src/wasm/engine/dotloom_wasm.js'

const BUDGET = 50
const fixtures = resolve(process.cwd(), '../../tests/fixtures/parity')

interface Case {
  name: string
  types: unknown[]
  setup: unknown[]
  edits: unknown[]
  hit: [number, number, number][]
  snap: unknown[]
}

function fnv1a(data: Uint8Array): string {
  let h = 0xcbf29ce484222325n
  for (const b of data) {
    h ^= BigInt(b)
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn
  }
  return h.toString(16).padStart(16, '0')
}

const text = (s: string) => fnv1a(new TextEncoder().encode(s))

function apply(e: WasmEngine, tx: unknown): string {
  e.begin_apply(JSON.stringify(tx), undefined)
  for (;;) {
    const r = e.step(BUDGET)
    if (!r.includes('"state":"running"')) return r
  }
}

function run(c: Case): Record<string, string> {
  const out: Record<string, string> = {}
  const key = (k: string) => `${String(Object.keys(out).length).padStart(3, '0')}-${k}`
  const e = new WasmEngine()
  try {
    if (c.types.length > 0) e.register_types(JSON.stringify(c.types), 'parity')
    for (const tx of c.setup) out[key('setup')] = text(apply(e, tx))
    for (const tx of c.edits) out[key('edit')] = text(apply(e, tx))
    out.document = text(e.document_json())
    out.scene = fnv1a(e.full_scene())
    c.hit.forEach(([x, y, r], i) => {
      out[`hit-${i}`] = text(e.hit_test(x, y, r))
    })
    c.snap.forEach((q, i) => {
      out[`snap-${i}`] = text(e.snap(JSON.stringify(q)))
    })
    out.svg = text(e.export_svg(undefined, undefined, undefined))
    out.dxf = text(e.export_dxf())
  } finally {
    e.free()
  }
  return out
}

describe('native/WASM parity', () => {
  it('WASM results match the digests the native build produces', () => {
    initSync({ module: readFileSync(resolve(process.cwd(), 'src/wasm/engine/dotloom_wasm_bg.wasm')) })
    const cases = JSON.parse(readFileSync(resolve(fixtures, 'cases.json'), 'utf8')) as Case[]
    const expected = JSON.parse(readFileSync(resolve(fixtures, 'expected.json'), 'utf8')) as Record<
      string,
      Record<string, string>
    >
    const diffs: string[] = []
    for (const c of cases) {
      const got = run(c)
      const want = expected[c.name]
      expect(want, `expected digests for ${c.name}`).toBeDefined()
      for (const [k, digest] of Object.entries(want ?? {})) {
        if (got[k] !== digest) diffs.push(`${c.name}/${k}: expected ${digest}, got ${got[k]}`)
      }
    }
    expect(cases.length).toBe(Object.keys(expected).length)
    expect(diffs, diffs.join('\n')).toEqual([])
  }, 120_000)
})
