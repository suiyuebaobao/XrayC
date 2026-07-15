// 本文件维护运营中心、账本排行和审计日志 API 方法。
// 它只负责接口调用和 normalizer 组合，不包含页面状态和图表逻辑。
// 健康检查页面复用这里的 summary 与 control-plane 数据。
// 新增运行态检查接口时，应放在这里保持运营能力集中。

import type {
  AccessOperationsSettings,
  AdminAuditLog,
  OperationsLedgerRanking,
  OperationsSummary,
  PlatformMetrics,
} from '../types';
import { request } from '../http';
import { appendQueryParam, collectionValue } from '../primitives';
import { normalizeAdminAuditLog } from '../normalizers/audit';
import {
  normalizeAccessOperationsSettings,
  normalizeOperationsLedgerRanking,
  normalizeOperationsSummary,
  normalizeOverview,
  normalizePlatformMetrics,
} from '../normalizers/operations';
import { plannedApiPaths, rustApiPaths } from '../paths';

export const operationsApi = {
  async getOverview() {
    const data = await request<Record<string, unknown>>(rustApiPaths.overview);
    return normalizeOverview(data);
  },

  async getOperationsSummary(): Promise<OperationsSummary> {
    const data = await request<Record<string, unknown>>(rustApiPaths.accessOperationsSummary);
    return normalizeOperationsSummary(data);
  },

  async getOperationsLedgerRanking(limit = 10): Promise<OperationsLedgerRanking> {
    const params = new URLSearchParams();
    appendQueryParam(params, 'limit', limit);
    const path = params.toString()
      ? `${rustApiPaths.accessOperationsLedgerRanking}?${params.toString()}`
      : rustApiPaths.accessOperationsLedgerRanking;
    const data = await request<Record<string, unknown>>(path);
    return normalizeOperationsLedgerRanking(data);
  },

  async getPlatformMetrics(): Promise<PlatformMetrics> {
    const data = await request<Record<string, unknown>>(rustApiPaths.accessOperationsPlatformMetrics);
    return normalizePlatformMetrics(data);
  },

  async getAccessOperationsSettings(): Promise<AccessOperationsSettings> {
    const data = await request<Record<string, unknown>>(rustApiPaths.accessOperationsSettings);
    return normalizeAccessOperationsSettings(data);
  },

  async updateAccessOperationsSettings(settings: AccessOperationsSettings): Promise<AccessOperationsSettings> {
    const data = await request<Record<string, unknown>>(rustApiPaths.accessOperationsSettings, {
      method: 'PUT',
      body: JSON.stringify({
        probe_policy: {
          exit_auto_failover_enabled: settings.probePolicy.exitAutoFailoverEnabled,
          exit_failure_threshold: settings.probePolicy.exitFailureThreshold,
          exit_recovery_threshold: settings.probePolicy.exitRecoveryThreshold,
          exit_window_minutes: settings.probePolicy.exitWindowMinutes,
          exit_probe_interval_seconds: settings.probePolicy.exitProbeIntervalSeconds,
          probe_queue_batch_size: settings.probePolicy.probeQueueBatchSize,
          max_pending_probe_tasks: settings.probePolicy.maxPendingProbeTasks,
        },
        traffic_log_retention: {
          detail_retention_days: settings.trafficLogRetention.detailRetentionDays,
          prune_enabled: settings.trafficLogRetention.pruneEnabled,
          delete_batch_size: settings.trafficLogRetention.deleteBatchSize,
        },
        database_backup: {
          enabled: settings.databaseBackup.enabled,
          interval_days: settings.databaseBackup.intervalDays,
          retention_days: settings.databaseBackup.retentionDays,
        },
      }),
    });
    return normalizeAccessOperationsSettings(data);
  },

  async triggerAccessOperationsProbe(exitEndpointId: string) {
    await request<Record<string, unknown>>(rustApiPaths.accessOperationsProbe, {
      method: 'POST',
      body: JSON.stringify({ exit_endpoint_id: exitEndpointId }),
    });
  },

  async getAdminAuditLogs(): Promise<AdminAuditLog[]> {
    const data = await request<unknown>(plannedApiPaths.adminAuditLogs);
    return collectionValue(data, ['auditLogs', 'audit_logs', 'logs', 'items', 'data']).map(normalizeAdminAuditLog);
  },
};
