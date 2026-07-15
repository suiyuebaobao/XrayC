// 本文件维护后台「数据库备份」管理相关 API 方法。
// 它只组织请求参数并调用 backup normalizer，不实现真实备份/SSH/邮件逻辑。
// 写值敏感字段（SSH 密码/附件口令）由 normalizer 控制不回显、留空不提交（写时保留）。
// 新增备份接口时应放在这里，而不是页面组件里。

import type {
  BackupActiveMode,
  BackupConfig,
  BackupOffsiteConfig,
  BackupProvisionResult,
  BackupRunResult,
  BackupState,
} from '../types';
import { request } from '../http';
import { isRecord } from '../primitives';
import {
  backupConfigPayload,
  backupProvisionPayload,
  normalizeBackupConfig,
  normalizeBackupProvisionResult,
  normalizeBackupRunResult,
  normalizeBackupState,
} from '../normalizers/backup';
import { plannedApiPaths } from '../paths';

export const backupApi = {
  async getBackupConfig(): Promise<BackupConfig> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminBackupConfig);
    return normalizeBackupConfig(data);
  },

  async updateBackupConfig(config: BackupConfig): Promise<BackupConfig> {
    const data = await request<unknown>(plannedApiPaths.adminBackupConfig, {
      method: 'PUT',
      body: JSON.stringify(backupConfigPayload(config)),
    });
    return isRecord(data) ? normalizeBackupConfig(data) : config;
  },

  async testAndProvisionBackup(offsite: BackupOffsiteConfig): Promise<BackupProvisionResult> {
    const data = await request<unknown>(plannedApiPaths.adminBackupTestProvision, {
      method: 'POST',
      body: JSON.stringify(backupProvisionPayload(offsite)),
    });
    return normalizeBackupProvisionResult(data);
  },

  async runBackupNow(mode: BackupActiveMode): Promise<BackupRunResult> {
    const params = new URLSearchParams({ mode });
    const data = await request<unknown>(`${plannedApiPaths.adminBackupRunNow}?${params.toString()}`, {
      method: 'POST',
    });
    return normalizeBackupRunResult(data);
  },

  async getBackupState(): Promise<BackupState> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminBackupState);
    return normalizeBackupState(data);
  },
};
