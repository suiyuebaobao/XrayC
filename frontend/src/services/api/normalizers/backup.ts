// 本文件负责「数据库备份」配置/状态响应的标准化与提交序列化。
// 它把后端 snake_case 字段转成前端稳定类型，敏感写值字段不回显只读 `*_set`。
// 提交时密码/口令留空则删除该键，后端保留旧值（写时保留），避免页面回显覆盖。
// 本文件不发起请求、不保存凭据，只做纯数据转换。

import type {
  BackupConfig,
  BackupDestination,
  BackupEmailConfig,
  BackupEmailOversizeMode,
  BackupFullConfig,
  BackupHistoryEntry,
  BackupModeState,
  BackupOffsiteConfig,
  BackupProvisionResult,
  BackupRunResult,
  BackupSchedule,
  BackupState,
  BackupWalConfig,
} from '../types';
import {
  arrayValue,
  booleanValue,
  numberValue,
  recordValue,
  stringArrayValue,
  stringValue,
} from '../primitives';

// 归一化存储位置：只认 local/offsite/both，其余（含缺失）回退 local。
function normalizeDestination(value: unknown): BackupDestination {
  const raw = stringValue(value);
  return raw === 'offsite' || raw === 'both' ? raw : 'local';
}

// 归一化邮件超限处理：只认 notify/split，其余（含缺失）回退 notify。
function normalizeOversizeMode(value: unknown): BackupEmailOversizeMode {
  return stringValue(value) === 'split' ? 'split' : 'notify';
}

// 归一化调度：kind 只认 interval/cron，间隔非法时回退调用方给的默认值。
function normalizeSchedule(value: unknown, fallbackIntervalMinutes: number): BackupSchedule {
  const data = recordValue(value);
  const intervalMinutes = numberValue(data.intervalMinutes ?? data.interval_minutes);
  return {
    kind: stringValue(data.kind) === 'cron' ? 'cron' : 'interval',
    intervalMinutes: intervalMinutes >= 1 ? Math.floor(intervalMinutes) : fallbackIntervalMinutes,
    cron: stringValue(data.cron),
  };
}

function normalizeOffsite(value: unknown): BackupOffsiteConfig {
  const data = recordValue(value);
  const sshPort = numberValue(data.sshPort ?? data.ssh_port);
  const retentionDays = numberValue(data.retentionDays ?? data.retention_days);
  return {
    sshHost: stringValue(data.sshHost ?? data.ssh_host),
    sshPort: sshPort >= 1 ? sshPort : 22,
    sshUser: stringValue(data.sshUser ?? data.ssh_user) || 'root',
    // SSH 密码写值不回显，永远清空输入框，仅靠 *Set 标记是否已配置。
    sshPassword: '',
    sshPasswordSet: booleanValue(data.sshPasswordSet ?? data.ssh_password_set),
    remoteDir: stringValue(data.remoteDir ?? data.remote_dir) || '/var/backups/xrayc',
    pubkeyInstalled: booleanValue(data.pubkeyInstalled ?? data.pubkey_installed),
    pubkeyFingerprint: stringValue(data.pubkeyFingerprint ?? data.pubkey_fingerprint),
    retentionDays: retentionDays >= 1 ? retentionDays : 30,
  };
}

function normalizeFull(value: unknown): BackupFullConfig {
  const data = recordValue(value);
  const retentionDays = numberValue(data.retentionDays ?? data.retention_days);
  const enabledRaw = data.enabled;
  return {
    // 全量默认开启（首版兼容旧 database_backup），后端未给时按 true。
    enabled: enabledRaw === undefined ? true : booleanValue(enabledRaw),
    excludeTrafficLogs: booleanValue(data.excludeTrafficLogs ?? data.exclude_traffic_logs),
    schedule: normalizeSchedule(data.schedule, 1440),
    retentionDays: retentionDays >= 1 ? retentionDays : 30,
    destination: normalizeDestination(data.destination),
  };
}

function normalizeWal(value: unknown): BackupWalConfig {
  const data = recordValue(value);
  const retentionFull = numberValue(data.retentionFull ?? data.retention_full);
  return {
    enabled: booleanValue(data.enabled),
    configured: booleanValue(data.configured),
    fullBackupSchedule: normalizeSchedule(data.fullBackupSchedule ?? data.full_backup_schedule, 10080),
    retentionFull: retentionFull >= 1 ? retentionFull : 4,
    destination: normalizeDestination(data.destination),
  };
}

function normalizeEmail(value: unknown): BackupEmailConfig {
  const data = recordValue(value);
  const maxAttachMb = numberValue(data.maxAttachMb ?? data.max_attach_mb);
  const attachEncryptedRaw = data.attachEncrypted ?? data.attach_encrypted;
  const notifySuccessRaw = data.notifyOnSuccess ?? data.notify_on_success;
  const notifyFailureRaw = data.notifyOnFailure ?? data.notify_on_failure;
  return {
    enabled: booleanValue(data.enabled),
    recipients: stringArrayValue(data.recipients),
    // 邮件独立调度，默认每天（1440 分钟）；缺失/非法由 normalizeSchedule 回退成 interval + 1440。
    schedule: normalizeSchedule(data.schedule, 1440),
    attachEncrypted: attachEncryptedRaw === undefined ? true : booleanValue(attachEncryptedRaw),
    // 附件口令写值不回显，永远清空输入框，仅靠 *Set 标记是否已配置。
    attachPassphrase: '',
    attachPassphraseSet: booleanValue(data.attachPassphraseSet ?? data.attach_passphrase_set),
    maxAttachMb: maxAttachMb >= 1 ? maxAttachMb : 20,
    notifyOnSuccess: notifySuccessRaw === undefined ? true : booleanValue(notifySuccessRaw),
    notifyOnFailure: notifyFailureRaw === undefined ? true : booleanValue(notifyFailureRaw),
    oversizeMode: normalizeOversizeMode(data.oversizeMode ?? data.oversize_mode),
  };
}

export function normalizeBackupConfig(data: Record<string, unknown>): BackupConfig {
  return {
    offsite: normalizeOffsite(data.offsite),
    full: normalizeFull(data.full),
    wal: normalizeWal(data.wal),
    email: normalizeEmail(data.email),
  };
}

// 序列化调度：只发后端需要的字段（cron 始终带，间隔按 kind 生效）。
function schedulePayload(schedule: BackupSchedule) {
  const intervalMinutes = Number.isFinite(schedule.intervalMinutes) && schedule.intervalMinutes >= 1
    ? Math.floor(schedule.intervalMinutes)
    : 1;
  return {
    kind: schedule.kind,
    interval_minutes: intervalMinutes,
    cron: schedule.cron.trim(),
  };
}

// 组装 PUT /config 请求体：写值密码/口令留空则不传（后端保留旧值），
// 后端派生的只读态（pubkey_installed/pubkey_fingerprint/configured）不回传。
export function backupConfigPayload(config: BackupConfig) {
  // 异地段：只发共享连接 + 保留天数（不再有 enabled/schedule）；密码留空则不传（写时保留）。
  const offsite: Record<string, unknown> = {
    ssh_host: config.offsite.sshHost.trim(),
    ssh_port: config.offsite.sshPort,
    ssh_user: config.offsite.sshUser.trim(),
    remote_dir: config.offsite.remoteDir.trim(),
    retention_days: config.offsite.retentionDays,
  };
  if (config.offsite.sshPassword) {
    offsite.ssh_password = config.offsite.sshPassword;
  }

  const email: Record<string, unknown> = {
    enabled: config.email.enabled,
    recipients: config.email.recipients,
    // 邮件独立调度序列化回后端（snake_case schedule，内部 interval_minutes/kind/cron），与全量/WAL 同口径。
    schedule: schedulePayload(config.email.schedule),
    attach_encrypted: config.email.attachEncrypted,
    max_attach_mb: config.email.maxAttachMb,
    notify_on_success: config.email.notifyOnSuccess,
    notify_on_failure: config.email.notifyOnFailure,
    oversize_mode: config.email.oversizeMode,
  };
  if (config.email.attachPassphrase) {
    email.attach_passphrase = config.email.attachPassphrase;
  }

  return {
    offsite,
    full: {
      enabled: config.full.enabled,
      exclude_traffic_logs: config.full.excludeTrafficLogs,
      schedule: schedulePayload(config.full.schedule),
      retention_days: config.full.retentionDays,
      destination: config.full.destination,
    },
    wal: {
      enabled: config.wal.enabled,
      full_backup_schedule: schedulePayload(config.wal.fullBackupSchedule),
      retention_full: config.wal.retentionFull,
      destination: config.wal.destination,
    },
    email,
  };
}

// 组装 test-and-provision 请求体：只发异地机连接子集，密码留空则不传（后端用已存密码）。
export function backupProvisionPayload(offsite: BackupOffsiteConfig) {
  const body: Record<string, unknown> = {
    ssh_host: offsite.sshHost.trim(),
    ssh_port: offsite.sshPort,
    ssh_user: offsite.sshUser.trim(),
    remote_dir: offsite.remoteDir.trim(),
  };
  if (offsite.sshPassword) {
    body.ssh_password = offsite.sshPassword;
  }
  return body;
}

export function normalizeBackupProvisionResult(data: unknown): BackupProvisionResult {
  const record = recordValue(data);
  return {
    ok: booleanValue(record.ok),
    fingerprint: stringValue(record.fingerprint ?? record.pubkeyFingerprint ?? record.pubkey_fingerprint),
    pubkeyInstalled: booleanValue(record.pubkeyInstalled ?? record.pubkey_installed),
    message: stringValue(record.message),
  };
}

export function normalizeBackupRunResult(data: unknown): BackupRunResult {
  const record = recordValue(data);
  return {
    // 后端可能只回 202/空体，未显式给 ok 时视为已受理。
    ok: record.ok === undefined ? true : booleanValue(record.ok),
    message: stringValue(record.message),
  };
}

// 单条备份历史归一：snake→camel（size_bytes→sizeBytes / offsite_synced→offsiteSynced）；
// 字节非负兜底；文件名/错误空串折成 null；异地保持三态（布尔透传，缺失/ null → null 表无记录）。
function normalizeHistoryEntry(value: unknown): BackupHistoryEntry {
  const data = recordValue(value);
  const sizeBytes = numberValue(data.sizeBytes ?? data.size_bytes);
  const offsiteRaw = data.offsiteSynced ?? data.offsite_synced;
  const file = stringValue(data.file);
  const error = stringValue(data.error);
  return {
    at: stringValue(data.at),
    ok: booleanValue(data.ok),
    sizeBytes: sizeBytes >= 0 ? sizeBytes : 0,
    file: file || null,
    offsiteSynced: typeof offsiteRaw === 'boolean' ? offsiteRaw : null,
    error: error || null,
  };
}

// 备份历史列表归一：非数组（含缺失）一律回退空数组，逐条 normalize；保持后端顺序（最新在前）。
function normalizeHistory(value: unknown): BackupHistoryEntry[] {
  return arrayValue(value).map(normalizeHistoryEntry);
}

function normalizeModeState(value: unknown): BackupModeState {
  const data = recordValue(value);
  // WAL 归档成功标记是三态：后端给布尔则透传，缺失/ null 则表示尚无记录。
  const archiveOkRaw = data.walArchiveOk ?? data.wal_last_archive_ok;
  return {
    status: stringValue(data.status) || 'idle',
    lastStartedAt: stringValue(data.lastStartedAt ?? data.last_started_at),
    lastSuccessAt: stringValue(data.lastSuccessAt ?? data.last_success_at),
    lastFailedAt: stringValue(data.lastFailedAt ?? data.last_failed_at),
    errorSummary: stringValue(data.errorSummary ?? data.error_summary),
    lastFileName: stringValue(data.lastFileName ?? data.last_file_name),
    // 异地同步结果并入全量·WAL 的 state。
    offsiteSynced: booleanValue(data.offsiteSynced ?? data.offsite_synced),
    offsiteError: stringValue(data.offsiteError ?? data.offsite_error),
    offsiteLastSyncAt: stringValue(data.offsiteLastSyncAt ?? data.offsite_last_sync_at),
    emailLastSentAt: stringValue(data.emailLastSentAt ?? data.email_last_sent_at),
    emailAttachmentName: stringValue(data.emailAttachmentName ?? data.attachment_name),
    walArchiveOk: typeof archiveOkRaw === 'boolean' ? archiveOkRaw : null,
    history: normalizeHistory(data.history),
  };
}

export function normalizeBackupState(data: Record<string, unknown>): BackupState {
  return {
    full: normalizeModeState(data.full),
    wal: normalizeModeState(data.wal),
    email: normalizeModeState(data.email),
  };
}
