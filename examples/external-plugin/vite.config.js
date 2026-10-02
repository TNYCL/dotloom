import { defineConfig } from 'vite'

export default defineConfig({
  base: './',
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloom/sdk', '@dotloom/react'] },
  build: { target: 'es2022' },
})
