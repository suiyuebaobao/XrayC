// 本文件封装后台健康检查页面的数据加载和健康状态推导。
// 它从管理接口读取真实中转节点、线路、分组和运行 summary。
// 这里不生成模拟数据，所有健康结论都基于心跳、探针和运行指标。
// 页面组件只消费这里输出的小卡片模型，避免单个 Vue 文件过长。
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import {
  apiClient,
  type AccessLine,
  type AccessNodeSummary,
  type ExitPool,
  type HealthStatus,
  type OperationsSummary,
} from '@/services/api';
import { ageSeconds, formatNumber, formatTime, probeStatusLabel, runtimeStatusLabel } from '@/views/health/format';

export type HealthCard = {
  title: string;
  value: string;
  hint: string;
  status: HealthStatus;
};

export type NodeCard = {
  node: AccessNodeSummary;
  status: HealthStatus;
  reason: string;
};

export type LineCard = {
  line: AccessLine;
  status: HealthStatus;
  reason: string;
};

const refreshIntervalMs = 15_000;

export function useHealthCheck() {
  const loading = ref(true);
  const errorMessage = ref('');
  const refreshedAt = ref('');
  const summary = ref<OperationsSummary | null>(null);
  const accessNodes = ref<AccessNodeSummary[]>([]);
  const accessLines = ref<AccessLine[]>([]);
  const exitPools = ref<ExitPool[]>([]);
  let refreshTimer: number | undefined;

  const nodeCards = computed<NodeCard[]>(() => (
    accessNodes.value.map((node) => ({
      node,
      status: node.healthStatus || inferNodeHealth(node),
      reason: node.healthReason || nodeHealthReason(node),
    }))
  ));

  const lineCards = computed<LineCard[]>(() => (
    accessLines.value.map((line) => {
      const node = accessNodes.value.find((item) => item.id === line.accessNodeId);
      const status = inferLineHealth(line, node);
      return { line, status, reason: lineHealthReason(line, node, status) };
    })
  ));

  const poolHealth = computed(() => {
    const pools = exitPools.value;
    return {
      healthy: pools.filter((pool) => pool.status === 'healthy').length,
      degraded: pools.filter((pool) => pool.status === 'degraded').length,
      offline: pools.filter((pool) => pool.status === 'offline').length,
      totalMembers: pools.reduce((sum, pool) => sum + pool.totalMembers, 0),
      healthyMembers: pools.reduce((sum, pool) => sum + pool.healthyMembers, 0),
    };
  });

  const nodeHealth = computed(() => countStatuses(nodeCards.value.map((item) => item.status)));
  const lineHealth = computed(() => countStatuses(lineCards.value.map((item) => item.status)));
  const probeProblemCount = computed(() => (
    accessLines.value.filter((line) => {
      const status = (line.probeStatus || '').toLowerCase();
      return status && status !== 'healthy' && status !== 'unknown';
    }).length + (summary.value?.exitProbeProblemCount ?? 0)
  ));

  const overviewCards = computed<HealthCard[]>(() => [
    {
      title: '中转节点',
      value: `${nodeHealth.value.healthy}/${accessNodes.value.length}`,
      hint: nodeHealthHint(nodeHealth.value),
      status: aggregateStatus(nodeHealth.value),
    },
    {
      title: '线路',
      value: `${lineHealth.value.healthy}/${accessLines.value.length}`,
      hint: lineHealthHint(lineHealth.value),
      status: aggregateStatus(lineHealth.value),
    },
    {
      title: '出口管理',
      value: `${poolHealth.value.healthyMembers}/${poolHealth.value.totalMembers}`,
      hint: `健康线路 ${poolHealth.value.healthyMembers} 条，离线分组 ${poolHealth.value.offline} 个`,
      status: poolHealthStatus(poolHealth.value),
    },
    {
      title: '运行数据',
      value: runtimeStatusLabel(summary.value?.runtimeMetricStatus ?? 'unknown'),
      hint: `最近指标 ${formatTime(summary.value?.latestMetricAt, '未上报')}`,
      status: runtimeStatusToHealth(summary.value?.runtimeMetricStatus ?? 'unknown'),
    },
    {
      title: '配置同步',
      value: formatNumber(summary.value?.configDirtyNodes),
      hint: '待节点拉取并应用的配置数',
      status: (summary.value?.configDirtyNodes ?? 0) > 0 ? 'syncing' : 'healthy',
    },
    {
      title: '探针异常',
      value: formatNumber(probeProblemCount.value),
      hint: `线路探针异常，线路状态 ${formatNumber(summary.value?.exitProbeStateCount)} 条`,
      status: probeProblemCount.value > 0 ? 'degraded' : 'healthy',
    },
  ]);

  onMounted(() => {
    void load();
    refreshTimer = window.setInterval(() => void load(false), refreshIntervalMs);
  });

  onBeforeUnmount(() => {
    if (refreshTimer) {
      window.clearInterval(refreshTimer);
    }
  });

  async function load(showLoading = true) {
    if (showLoading) {
      loading.value = true;
    }
    errorMessage.value = '';
    try {
      const [summaryResult, controlPlane] = await Promise.all([
        apiClient.getOperationsSummary(),
        apiClient.getControlPlane(),
      ]);
      summary.value = summaryResult;
      accessNodes.value = controlPlane.accessNodes;
      accessLines.value = controlPlane.accessLines;
      exitPools.value = controlPlane.exitPools;
      refreshedAt.value = new Date().toISOString();
    } catch (error) {
      errorMessage.value = error instanceof Error && error.message
        ? `健康数据加载失败：${error.message}`
        : '健康数据加载失败';
    } finally {
      loading.value = false;
    }
  }

  return { loading, errorMessage, refreshedAt, summary, overviewCards, nodeCards, lineCards, exitPools, load };
}

function inferNodeHealth(node: AccessNodeSummary): HealthStatus {
  if (!node.lastHeartbeatAt) {
    return 'offline';
  }
  const age = ageSeconds(node.lastHeartbeatAt, node.heartbeatAgeSeconds);
  if (age !== null && age > 300) {
    return 'offline';
  }
  if (age !== null && age > 90) {
    return 'stale';
  }
  return node.configDirty || !node.configSynced ? 'syncing' : 'healthy';
}

function inferLineHealth(line: AccessLine, node?: AccessNodeSummary): HealthStatus {
  if (line.status === 'disabled') {
    return 'offline';
  }
  const nodeStatus = node ? (node.healthStatus || inferNodeHealth(node)) : 'unknown';
  const probeStatus = (line.probeStatus || '').toLowerCase();
  if (nodeStatus === 'offline' || probeStatus === 'unhealthy' || probeStatus === 'timeout' || probeStatus === 'error') {
    return 'offline';
  }
  if (nodeStatus === 'syncing' || nodeStatus === 'stale') {
    return 'degraded';
  }
  if (line.metricStatus === 'stale' || line.metricStatus === 'no_data') {
    return 'stale';
  }
  return !line.metricCollectedAt || probeStatus === 'unknown' || !probeStatus ? 'degraded' : 'healthy';
}

function nodeHealthReason(node: AccessNodeSummary) {
  const status = inferNodeHealth(node);
  if (status === 'offline') {
    return node.lastHeartbeatAt ? '心跳超过 5 分钟未刷新' : '未收到心跳';
  }
  if (status === 'stale') {
    return '心跳超过 90 秒未刷新';
  }
  return status === 'syncing' ? node.configDirtyReason || '配置等待应用' : '心跳和配置同步正常';
}

function lineHealthReason(line: AccessLine, node: AccessNodeSummary | undefined, status: HealthStatus) {
  if (status === 'offline') {
    if (line.status === 'disabled') {
      return '线路已禁用';
    }
    if (node && (node.healthStatus || inferNodeHealth(node)) === 'offline') {
      return '所属中转节点离线';
    }
    return probeStatusLabel(line.probeStatus);
  }
  if (status === 'stale') {
    return '运行指标已过期';
  }
  if (status === 'degraded') {
    return node?.configDirty ? '节点配置同步中' : '缺少最新指标或探针';
  }
  return '线路、节点和探针正常';
}

function countStatuses(statuses: HealthStatus[]) {
  return {
    healthy: statuses.filter((status) => status === 'healthy').length,
    degraded: statuses.filter((status) => status === 'degraded' || status === 'syncing').length,
    stale: statuses.filter((status) => status === 'stale').length,
    offline: statuses.filter((status) => status === 'offline').length,
    unknown: statuses.filter((status) => status === 'unknown').length,
  };
}

function aggregateStatus(counts: ReturnType<typeof countStatuses>): HealthStatus {
  if (counts.offline > 0) {
    return 'offline';
  }
  if (counts.degraded > 0 || counts.stale > 0) {
    return 'degraded';
  }
  return counts.unknown > 0 ? 'unknown' : 'healthy';
}

function runtimeStatusToHealth(status: OperationsSummary['runtimeMetricStatus']): HealthStatus {
  if (status === 'fresh') {
    return 'healthy';
  }
  if (status === 'stale') {
    return 'stale';
  }
  return status === 'no_data' ? 'offline' : 'unknown';
}

function poolHealthStatus(pool: { degraded: number; offline: number }): HealthStatus {
  if (pool.offline > 0) {
    return 'offline';
  }
  return pool.degraded > 0 ? 'degraded' : 'healthy';
}

function nodeHealthHint(counts: ReturnType<typeof countStatuses>) {
  if (counts.offline > 0) {
    return `${counts.offline} 个离线`;
  }
  return counts.stale > 0 ? `${counts.stale} 个心跳过期` : '心跳正常';
}

function lineHealthHint(counts: ReturnType<typeof countStatuses>) {
  if (counts.offline > 0) {
    return `${counts.offline} 条不可用`;
  }
  const problemCount = counts.degraded + counts.stale;
  return problemCount > 0 ? `${problemCount} 条降级` : '线路健康';
}
