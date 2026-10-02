import { defineConfig, devices } from '@playwright/test'

/**
 * Browser matrix. Each spec runs every renderer backend explicitly; a backend a
 * browser cannot provide is reported as *skipped* with the reason (never passed).
 *
 * `DOTLOOM_E2E_BROWSERS=chromium,firefox` limits the projects.
 */
const wanted = (process.env.DOTLOOM_E2E_BROWSERS ?? 'chromium,firefox,webkit').split(',')

const chromiumArgs = ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist']
if (process.platform === 'linux' || process.env.DOTLOOM_E2E_SWIFTSHADER === '1') {
  // No GPU on CI runners: SwiftShader provides Vulkan for WebGPU and GL for WebGL2.
  chromiumArgs.push(
    '--enable-features=Vulkan',
    '--use-angle=swiftshader',
    '--use-webgpu-adapter=swiftshader',
    '--enable-unsafe-swiftshader',
  )
}

// DOTLOOM_E2E_SITE=1 runs the published-site checks (site/) instead of the app specs;
// DOTLOOM_SITE_URL points them at a deployed site.
const siteMode = process.env.DOTLOOM_E2E_SITE === '1'
const siteUrl = process.env.DOTLOOM_SITE_URL ?? 'http://localhost:5200/dotloom/'
// DOTLOOM_E2E_EXTERNAL=<dist dir> runs the external plugin app check; the sub-path
// build (`dist-subpath`, base /dotloom/) next to it is served under /dotloom/.
const externalDir = process.env.DOTLOOM_E2E_EXTERNAL
const externalSubDir = externalDir?.replace(/dist[/]?$/, 'dist-subpath')

export default defineConfig({
  testDir: externalDir ? './external' : siteMode ? './site' : './specs',
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  // No retries: a flaky test must fail and be fixed, not pass on a second try.
  retries: 0,
  reporter: [['list'], ['json', { outputFile: 'results/e2e.json' }]],
  use: {
    baseURL: externalDir ? 'http://localhost:5201/' : siteMode ? siteUrl : 'http://localhost:5199/',
    trace: 'retain-on-failure',
    // CI runs Firefox headed under Xvfb so it gets Mesa's software WebGL.
    headless: process.env.DOTLOOM_E2E_HEADED !== '1',
  },
  webServer: externalDir
    ? [
        {
          command: `node ../../scripts/serve-site.mjs --root "${externalDir}" --base / --port 5201`,
          url: 'http://localhost:5201/',
          reuseExistingServer: false,
        },
        {
          command: `node ../../scripts/serve-site.mjs --root "${externalSubDir}" --base /dotloom/ --port 5202`,
          url: 'http://localhost:5202/dotloom/',
          reuseExistingServer: false,
        },
      ]
    : siteMode
      ? process.env.DOTLOOM_SITE_URL
        ? []
        : [
            {
              command: 'node ../../scripts/serve-site.mjs --port 5200',
              url: siteUrl,
              reuseExistingServer: !process.env.CI,
            },
          ]
      : [
          {
            command: 'pnpm run build && pnpm run preview',
            url: 'http://localhost:5199/',
            reuseExistingServer: !process.env.CI,
            timeout: 120_000,
          },
          {
            // The reference React editor (apps/playground).
            command: 'pnpm --filter @dotloomjs/playground run build && pnpm --filter @dotloomjs/playground run preview',
            url: 'http://localhost:5198/',
            reuseExistingServer: !process.env.CI,
            timeout: 120_000,
          },
        ],
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'], channel: 'chromium', launchOptions: { args: chromiumArgs } },
    },
    {
      name: 'firefox',
      use: {
        ...devices['Desktop Firefox'],
        // Headless Linux runners have no GPU: allow Mesa's software WebGL.
        launchOptions: { firefoxUserPrefs: { 'webgl.force-enabled': true, 'webgl.disabled': false } },
      },
    },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ].filter((p) => wanted.includes(p.name)),
})
