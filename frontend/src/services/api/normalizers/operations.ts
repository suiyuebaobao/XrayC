// 本文件负责运营中心、健康状态和账本排行接口的响应标准化。
// 它把后端 snake_case/camelCase 混合字段转换成前端稳定类型。
// 探针策略的默认值和范围限制集中在这里，避免页面重复兜底。
// 本文件不发起请求，只处理数据转换和状态枚举归一化。

import type {
  AccessOperationsSettings,
  HealthStatus,
  OperationsEvent,
  OperationsLedgerRanking,
  OperationsLedgerRankingItem,
  OperationsLedgerRankingTotals,
  OperationsSummary,
  PlatformDatabase,
  PlatformDatabaseTable,
  PlatformDisk,
  PlatformMetrics,
  PlatformUsage,
  ProductionAlert,
  ProductionAlertSummary,
  OverviewSummary,
  TrafficChartPoint,
  TrafficEntitySummary,
  TrafficHealthSummary,
  TrafficWindowSummary,
} from '../types';
import {
  arrayValue,
  booleanValue,
  clampNumber,
  collectionValue,
  numberValue,
  optionalNumberValue,
  recordValue,
  stringValue,
} from '../primitives';

export function normalizeOperationsSummary(data: Record<string, unknown>): OperationsSummary {
  return {
    activeUsers: optionalNumberValue(data.activeUsers ?? data.active_users),
    activeAccessLines: optionalNumberValue(data.activeAccessLines ?? data.active_access_lines ?? data.access_line_count),
    healthyExitPools: optionalNumberValue(data.healthyExitPools ?? data.healthy_exit_pools ?? data.healthy_exit_pool_count),
    exitProbeProblemCount: optionalNumberValue(data.exitProbeProblemCount ?? data.exit_probe_problem_count),
    exitProbeStateCount: optionalNumberValue(data.exitProbeStateCount ?? data.exit_probe_state_count),
    monthlyBilledTrafficGb: optionalNumberValue(data.monthlyBilledTrafficGb ?? data.monthly_billed_traffic_gb),
    configDirtyNodes: optionalNumberValue(data.configDirtyNodes ?? data.config_dirty_nodes ?? data.dirty_access_nodes),
    onlineUsers: optionalNumberValue(data.onlineUsers ?? data.online_users),
    activeConnections: optionalNumberValue(data.activeConnections ?? data.active_connections),
    uniqueClientIpCount: optionalNumberValue(
      data.uniqueClientIpCount
      ?? data.unique_client_ip_count
      ?? data.uniqueClientIps
      ?? data.unique_client_ips
      ?? data.distinctClientIpCount
      ?? data.distinct_client_ip_count
      ?? data.clientIpCount
      ?? data.client_ip_count,
    ),
    uplinkRateBps: optionalNumberValue(data.uplinkRateBps ?? data.uplink_rate_bps ?? data.totalUplinkRateBps ?? data.total_uplink_rate_bps),
    downlinkRateBps: optionalNumberValue(
      data.downlinkRateBps ?? data.downlink_rate_bps ?? data.totalDownlinkRateBps ?? data.total_downlink_rate_bps,
    ),
    accessNodeCount: optionalNumberValue(data.accessNodeCount ?? data.access_node_count),
    accessLineCount: optionalNumberValue(data.accessLineCount ?? data.access_line_count),
    lineGroupCount: optionalNumberValue(data.lineGroupCount ?? data.line_group_count),
    exitPoolCount: optionalNumberValue(data.exitPoolCount ?? data.exit_pool_count),
    ledgerCount: optionalNumberValue(data.ledgerCount ?? data.ledger_count),
    generatedAt: stringValue(data.generatedAt ?? data.generated_at),
    latestMetricAt: stringValue(data.latestMetricAt ?? data.latest_metric_at),
    metricStatus: stringValue(data.metricStatus ?? data.metric_status),
    runtimeMetricStatus: normalizeRuntimeMetricStatus(data.runtimeMetricStatus ?? data.runtime_metric_status ?? data.metricStatus ?? data.metric_status),
    runtimeMetricLineCount: optionalNumberValue(data.runtimeMetricLineCount ?? data.runtime_metric_line_count),
    runtimeMetricExpectedLineCount: optionalNumberValue(data.runtimeMetricExpectedLineCount ?? data.runtime_metric_expected_line_count),
    trafficHealth: normalizeTrafficHealth(data.trafficHealth ?? data.traffic_health),
    alerts: arrayValue(data.alerts).map(normalizeProductionAlert),
    alertSummary: normalizeProductionAlertSummary(data.alertSummary ?? data.alert_summary),
    recentEvents: arrayValue(data.recentEvents ?? data.recent_events).map(normalizeOperationsEvent),
    settings: normalizeAccessOperationsSettings(recordValue(data.settings)),
  };
}

// 监控中心·平台资源指标标准化：CPU/内存/磁盘的千分比与字节用量保持原值（不在此做单位换算，
// 展示侧 pctMilli/1000 取百分号、字节走 formatBytes，环形/进度卡需要原始分量做精确占比）。
// host_mounted 缺省按 false（保守提示为容器视图），disk 带挂载点。database 给库总字节 + 前几大表明细。
export function normalizePlatformMetrics(data: Record<string, unknown>): PlatformMetrics {
  const cpu = recordValue(data.cpu);
  return {
    generatedAt: stringValue(data.generatedAt ?? data.generated_at),
    cpu: {
      pctMilli: clampPctMilli(numberValue(cpu.pctMilli ?? cpu.pct_milli)),
    },
    memory: normalizePlatformUsage(data.memory),
    disk: normalizePlatformDisk(data.disk),
    database: normalizePlatformDatabase(data.database),
  };
}

function normalizePlatformUsage(value: unknown): PlatformUsage {
  const data = recordValue(value);
  const usedPctMilli = clampPctMilli(numberValue(data.usedPctMilli ?? data.used_pct_milli));
  const rawFree = data.freePctMilli ?? data.free_pct_milli;
  // 后端缺剩余率字段时按 100% - 已用率兜底，保证剩余卡片始终有值。
  const freePctMilli =
    rawFree === undefined || rawFree === null
      ? Math.max(0, 100_000 - usedPctMilli)
      : clampPctMilli(numberValue(rawFree));
  return {
    usedBytes: Math.max(0, numberValue(data.usedBytes ?? data.used_bytes)),
    totalBytes: Math.max(0, numberValue(data.totalBytes ?? data.total_bytes)),
    usedPctMilli,
    freePctMilli,
  };
}

function normalizePlatformDisk(value: unknown): PlatformDisk {
  const data = recordValue(value);
  return {
    ...normalizePlatformUsage(value),
    mountPoint: stringValue(data.mountPoint ?? data.mount_point),
    // host_mounted 只有显式为 true 才算挂了宿主盘；缺省按容器视图提示升级面板。
    hostMounted: booleanValue(data.hostMounted ?? data.host_mounted),
  };
}

function normalizePlatformDatabase(value: unknown): PlatformDatabase {
  const data = recordValue(value);
  return {
    totalBytes: Math.max(0, numberValue(data.totalBytes ?? data.total_bytes)),
    tables: arrayValue(data.tables)
      .map(normalizePlatformDatabaseTable)
      .sort((left, right) => right.bytes - left.bytes),
  };
}

function normalizePlatformDatabaseTable(value: unknown): PlatformDatabaseTable {
  const data = recordValue(value);
  return {
    name: stringValue(data.name),
    bytes: Math.max(0, numberValue(data.bytes)),
  };
}

// 千分比统一钳制在 [0,100000]，防止后端越界值把环形/进度条画爆。
function clampPctMilli(value: number): number {
  return clampNumber(value, 0, 100_000);
}

export function normalizeOverview(data: Record<string, unknown>): OverviewSummary {
  return {
    activeUsers: numberValue(data.activeUsers ?? data.active_users),
    activeAccessLines: numberValue(data.activeAccessLines ?? data.active_access_lines ?? data.access_line_count),
    healthyExitPools: numberValue(data.healthyExitPools ?? data.healthy_exit_pools ?? data.healthy_exit_pool_count),
    monthlyBilledTrafficGb: numberValue(data.monthlyBilledTrafficGb ?? data.monthly_billed_traffic_gb),
    configDirtyNodes: numberValue(data.configDirtyNodes ?? data.config_dirty_nodes ?? data.dirty_access_nodes),
    accessNodeCount: optionalNumberValue(data.accessNodeCount ?? data.access_node_count),
    recentEvents: arrayValue(data.recentEvents ?? data.recent_events).map(normalizeOverviewEvent),
  };
}

export function normalizeOperationsLedgerRanking(data: Record<string, unknown>): OperationsLedgerRanking {
  return {
    source: stringValue(data.source) || 'usage_ledgers+usage_daily_rollups',
    limit: numberValue(data.limit) || 10,
    generatedAt: stringValue(data.generatedAt ?? data.generated_at),
    totals: normalizeOperationsLedgerRankingTotals(recordValue(data.totals)),
    items: collectionValue(data, ['items', 'rankings', 'line_rankings', 'access_line_rankings'])
      .map(normalizeOperationsLedgerRankingItem)
      .sort((left, right) => (
        right.billedBytes - left.billedBytes
        || right.realBytes - left.realBytes
        || right.ledgerCount - left.ledgerCount
        || left.accessLineName.localeCompare(right.accessLineName, 'zh-CN')
      ))
      .map((item, index) => ({ ...item, rank: index + 1 })),
  };
}

function normalizeOperationsLedgerRankingTotals(data: Record<string, unknown>): OperationsLedgerRankingTotals {
  return {
    accessLineCount: numberValue(data.accessLineCount ?? data.access_line_count),
    ledgerCount: numberValue(data.ledgerCount ?? data.ledger_count),
    deltaUplink: numberValue(data.deltaUplink ?? data.delta_uplink),
    deltaDownlink: numberValue(data.deltaDownlink ?? data.delta_downlink),
    realBytes: numberValue(data.realBytes ?? data.real_bytes ?? data.deltaTotal ?? data.delta_total),
    billedUplink: numberValue(data.billedUplink ?? data.billed_uplink),
    billedDownlink: numberValue(data.billedDownlink ?? data.billed_downlink),
    billedBytes: numberValue(data.billedBytes ?? data.billed_bytes),
    latestCollectedAt: stringValue(data.latestCollectedAt ?? data.latest_collected_at ?? data.collected_at),
  };
}

function normalizeOperationsLedgerRankingItem(value: unknown): OperationsLedgerRankingItem {
  const data = recordValue(value);
  return {
    rank: numberValue(data.rank),
    accessLineId: stringValue(data.accessLineId ?? data.access_line_id ?? data.id),
    accessLineName: stringValue(data.accessLineName ?? data.access_line_name ?? data.name),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name ?? data.accessNode ?? data.access_node),
    region: stringValue(data.region ?? data.regionName ?? data.region_name ?? data.regionCode ?? data.region_code),
    listenHost: stringValue(data.listenHost ?? data.listen_host),
    listenPort: numberValue(data.listenPort ?? data.listen_port),
    ledgerCount: numberValue(data.ledgerCount ?? data.ledger_count),
    deltaUplink: numberValue(data.deltaUplink ?? data.delta_uplink),
    deltaDownlink: numberValue(data.deltaDownlink ?? data.delta_downlink),
    realBytes: numberValue(data.realBytes ?? data.real_bytes ?? data.deltaTotal ?? data.delta_total),
    billedUplink: numberValue(data.billedUplink ?? data.billed_uplink),
    billedDownlink: numberValue(data.billedDownlink ?? data.billed_downlink),
    billedBytes: numberValue(data.billedBytes ?? data.billed_bytes),
    latestCollectedAt: stringValue(data.latestCollectedAt ?? data.latest_collected_at ?? data.collected_at),
  };
}

export function normalizeRuntimeMetricStatus(value: unknown): OperationsSummary['runtimeMetricStatus'] {
  const status = stringValue(value);
  return status === 'fresh' || status === 'stale' || status === 'no_data' ? status : 'unknown';
}

function normalizeTrafficHealth(value: unknown): TrafficHealthSummary {
  const data = recordValue(value);
  const windows = recordValue(data.windows);
  return {
    generatedAt: stringValue(data.generatedAt ?? data.generated_at),
    windows: {
      today: normalizeTrafficWindow(windows.today),
      week: normalizeTrafficWindow(windows.week),
      month: normalizeTrafficWindow(windows.month),
      total: normalizeTrafficWindow(windows.total),
    },
    lineItems: arrayValue(data.lineItems ?? data.line_items).map((item) => normalizeTrafficEntity(item, ['accessLineId', 'access_line_id'])),
    nodeItems: arrayValue(data.nodeItems ?? data.node_items).map((item) => normalizeTrafficEntity(item, ['accessNodeId', 'access_node_id'])),
    exitItems: arrayValue(data.exitItems ?? data.exit_items).map((item) => normalizeTrafficEntity(item, ['exitEndpointId', 'exit_endpoint_id'])),
    groupItems: arrayValue(data.groupItems ?? data.group_items).map((item) => normalizeTrafficEntity(item, ['lineGroupId', 'line_group_id'])),
  };
}

function normalizeTrafficWindow(value: unknown): TrafficWindowSummary {
  const data = recordValue(value);
  return {
    realBytes: numberValue(data.realBytes ?? data.real_bytes),
    billedBytes: numberValue(data.billedBytes ?? data.billed_bytes),
  };
}

function normalizeTrafficEntity(value: unknown, idKeys: string[]): TrafficEntitySummary {
  const data = recordValue(value);
  const id = idKeys.map((key) => stringValue(data[key])).find(Boolean) || stringValue(data.id);
  return {
    id,
    name: stringValue(data.name),
    todayRealBytes: numberValue(data.todayRealBytes ?? data.today_real_bytes),
    weekRealBytes: numberValue(data.weekRealBytes ?? data.week_real_bytes),
    monthRealBytes: numberValue(data.monthRealBytes ?? data.month_real_bytes),
    monthBilledBytes: numberValue(data.monthBilledBytes ?? data.month_billed_bytes),
    totalRealBytes: numberValue(data.totalRealBytes ?? data.total_real_bytes),
    totalBilledBytes: numberValue(data.totalBilledBytes ?? data.total_billed_bytes),
    peakHourRealBytes: numberValue(data.peakHourRealBytes ?? data.peak_hour_real_bytes),
    lowHourRealBytes: numberValue(data.lowHourRealBytes ?? data.low_hour_real_bytes),
    averageDailyRealBytes: numberValue(data.averageDailyRealBytes ?? data.average_daily_real_bytes),
    chart: arrayValue(data.chart ?? data.points).map(normalizeTrafficChartPoint),
  };
}

function normalizeTrafficChartPoint(value: unknown): TrafficChartPoint {
  const data = recordValue(value);
  return {
    windowStart: stringValue(data.windowStart ?? data.window_start),
    realBytes: numberValue(data.realBytes ?? data.real_bytes),
    billedBytes: numberValue(data.billedBytes ?? data.billed_bytes),
  };
}

export function normalizeHealthStatus(value: unknown): HealthStatus {
  const status = stringValue(value).toLowerCase();
  if (status === 'healthy' || status === 'syncing' || status === 'stale' || status === 'offline' || status === 'degraded') {
    return status;
  }
  return 'unknown';
}

export function normalizeAccessOperationsSettings(data: Record<string, unknown>): AccessOperationsSettings {
  const probePolicy = recordValue(data.probePolicy ?? data.probe_policy);
  const retention = recordValue(data.trafficLogRetention ?? data.traffic_log_retention);
  const backup = recordValue(data.databaseBackup ?? data.database_backup);
  const enabledValue =
    probePolicy.exitAutoFailoverEnabled
    ?? probePolicy.exit_auto_failover_enabled
    ?? probePolicy.autoFailoverEnabled
    ?? probePolicy.auto_failover_enabled;
  const pruneEnabledValue =
    retention.pruneEnabled
    ?? retention.prune_enabled
    ?? retention.enabled;
  const backupEnabledValue =
    backup.enabled
    ?? backup.autoBackupEnabled
    ?? backup.auto_backup_enabled;
  return {
    probePolicy: {
      exitAutoFailoverEnabled: enabledValue === undefined ? true : booleanValue(enabledValue),
      exitFailureThreshold: clampNumber(
        numberValue(probePolicy.exitFailureThreshold ?? probePolicy.exit_failure_threshold ?? probePolicy.failure_threshold) || 3,
        1,
        20,
      ),
      exitRecoveryThreshold: clampNumber(
        numberValue(probePolicy.exitRecoveryThreshold ?? probePolicy.exit_recovery_threshold ?? probePolicy.recovery_threshold) || 2,
        1,
        20,
      ),
      exitWindowMinutes: clampNumber(numberValue(probePolicy.exitWindowMinutes ?? probePolicy.exit_window_minutes ?? probePolicy.window_minutes) || 10, 1, 120),
      exitProbeIntervalSeconds: clampNumber(
        numberValue(probePolicy.exitProbeIntervalSeconds ?? probePolicy.exit_probe_interval_seconds ?? probePolicy.probe_interval_seconds) || 300,
        30,
        86400,
      ),
      probeQueueBatchSize: clampNumber(
        numberValue(probePolicy.probeQueueBatchSize ?? probePolicy.probe_queue_batch_size ?? probePolicy.queue_batch_size) || 100,
        1,
        1000,
      ),
      maxPendingProbeTasks: clampNumber(
        numberValue(probePolicy.maxPendingProbeTasks ?? probePolicy.max_pending_probe_tasks ?? probePolicy.max_pending_tasks) || 5,
        1,
        100,
      ),
    },
    trafficLogRetention: {
      detailRetentionDays: clampNumber(
        numberValue(retention.detailRetentionDays ?? retention.detail_retention_days ?? retention.retention_days) || 14,
        1,
        3650,
      ),
      pruneEnabled: pruneEnabledValue === undefined ? true : booleanValue(pruneEnabledValue),
      deleteBatchSize: clampNumber(
        numberValue(retention.deleteBatchSize ?? retention.delete_batch_size ?? retention.batch_size) || 5000,
        100,
        100000,
      ),
    },
    databaseBackup: {
      enabled: backupEnabledValue === undefined ? true : booleanValue(backupEnabledValue),
      intervalDays: clampNumber(
        numberValue(backup.intervalDays ?? backup.interval_days ?? backup.backup_interval_days) || 1,
        1,
        30,
      ),
      retentionDays: clampNumber(
        numberValue(backup.retentionDays ?? backup.retention_days ?? backup.backup_retention_days) || 30,
        1,
        3650,
      ),
    },
  };
}

function normalizeOperationsEvent(value: unknown, index: number): OperationsEvent {
  const data = recordValue(value);
  const rawLevel = stringValue(data.level ?? data.severity ?? data.type).toLowerCase();
  const level =
    rawLevel === 'danger' || rawLevel === 'error' || rawLevel === 'critical'
      ? 'danger'
      : rawLevel === 'warning' || rawLevel === 'warn'
        ? 'warning'
        : 'info';

  return {
    id: stringValue(data.id) || numberValue(data.id) || index + 1,
    level,
    title: stringValue(data.title ?? data.message ?? data.event),
    time: stringValue(data.time ?? data.created_at ?? data.occurred_at ?? data.generated_at),
  };
}

function normalizeProductionAlert(value: unknown): ProductionAlert {
  const data = recordValue(value);
  const severity = normalizeAlertSeverity(data.severity ?? data.level);
  return {
    id: stringValue(data.id) || `${stringValue(data.kind)}:${stringValue(data.resource_id ?? data.resourceId)}`,
    kind: stringValue(data.kind),
    severity,
    resourceType: stringValue(data.resourceType ?? data.resource_type),
    resourceId: stringValue(data.resourceId ?? data.resource_id),
    resourceName: stringValue(data.resourceName ?? data.resource_name),
    title: stringValue(data.title),
    message: stringValue(data.message),
    detectedAt: stringValue(data.detectedAt ?? data.detected_at ?? data.created_at),
  };
}

function normalizeProductionAlertSummary(value: unknown): ProductionAlertSummary {
  const data = recordValue(value);
  return {
    dangerCount: numberValue(data.dangerCount ?? data.danger_count),
    warningCount: numberValue(data.warningCount ?? data.warning_count),
    infoCount: numberValue(data.infoCount ?? data.info_count),
    totalCount: numberValue(data.totalCount ?? data.total_count),
    generatedAt: stringValue(data.generatedAt ?? data.generated_at),
  };
}

function normalizeAlertSeverity(value: unknown): ProductionAlert['severity'] {
  const severity = stringValue(value).toLowerCase();
  if (severity === 'danger' || severity === 'error' || severity === 'critical') {
    return 'danger';
  }
  if (severity === 'warning' || severity === 'warn') {
    return 'warning';
  }
  return 'info';
}

function normalizeOverviewEvent(value: unknown, index: number): OverviewSummary['recentEvents'][number] {
  const data = recordValue(value);
  const rawLevel = stringValue(data.level ?? data.severity);
  const level = rawLevel === 'danger' || rawLevel === 'warning' ? rawLevel : 'info';

  return {
    id: numberValue(data.id) || index + 1,
    level,
    title: stringValue(data.title ?? data.message ?? data.event) || '系统事件',
    time: stringValue(data.time ?? data.created_at ?? data.occurred_at),
  };
}
