import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Development requests use the same API paths as production to keep cookie behavior identical.
export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8091',
      // Local visual QA can switch mock phases without exposing the endpoint in production builds.
      '/mock-phase': 'http://127.0.0.1:8091',
    },
  },
  test: {
    environment: 'jsdom',
  },
})
