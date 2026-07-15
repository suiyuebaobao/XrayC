// 本文件定义后台「数据库备份」管理页的前端 API 类型契约。
// 它描述脱敏后的备份配置、各模式运行状态、测试装公钥结果的数据结构。
// 敏感项（SSH 密码 / 邮件附件口令）写值不回显，改用布尔 `*Set` 标记是否已设置。
// 文件不发起请求、不保存凭据，仅作为 normalizer 与页面之间的类型契约。

// 调度类型：按固定间隔（分钟）或按 5 段 cron 表达式触发。
export type BackupScheduleKind = 'interval' | 'cron';

export type BackupSchedule = {
  kind: BackupScheduleKind;
  // 间隔分钟数（kind=interval 生效，取代旧的 interval_days，≥1）。
  intervalMinutes: number;
  // 5 段 cron 表达式（kind=cron 生效）。
  cron: string;
};

// 备份存储位置：本机 / 异地 / 本机+异地（全量、WAL 各自选，默认本机）。
export type BackupDestination = 'local' | 'offsite' | 'both';

// 邮件附件超限处理：仅通知（默认，推荐）/ 自动分片发送。
export type BackupEmailOversizeMode = 'notify' | 'split';

// 异地备份服务器（rsync 共享目标）：只保存 SSH 连接信息 + 异地保留天数。
// 异地不再是独立备份模式（无自身开关/调度）；全量、WAL 通过 destination 决定是否推到这台机。
export type BackupOffsiteConfig = {
  sshHost: string;
  sshPort: number;
  sshUser: string;
  // SSH 密码写值不回显，仅用布尔标记是否已设置；提交时留空表示沿用旧值（写时保留）。
  sshPassword: string;
  sshPasswordSet: boolean;
  remoteDir: string;
  // 公钥是否已装到异地机（由 test-and-provision 装成功置 true），及展示用指纹。
  pubkeyInstalled: boolean;
  pubkeyFingerprint: string;
  // 异地机上保留最近多少天的备份文件。
  retentionDays: number;
};

// 全量 pg_dump 配置。
export type BackupFullConfig = {
  enabled: boolean;
  // 排除观测大表（只删数据保结构），减小 dump 体积。
  excludeTrafficLogs: boolean;
  schedule: BackupSchedule;
  retentionDays: number;
  // 存储位置：本机 / 异地 / 本机+异地。
  destination: BackupDestination;
};

// WAL / PITR（base backup + WAL 归档）配置。
export type BackupWalConfig = {
  enabled: boolean;
  // archive_mode / 归档链路是否就绪（后端派生，只读展示）。
  configured: boolean;
  fullBackupSchedule: BackupSchedule;
  // 保留几个 base backup。
  retentionFull: number;
  // 存储位置：本机 / 异地 / 本机+异地。
  destination: BackupDestination;
};

// 邮件发文件配置。
export type BackupEmailConfig = {
  enabled: boolean;
  recipients: string[];
  // 邮件是完全独立的备份方式，有自己的调度（与全量/WAL 同类型，四档：按间隔/每天/每周/每月），不跟随全量。
  schedule: BackupSchedule;
  attachEncrypted: boolean;
  // 附件口令写值不回显，仅用布尔标记是否已设置；提交时留空表示沿用旧值（写时保留）。
  attachPassphrase: string;
  attachPassphraseSet: boolean;
  maxAttachMb: number;
  notifyOnSuccess: boolean;
  notifyOnFailure: boolean;
  // 附件超上限时：仅通知（推荐）/ 自动分片发送。
  oversizeMode: BackupEmailOversizeMode;
};

// 脱敏后的整份备份配置（GET/PUT /api/admin/backup/config）。
export type BackupConfig = {
  offsite: BackupOffsiteConfig;
  full: BackupFullConfig;
  wal: BackupWalConfig;
  email: BackupEmailConfig;
};

// 备份能力标识：含历史 offsite 一档，仅供 runModeLabel 等中文查表用（backupSchedule.ts 依赖）。
export type BackupRunMode = 'full' | 'offsite' | 'wal' | 'email';

// 真正的运行模式（运行状态表行 / run-now / state 键）：异地已并入全量·WAL，不再单列。
export type BackupActiveMode = 'full' | 'wal' | 'email';

// 单条备份历史记录（后端 shape_backup_state 把 history 数组并进每个方式对象，最新在前、最多 30 条）：
// 空可选字段（文件名/错误）归一为 null；异地三态（已推/未推/无异地记录）用 boolean | null 表达。
export type BackupHistoryEntry = {
  // 备份发生时间（RFC3339）。
  at: string;
  // 该次是否成功。
  ok: boolean;
  // 产物字节数（失败或无产物时为 0）。
  sizeBytes: number;
  // 备份文件名，无则 null。
  file: string | null;
  // 是否已推异地：true 已推 / false 未推 / null 该方式无异地记录。
  offsiteSynced: boolean | null;
  // 脱敏后的错误摘要，无则 null。
  error: string | null;
};

// 单个模式的最近运行状态（时间字段空串表示尚无记录）。
export type BackupModeState = {
  status: string;
  lastStartedAt: string;
  lastSuccessAt: string;
  lastFailedAt: string;
  // 脱敏后的错误摘要（绝不含密码/连接串/host）。
  errorSummary: string;
  lastFileName: string;
  // 异地同步结果并入全量·WAL 的 state（后端 worker 按模式补写）：
  // 最近一次异地 rsync 是否成功、脱敏错误、同步时间。
  offsiteSynced: boolean;
  offsiteError: string;
  offsiteLastSyncAt: string;
  // 邮件最近发送时间 + 最近附件名。
  emailLastSentAt: string;
  emailAttachmentName: string;
  // WAL 最近归档是否成功（null 表示尚无记录）。
  walArchiveOk: boolean | null;
  // 该方式最近若干次备份历史（最新在前，最多 30 条；无记录为空数组）。
  history: BackupHistoryEntry[];
};

// 各模式运行状态汇总（GET /api/admin/backup/state）；异地不再单列一段。
export type BackupState = {
  full: BackupModeState;
  wal: BackupModeState;
  email: BackupModeState;
};

// 测试异地机 SSH 连通并装公钥的返回（脱敏）。
export type BackupProvisionResult = {
  ok: boolean;
  fingerprint: string;
  pubkeyInstalled: boolean;
  message: string;
};

// 立即触发一次备份的返回（脱敏）。
export type BackupRunResult = {
  ok: boolean;
  message: string;
};
