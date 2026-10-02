import { defineConfig } from 'vite'

// The harness consumes the *built* @dotloom/sdk package (dist), like an app would.
export default defineConfig({
  root: 'harness',
  base: './',
  build: { outDir: '../dist', emptyOutDir: true, target: 'es2022' },
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloom/sdk'] },
  preview: { port: 5199, strictPort: true },
})
