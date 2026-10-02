#!/usr/bin/env node
// WebAssembly transfer sizes of the packaged SDK (DL-PERF-8): raw, gzip -9 and
// brotli q11, as a CDN or static host would serve them.
//
//   node scripts/bench/wasm-size.mjs [--json bench/wasm-size.json]

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { brotliCompressSync, constants, gzipSync } from 'node:zlib'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const files = {
  engine: 'packages/sdk/dist/wasm/engine/dotloom_wasm_bg.wasm',
  renderer: 'packages/sdk/dist/wasm/render/dotloom_render_web_bg.wasm',
}
const kib = (n) => Math.round((n / 1024) * 10) / 10
const rows = {}
for (const [name, rel] of Object.entries(files)) {
  const bytes = readFileSync(resolve(root, rel))
  rows[name] = {
    file: rel,
    rawKiB: kib(bytes.length),
    gzipKiB: kib(gzipSync(bytes, { level: 9 }).length),
    brotliKiB: kib(
      brotliCompressSync(bytes, {
        params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: bytes.length },
      }).length,
    ),
  }
}
console.table(rows)
const i = process.argv.indexOf('--json')
if (i >= 0) writeFileSync(resolve(process.cwd(), process.argv[i + 1]), `${JSON.stringify(rows, null, 2)}\n`)
