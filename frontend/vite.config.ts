// 中文说明：Vite 前端构建配置入口。
// 本文件只维护插件、路径别名和本地开发代理。
// 修改代理目标时必须同步检查真实后端 smoke。
import { fileURLToPath, URL } from 'node:url';
import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [vue(), {
    name: 'xrayc-release-info',
    generateBundle() {
      this.emitFile({ type: 'asset', fileName: 'release.json', source: JSON.stringify({ release_id: process.env.VITE_BUILD_ID || 'development' }) });
    },
  }],
  build: {
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('/node_modules/')) {
            return undefined;
          }
          if (id.includes('/node_modules/vue') || id.includes('/node_modules/vue-router') || id.includes('/node_modules/pinia')) {
            return 'vue-vendor';
          }
          if (id.includes('/node_modules/element-plus') || id.includes('/node_modules/@element-plus')) {
            return 'element-plus';
          }
          if (id.includes('/node_modules/@vueuse')) {
            return 'vueuse';
          }
          return 'vendor';
        },
      },
    },
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': process.env.VITE_UI_PREVIEW === 'true' ? 'http://127.0.0.1:18880' : process.env.VITE_API_PROXY_TARGET || 'http://127.0.0.1:8080',
      '^/sub(?:/|$)': process.env.VITE_UI_PREVIEW === 'true' ? 'http://127.0.0.1:18880' : process.env.VITE_API_PROXY_TARGET || 'http://127.0.0.1:8080',
    },
  },
});
