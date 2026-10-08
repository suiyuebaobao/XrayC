// 本文件封装运营中心页面的数据加载、派生状态和用户动作。
// 它负责调用真实管理接口读取 summary、控制面和账本排行。
// 组件层只消费这里输出的响应式数据，避免页面 SFC 承担业务编排。
// 本文件不包含模板结构，便于后续继续拆分运营模块。
import { ElMessage } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import {
  apiClient,
  type AccessLine,
  type AccessNodeSummary,
  type AccessOperationsSettings,
  type ExitEndpointSummary,
  type ExitPool,
  type OperationsLedgerRanking,
  type OperationsSummary,
} from '@/services/api';
import {
  appendHint,
  dirtyNodeHint,
  formatKnownNumber,
  formatRuntimeNumber,
  formatRuntimeRate,
  formatTime,
  sumReported,
} from '@/views/operations/operationsFormatters';

export type ProbeTarget = {
  poolName: string;
  member: ExitEndpointSummary;
};

export type OperationsMetricCard = {
  label: string;
  value: string;
  hint: string;
};

const defaultSettings: AccessOperationsSettings = {
  probePolicy: {
    exitAutoFailoverEnabled: true,
    exitFailureThreshold: 3,
    exitRecoveryThreshold: 2,
    exitWindowMinutes: 10,
    exitProbeIntervalSeconds: 300,
    probeQueueBatchSize: 100,
    maxPendingProbeTasks: 5,
  },
  trafficLogRetention: {
    detailRetentionDays: 14,
    pruneEnabled: true,
    deleteBatchSize: 5000,
  },
  databaseBackup: {
    enabled: true,
    intervalDays: 1,
    retentionDays: 30,
  },
};

export function useOperationsPage(options: { includeDetails?: () => boolean; includeRanking?: () => boolean } = {}) {
  let loadGeneration = 0;
  const loading = ref(true);
  const summary = ref<OperationsSummary>();
  const ledgerRanking = ref<OperationsLedgerRanking | null>(null);
  const accessNodes = ref<AccessNodeSummary[] | null>(null);
  const accessLines = ref<AccessLine[] | null>(null);
  const exitPools = ref<ExitPool[] | null>(null);
  const summaryError = ref('');
  const ledgerRankingError = ref('');
  const nodeDetailError = ref('');
  const settingsError = ref('');
  const settingsMessage = ref('');
  const probeError = ref('');
  const probeMessage = ref('');
  const savingSettings = ref(false);
  const probingExitEndpointId = ref('');
  const settingsForm = ref<AccessOperationsSettings>(cloneSettings(defaultSettings));

  const dirtyNodes = computed(() => accessNodes.value?.filter((node) => node.configDirty) ?? []);
  const dirtyNodeCount = computed(() => (
    summary.value?.configDirtyNodes ?? accessNodes.value?.filter((node) => node.configDirty).length ?? null
  ));
  const recentEvents = computed(() => summary.value?.recentEvents ?? []);
  const productionAlerts = computed(() => summary.value?.alerts ?? []);
  const alertSummary = computed(() => summary.value?.alertSummary ?? {
    dangerCount: 0,
    warningCount: 0,
    infoCount: 0,
    totalCount: 0,
    generatedAt: '',
  });
  const ledgerRankingRows = computed(() => ledgerRanking.value?.items ?? []);
  const ledgerRankingTotals = computed(() => ledgerRanking.value?.totals ?? null);
  const ledgerRankingHint = computed(() => {
    const totals = ledgerRankingTotals.value;
    if (!totals) {
      return '按中转入口汇总';
    }
    return `覆盖中转入口 ${formatKnownNumber(totals.accessLineCount)} 个，账本 ${formatKnownNumber(totals.ledgerCount)} 条`;
  });
  const runtimeMetricStatus = computed(() => summary.value?.runtimeMetricStatus ?? 'unknown');
  const runtimeLines = computed(() => accessLines.value ?? []);
  const runtimeMetricCoverageHint = computed(() => {
    const reported = summary.value?.runtimeMetricLineCount;
    const expected = summary.value?.runtimeMetricExpectedLineCount ?? summary.value?.accessLineCount;
    if (reported === null || reported === undefined || expected === null || expected === undefined) {
      return '';
    }
    return `覆盖中转入口：${formatKnownNumber(reported)} / ${formatKnownNumber(expected)}`;
  });
  const runtimeMetricHint = computed(() => (
    appendHint(
      runtimeMetricStatus.value === 'fresh'
        ? `最近上报：${formatTime(summary.value?.latestMetricAt)}`
        : runtimeMetricStatus.value === 'stale'
          ? `已过期：${formatTime(summary.value?.latestMetricAt)}`
          : runtimeMetricStatus.value === 'no_data'
            ? '未收到节点运行数据'
            : '运行数据状态未知',
      runtimeMetricCoverageHint.value,
    )
  ));
  const runtimeMetricAlertTitle = computed(() => {
    if (runtimeMetricStatus.value === 'stale') {
      return '节点运行数据已过期（stale），在线用户、连接数、客户端 IP 和入口速率显示的是旧快照。';
    }
    if (runtimeMetricStatus.value === 'no_data') {
      return '未收到节点运行数据（no_data），在线用户、连接数、客户端 IP 和入口速率不是 0，而是未上报。';
    }
    return '节点运行数据状态未知（unknown），请检查运行上报链路。';
  });
  const runtimeMetricAlertType = computed(() => (runtimeMetricStatus.value === 'stale' ? 'warning' : 'info'));
  const runtimeRateTotals = computed(() => ({
    uplink: summary.value?.uplinkRateBps ?? sumReported(runtimeLines.value.map((line) => line.uplinkRateBps)),
    downlink: summary.value?.downlinkRateBps ?? sumReported(runtimeLines.value.map((line) => line.downlinkRateBps)),
  }));
  const operationProbeTargets = computed<ProbeTarget[]>(() => (
    (exitPools.value ?? []).flatMap((pool) => (
      pool.members.map((member) => ({
        poolName: pool.name || pool.uuid,
        member,
      }))
    ))
  ));

  const metricCards = computed<OperationsMetricCard[]>(() => [
    {
      label: '活跃用户',
      value: formatKnownNumber(summary.value?.activeUsers),
      hint: '未禁用用户数',
    },
    {
      label: '在线用户',
      value: formatRuntimeNumber(summary.value?.onlineUsers, runtimeMetricStatus.value),
      hint: runtimeMetricHint.value,
    },
    {
      label: '活跃连接',
      value: formatRuntimeNumber(summary.value?.activeConnections, runtimeMetricStatus.value),
      hint: runtimeMetricHint.value,
    },
    {
      label: '独立客户端 IP',
      value: formatRuntimeNumber(summary.value?.uniqueClientIpCount, runtimeMetricStatus.value),
      hint: runtimeMetricHint.value,
    },
    {
      label: '上行速率',
      value: formatRuntimeRate(runtimeRateTotals.value.uplink, runtimeMetricStatus.value),
      hint: runtimeMetricHint.value,
    },
    {
      label: '下行速率',
      value: formatRuntimeRate(runtimeRateTotals.value.downlink, runtimeMetricStatus.value),
      hint: runtimeMetricHint.value,
    },
    {
      label: '待同步节点',
      value: formatKnownNumber(dirtyNodeCount.value),
      hint: dirtyNodeHint(dirtyNodeCount.value),
    },
  ]);

  onMounted(load);

  async function load() {
    const generation = ++loadGeneration;
    loading.value = true;
    summaryError.value = '';
    ledgerRankingError.value = '';
    nodeDetailError.value = '';

    const [summaryResult, controlPlaneResult, ledgerRankingResult] = await Promise.allSettled([
      apiClient.getOperationsSummary(),
      options.includeDetails?.() === false ? Promise.resolve(null) : apiClient.getControlPlane(),
      options.includeRanking?.() === false ? Promise.resolve(null) : apiClient.getOperationsLedgerRanking(10),
    ]);

    if (generation !== loadGeneration) return;
    if (summaryResult.status === 'fulfilled') {
      summary.value = summaryResult.value;
      settingsForm.value = cloneSettings(summaryResult.value.settings);
    } else {
      summary.value = undefined;
      summaryError.value = errorText(summaryResult.reason, '运营 summary 加载失败');
    }

    if (controlPlaneResult.status === 'fulfilled' && controlPlaneResult.value) {
      accessNodes.value = controlPlaneResult.value.accessNodes;
      accessLines.value = controlPlaneResult.value.accessLines;
      exitPools.value = controlPlaneResult.value.exitPools;
    } else {
      accessNodes.value = null;
      accessLines.value = null;
      exitPools.value = null;
      nodeDetailError.value = controlPlaneResult.status === 'rejected' ? errorText(controlPlaneResult.reason, '中转节点明细未上报') : '';
    }

    if (ledgerRankingResult.status === 'fulfilled') {
      ledgerRanking.value = ledgerRankingResult.value;
    } else {
      ledgerRanking.value = null;
      ledgerRankingError.value = errorText(ledgerRankingResult.reason, '账本流量排行加载失败');
    }

    loading.value = false;
  }

  async function saveSettings() {
    savingSettings.value = true;
    settingsError.value = '';
    settingsMessage.value = '';
    try {
      const saved = await apiClient.updateAccessOperationsSettings(settingsForm.value);
      settingsForm.value = cloneSettings(saved);
      if (summary.value) {
        summary.value.settings = saved;
      }
      settingsMessage.value = '运营设置已保存';
    } catch (error) {
      settingsError.value = errorText(error, '运营设置保存失败');
    } finally {
      savingSettings.value = false;
    }
  }

  async function triggerOperationsProbe(target: ProbeTarget) {
    if (probingExitEndpointId.value) {
      return;
    }

    probingExitEndpointId.value = target.member.id;
    probeError.value = '';
    probeMessage.value = '';
    try {
      await apiClient.triggerAccessOperationsProbe(target.member.id);
      probeMessage.value = '已登记主动探测请求，等待节点运行组件拉取执行。';
      ElMessage.success('已登记主动探测请求');
      await load();
    } catch {
      probeError.value = '主动探测触发失败，请稍后重试。';
      ElMessage.error(probeError.value);
    } finally {
      probingExitEndpointId.value = '';
    }
  }

  return {
    accessNodes,
    dirtyNodeCount,
    dirtyNodes,
    ledgerRankingError,
    ledgerRankingHint,
    ledgerRankingRows,
    load,
    loading,
    metricCards,
    nodeDetailError,
    operationProbeTargets,
    probeError,
    probeMessage,
    probingExitEndpointId,
    productionAlerts,
    alertSummary,
    recentEvents,
    runtimeLines,
    runtimeMetricAlertTitle,
    runtimeMetricAlertType,
    runtimeMetricStatus,
    saveSettings,
    savingSettings,
    settingsError,
    settingsForm,
    settingsMessage,
    summary,
    summaryError,
    triggerOperationsProbe,
  };
}

function cloneSettings(settings: AccessOperationsSettings): AccessOperationsSettings {
  return {
    probePolicy: { ...settings.probePolicy },
    trafficLogRetention: { ...settings.trafficLogRetention },
    databaseBackup: { ...settings.databaseBackup },
  };
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? `${fallback}：${error.message}` : fallback;
}
