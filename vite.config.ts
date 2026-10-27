import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  build: {
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: 'react', test: /node_modules[\\/](?:react|react-dom|scheduler)[\\/]/, priority: 30 },
            { name: 'motion', test: /node_modules[\\/](?:motion|motion-dom|motion-utils|framer-motion)[\\/]/, priority: 20 },
            { name: 'controls', test: /node_modules[\\/](?:radix-ui|@radix-ui|cmdk|sonner)[\\/]/, priority: 10 },
          ],
        },
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: 'http://127.0.0.1:8765', changeOrigin: true },
      '/content': { target: 'http://127.0.0.1:8765', changeOrigin: true },
    },
  },
})
