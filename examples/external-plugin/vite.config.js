import { defineConfig } from 'vite'

export default defineConfig({
  base: './',
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloomjs/sdk', '@dotloomjs/react'] },
  build: { target: 'es2022' },
})
