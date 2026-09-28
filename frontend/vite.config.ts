// 使用 vitest/config 的 defineConfig，让 vite.config.ts 中的 test 字段获得完整类型支持。
import { defineConfig } from 'vitest/config'
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
