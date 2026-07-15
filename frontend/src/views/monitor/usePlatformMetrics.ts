// 本文件封装监控中心「平台资源」tab 的数据加载与派生状态。
// 它只调用真实只读接口 GET /api/admin/access-operations/platform-metrics，
// 拿控制面 CPU/内存/磁盘占用率与数据库存储占用，不生成任何模拟数据。
// 页面壳只消费这里输出的响应式 metrics 与 load/loading/error，避免 SFC 承担编排。
import { ref } from 'vue';
import { apiClient, type PlatformMetrics } from '@/services/api';

export function usePlatformMetrics() {
  const loading = ref(false);
  const error = ref('');
  const metrics = ref<PlatformMetrics | null>(null);

  async function load() {
    loading.value = true;
    error.value = '';
    try {
      metrics.value = await apiClient.getPlatformMetrics();
    } catch (err) {
      metrics.value = null;
      error.value = err instanceof Error && err.message
        ? `平台资源指标加载失败：${err.message}`
        : '平台资源指标加载失败';
    } finally {
      loading.value = false;
    }
  }

  return { loading, error, metrics, load };
}
