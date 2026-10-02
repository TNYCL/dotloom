import { defineConfig } from 'vite'

// The harness consumes the *built* @dotloomjs/sdk package (dist), like an app would.
export default defineConfig({
  root: 'harness',
  base: './',
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    target: 'es2022',
    // probe.html: a page without Dotloom for plain WebGL/WebGPU environment probes;
    // bench.html: the reference-device benchmark (playwright.bench.config.ts).
    rollupOptions: {
      input: { main: 'harness/index.html', probe: 'harness/probe.html', bench: 'harness/bench.html' },
    },
  },
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloomjs/sdk'] },
  preview: { port: 5199, strictPort: true },
})
