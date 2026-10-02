import { defineConfig, devices } from '@playwright/test'

/**
 * Browser matrix. Each spec runs every renderer backend explicitly; a backend a
 * browser cannot provide is reported as *skipped* with the reason (never passed).
 *
 * `DOTLOOM_E2E_BROWSERS=chromium,firefox` limits the projects.
 */
const wanted = (process.env.DOTLOOM_E2E_BROWSERS ?? 'chromium,firefox,webkit').split(',')

const chromiumArgs = ['--enable-unsafe-webgpu', '--ignore-gpu-blocklist']
if (process.platform === 'linux') {
  // No GPU on CI runners: SwiftShader provides Vulkan for WebGPU and GL for WebGL2.
  chromiumArgs.push(
    '--enable-features=Vulkan',
    '--use-angle=swiftshader',
    '--use-webgpu-adapter=swiftshader',
    '--enable-unsafe-swiftshader',
  )
}

export default defineConfig({
  testDir: './specs',
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['json', { outputFile: 'results/e2e.json' }]],
  use: { baseURL: 'http://localhost:5199/', trace: 'retain-on-failure' },
  webServer: {
    command: 'pnpm run build && pnpm run preview',
    url: 'http://localhost:5199/',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
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
