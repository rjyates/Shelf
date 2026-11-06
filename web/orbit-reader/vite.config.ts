import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

const bookorbit = fileURLToPath(new URL('./vendor/bookorbit', import.meta.url))

export default defineConfig({
  base: '/orbit-reader/',
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: [
      { find: '@bookorbit/types', replacement: fileURLToPath(new URL('./vendor/bookorbit/types/index.ts', import.meta.url)) },
      { find: '@', replacement: bookorbit },
    ],
  },
  publicDir: 'public',
  optimizeDeps: {
    exclude: ['@embedpdf/core', '@embedpdf/core/vue'],
  },
  worker: {
    format: 'es',
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    assetsDir: 'assets',
    rollupOptions: {
      output: {
        entryFileNames: 'orbit-reader.js',
        chunkFileNames: 'chunks/[name].js',
        assetFileNames: 'assets/[name][extname]',
      },
    },
  },
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
})
