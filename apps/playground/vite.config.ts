import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// DOTLOOM_BASE=/dotloom/playground/ for GitHub Pages.
export default defineConfig({
  base: process.env.DOTLOOM_BASE ?? '/',
  plugins: [react()],
  worker: { format: 'es' },
  optimizeDeps: { exclude: ['@dotloomjs/sdk', '@dotloomjs/react'] },
  build: { target: 'es2022', sourcemap: true },
  preview: { port: 5198, strictPort: true },
})
