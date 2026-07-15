// 本文件封装监控中心「节点资源」面板的今日业务流量加载。
// 它按「当天本地 0 点 → now」调 getNodeTraffic，并按 access_node_id 映射成每行可查的字典。
// 组件层只消费 trafficByNode 字典（某节点无数据即取不到，按 0 展示），不在面板里写请求。
// 抽成 composable 与运营 useOperationsPage 一致，保持 NodeResourcePanel 纯 props 可测。
import { ref } from 'vue';
import { apiClient, type NodeTrafficSummary } from '@/services/api';
import { resolveTrafficRange } from '@/views/monitor/nodeTraffic';

export function useNodeTraffic() {
  const loading = ref(false);
  const error = ref('');
  // 节点 id → 今日流量汇总；面板每行按 row.id 取值，取不到即视为无数据（0）。
  const trafficByNode = ref<Record<string, NodeTrafficSummary>>({});

  async function load() {
    loading.value = true;
    error.value = '';
    try {
      const { fromMs, toMs } = resolveTrafficRange('today', Date.now());
      const rows = await apiClient.getNodeTraffic(fromMs, toMs);
      const next: Record<string, NodeTrafficSummary> = {};
      for (const row of rows) {
        if (row.accessNodeId) {
          next[row.accessNodeId] = row;
        }
      }
      trafficByNode.value = next;
    } catch (err) {
      trafficByNode.value = {};
      error.value = err instanceof Error && err.message ? `节点业务流量加载失败：${err.message}` : '节点业务流量加载失败';
    } finally {
      loading.value = false;
    }
  }

  return { loading, error, trafficByNode, load };
}
