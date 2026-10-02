import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    alias: {
      '@dotloomjs/sdk/node': fileURLToPath(new URL('../../packages/sdk/src/node.ts', import.meta.url)),
      '@dotloomjs/sdk': fileURLToPath(new URL('../../packages/sdk/src/index.ts', import.meta.url)),
    },
  },
  test: { include: ['test/**/*.test.ts'], environment: 'node', testTimeout: 30_000 },
})
