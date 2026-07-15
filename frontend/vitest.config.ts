// 中文说明：前端单元测试（Vitest）配置。
// 只跑 src 下 *.spec.ts 纯函数单测（node 环境，无需浏览器/DOM）。
// 复用 vite 的 @ → src 路径别名，保证被测模块里的 @/ 引用可解析。
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.spec.ts'],
  },
});
