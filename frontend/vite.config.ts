import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Development requests use the same API paths as production to keep cookie behavior identical.
export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8091',
    },
  },
  test: {
    environment: 'jsdom',
  },
})
