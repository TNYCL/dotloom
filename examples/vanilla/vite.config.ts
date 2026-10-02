import { defineConfig } from 'vite'

// DOTLOOM_BASE sets the public path (GitHub Pages: /dotloom/examples/<name>/).
export default defineConfig({
  base: process.env.DOTLOOM_BASE ?? '/',
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloom/sdk'] },
  build: { target: 'es2022' },
})
