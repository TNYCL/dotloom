import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// DOTLOOM_BASE sets the public path (GitHub Pages: /dotloom/examples/<name>/).
export default defineConfig({
  base: process.env.DOTLOOM_BASE ?? '/',
  plugins: [react()],
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloomjs/sdk', '@dotloomjs/react'] },
  build: { target: 'es2022' },
})
