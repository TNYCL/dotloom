#!/usr/bin/env node
// Download a real browser release and its WebDriver server from the vendor's own
// distribution (DL-TEST-13): the current or previous major of
//
// - chrome  — Chrome for Testing (googlechromelabs.github.io/chrome-for-testing)
// - msedge  — Microsoft Edge stable .deb (packages.microsoft.com, SHA-256 from the
//             repository index) + msedgedriver (msedgedriver.microsoft.com); Linux only
// - firefox — Firefox release (archive.mozilla.org, versions from
//             product-details.mozilla.org) + geckodriver (pinned, SHA-256 checked)
//
//   node tests/e2e/webdriver/install.mjs <chrome|msedge|firefox> <current|previous> --dir <dir>
//
// Prints and writes <dir>/browser.json: { browser, channel, version, binary, driver }.

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const [browser, channel] = process.argv.slice(2)
const dirArg = process.argv.indexOf('--dir')
const dir = resolve(dirArg >= 0 ? (process.argv[dirArg + 1] ?? '.') : `browsers/${browser}-${channel}`)
if (!['chrome', 'msedge', 'firefox'].includes(browser ?? '') || !['current', 'previous'].includes(channel ?? '')) {
  console.error('usage: install.mjs <chrome|msedge|firefox> <current|previous> [--dir <dir>]')
  process.exit(2)
}
const win = process.platform === 'win32'
if (process.platform !== 'linux' && !(win && browser === 'chrome')) {
  console.error(`${browser} downloads are implemented for Linux (and Chrome on Windows)`)
  process.exit(2)
}
rmSync(dir, { recursive: true, force: true })
mkdirSync(dir, { recursive: true })

const GECKODRIVER = {
  version: '0.37.1',
  linux64: 'e815130ea95983e162ae91843b48d3a3ce991735635fce83a647afde21e09f7e',
}

async function json(url) {
  const r = await fetch(url)
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`)
  return r.json()
}
async function text(url) {
  const r = await fetch(url)
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`)
  return r.text()
}
async function download(url, file, sha256) {
  const r = await fetch(url)
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`)
  const bytes = Buffer.from(await r.arrayBuffer())
  if (sha256) {
    const got = createHash('sha256').update(bytes).digest('hex')
    if (got !== sha256) throw new Error(`${url}: SHA-256 ${got} != ${sha256}`)
  }
  writeFileSync(file, bytes)
  return file
}
function run(cmd, args, cwd = dir) {
  const r = spawnSync(cmd, args, { cwd, stdio: 'inherit' })
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed`)
}
const major = (v) => Number(v.split('.')[0])
const cmp = (a, b) => {
  const x = a.split(/[.-]/).map(Number)
  const y = b.split(/[.-]/).map(Number)
  for (let i = 0; i < Math.max(x.length, y.length); i++)
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0)
  return 0
}
function unzip(file, into) {
  if (win)
    run('powershell', ['-NoProfile', '-Command', `Expand-Archive -Force -Path '${file}' -DestinationPath '${into}'`])
  else run('unzip', ['-q', '-o', file, '-d', into])
}

let info
if (browser === 'chrome') {
  const base = 'https://googlechromelabs.github.io/chrome-for-testing'
  const stable = (await json(`${base}/last-known-good-versions.json`)).channels.Stable.version
  const milestone = String(major(stable) - (channel === 'previous' ? 1 : 0))
  const m = (await json(`${base}/latest-versions-per-milestone-with-downloads.json`)).milestones[milestone]
  const platform = win ? 'win64' : 'linux64'
  const url = (what) => m.downloads[what].find((d) => d.platform === platform).url
  unzip(await download(url('chrome'), join(dir, 'chrome.zip')), dir)
  unzip(await download(url('chromedriver'), join(dir, 'chromedriver.zip')), dir)
  info = {
    version: m.version,
    binary: join(dir, `chrome-${platform}`, win ? 'chrome.exe' : 'chrome'),
    driver: join(dir, `chromedriver-${platform}`, win ? 'chromedriver.exe' : 'chromedriver'),
  }
} else if (browser === 'msedge') {
  const repo = 'https://packages.microsoft.com/repos/edge'
  const index = await text(`${repo}/dists/stable/main/binary-amd64/Packages`)
  const pkgs = index
    .split(/\n\n+/)
    .map((block) => Object.fromEntries(block.split('\n').map((l) => [l.split(': ')[0], l.slice(l.indexOf(': ') + 2)])))
    .filter((p) => p.Package === 'microsoft-edge-stable')
    .sort((a, b) => cmp(a.Version, b.Version))
  const newest = major(pkgs.at(-1).Version)
  const want = newest - (channel === 'previous' ? 1 : 0)
  const pkg = pkgs.filter((p) => major(p.Version) === want).at(-1)
  if (!pkg) throw new Error(`no Edge ${want} in the repository`)
  const deb = await download(`${repo}/${pkg.Filename}`, join(dir, 'edge.deb'), pkg.SHA256)
  run('dpkg-deb', ['-x', deb, join(dir, 'root')])
  const version = pkg.Version.split('-')[0]
  unzip(
    await download(`https://msedgedriver.microsoft.com/${version}/edgedriver_linux64.zip`, join(dir, 'driver.zip')),
    dir,
  )
  info = { version, binary: join(dir, 'root/opt/microsoft/msedge/msedge'), driver: join(dir, 'msedgedriver') }
} else {
  const pd = 'https://product-details.mozilla.org/1.0'
  const latest = (await json(`${pd}/firefox_versions.json`)).LATEST_FIREFOX_VERSION
  let version = latest
  if (channel === 'previous') {
    const all = [
      ...Object.keys(await json(`${pd}/firefox_history_major_releases.json`)),
      ...Object.keys(await json(`${pd}/firefox_history_stability_releases.json`)),
    ]
    version = all
      .filter((v) => major(v) === major(latest) - 1)
      .sort(cmp)
      .at(-1)
    if (!version) throw new Error(`no Firefox ${major(latest) - 1} release found`)
  }
  const ext = major(version) >= 135 ? 'tar.xz' : 'tar.bz2'
  const tarball = await download(
    `https://archive.mozilla.org/pub/firefox/releases/${version}/linux-x86_64/en-US/firefox-${version}.${ext}`,
    join(dir, `firefox.${ext}`),
  )
  run('tar', ['-xf', tarball])
  const gd = await download(
    `https://github.com/mozilla/geckodriver/releases/download/v${GECKODRIVER.version}/geckodriver-v${GECKODRIVER.version}-linux64.tar.gz`,
    join(dir, 'geckodriver.tar.gz'),
    GECKODRIVER.linux64,
  )
  run('tar', ['-xzf', gd])
  info = { version, binary: join(dir, 'firefox/firefox'), driver: join(dir, 'geckodriver') }
}

for (const p of [info.binary, info.driver]) {
  if (!existsSync(p)) throw new Error(`missing ${p} (have: ${readdirSync(dir).join(', ')})`)
  if (!win) chmodSync(p, 0o755)
}
const out = { browser, channel, ...info }
writeFileSync(join(dir, 'browser.json'), `${JSON.stringify(out, null, 2)}\n`)
console.log(JSON.stringify(out, null, 2))
