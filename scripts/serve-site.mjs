#!/usr/bin/env node
// Serve site/ under a sub-path the way GitHub Pages does (for local checks and E2E).
//
//   node scripts/serve-site.mjs [--port 5200] [--base /dotloom/]

import { createReadStream, existsSync, statSync } from 'node:fs'
import { createServer } from 'node:http'
import { dirname, extname, join, normalize, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..', 'site')
const arg = (name, fallback) => {
  const i = process.argv.indexOf(name)
  return i >= 0 ? process.argv[i + 1] : fallback
}
const port = Number(arg('--port', '5200'))
const base = arg('--base', '/dotloom/')

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.map': 'application/json',
  '.txt': 'text/plain; charset=utf-8',
}

function resolve(urlPath) {
  if (!urlPath.startsWith(base)) return null
  const rel = decodeURIComponent(urlPath.slice(base.length).split('?')[0] ?? '')
  const p = normalize(join(root, rel))
  if (!(p === root || p.startsWith(root + sep))) return null
  if (existsSync(p) && statSync(p).isDirectory()) return join(p, 'index.html')
  if (existsSync(p)) return p
  if (existsSync(`${p}.html`)) return `${p}.html`
  return null
}

createServer((req, res) => {
  const url = req.url ?? '/'
  if (url === '/' || url === base.slice(0, -1)) {
    res.writeHead(302, { location: base })
    res.end()
    return
  }
  const file = resolve(url)
  if (!file || !existsSync(file)) {
    const nf = join(root, '404.html')
    res.writeHead(404, { 'content-type': TYPES['.html'] })
    if (existsSync(nf)) createReadStream(nf).pipe(res)
    else res.end('not found')
    return
  }
  res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' })
  createReadStream(file).pipe(res)
}).listen(port, () => console.log(`serving ${root} at http://localhost:${port}${base}`))
