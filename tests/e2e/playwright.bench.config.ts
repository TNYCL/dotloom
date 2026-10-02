import { defineConfig } from '@playwright/test'

/**
 * Reference-device benchmarks (DL-PERF). Run locally on the recorded reference
 * device, never as a CI gate (hosted runners have no GPU):
 *
 *   pnpm --filter @dotloomjs/e2e run bench
 *
 * Projects drive the installed Google Chrome and Microsoft Edge (`channel`) and
 * Playwright's Firefox build, each with an explicit renderer backend.
 * `DOTLOOM_BENCH_PROJECTS=chrome-webgpu,edge-webgl2` limits the projects.
 */
const wanted = process.env.DOTLOOM_BENCH_PROJECTS?.split(',')
const view = { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 }
// Precise heap sizes and `window.gc()` for the open/close leak check.
const chromeArgs = [
  '--enable-unsafe-webgpu',
  '--ignore-gpu-blocklist',
  '--enable-precise-memory-info',
  '--js-flags=--expose-gc',
]

const projects: { name: string; backend: string; use: { browserName: 'chromium' | 'firefox'; channel?: string } }[] = [
  // No device descriptors: they would replace the real user agent.
  { name: 'chrome-webgpu', backend: 'webgpu', use: { browserName: 'chromium', channel: 'chrome' } },
  { name: 'chrome-webgl2', backend: 'webgl2', use: { browserName: 'chromium', channel: 'chrome' } },
  { name: 'edge-webgpu', backend: 'webgpu', use: { browserName: 'chromium', channel: 'msedge' } },
  { name: 'edge-webgl2', backend: 'webgl2', use: { browserName: 'chromium', channel: 'msedge' } },
  { name: 'firefox-webgpu', backend: 'webgpu', use: { browserName: 'firefox' } },
  { name: 'firefox-webgl2', backend: 'webgl2', use: { browserName: 'firefox' } },
]

export default defineConfig({
  testDir: './bench',
  testMatch: '**/*.bench.ts',
  timeout: 600_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  // Headed by default: the real compositor and display refresh (Firefox exposes
  // WebGPU only with a window). DOTLOOM_BENCH_HEADLESS=1 runs without windows.
  use: { baseURL: 'http://localhost:5199/', headless: process.env.DOTLOOM_BENCH_HEADLESS === '1' },
  webServer: [
    {
      command: 'pnpm run build && pnpm run preview',
      url: 'http://localhost:5199/',
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
  projects: projects
    .filter((p) => !wanted || wanted.includes(p.name))
    .map((p) => ({
      name: p.name,
      metadata: { backend: p.backend },
      use: {
        ...p.use,
        ...view,
        launchOptions: p.name.startsWith('firefox')
          ? { firefoxUserPrefs: { 'dom.webgpu.enabled': true, 'gfx.webgpu.ignore-blocklist': true } }
          : { args: chromeArgs },
      },
    })),
})
