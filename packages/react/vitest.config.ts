import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    // Tests run against SDK sources (and the WASM built into them).
    alias: {
      '@dotloom/sdk/node': fileURLToPath(new URL('../sdk/src/node.ts', import.meta.url)),
      '@dotloom/sdk': fileURLToPath(new URL('../sdk/src/index.ts', import.meta.url)),
    },
  },
  test: { include: ['test/**/*.test.tsx'], environment: 'jsdom', testTimeout: 30_000 },
})
