// 本文件定义运营中心、健康检查和审计日志相关的前端 API 类型。
// 这些类型描述运行态 summary、账本排行、探针策略和审计列表。
// 文件不包含接口请求逻辑，避免业务类型和 HTTP 行为继续耦合。
// 页面和 normalizer 通过统一类型出口复用这里的定义。

export type OverviewSummary = {
  activeUsers: number;
  activeAccessLines: number;
  healthyExitPools: number;
  monthlyBilledTrafficGb: number;
  configDirtyNodes: number;
  accessNodeCount: number | null;
  recentEvents: Array<{
    id: number;
    level: 'info' | 'warning' | 'danger';
    title: string;
    time: string;
  }>;
};

export type OperationsEvent = {
  id: number | string;
  level: 'info' | 'warning' | 'danger';
  title: string;
  time: string;
};

export type ProductionAlert = {
  id: string;
  kind: string;
  severity: 'info' | 'warning' | 'danger';
  resourceType: string;
  resourceId: string;
  resourceName: string;
  title: string;
  message: string;
  detectedAt: string;
};

export type ProductionAlertSummary = {
  dangerCount: number;
  warningCount: number;
  infoCount: number;
  totalCount: number;
  generatedAt: string;
};

export type RuntimeMetricStatus = 'fresh' | 'stale' | 'no_data' | 'unknown';
export type HealthStatus = 'healthy' | 'syncing' | 'stale' | 'offline' | 'degraded' | 'unknown';

export type TrafficWindowSummary = {
  realBytes: number;
  billedBytes: number;
};

export type TrafficChartPoint = {
  windowStart: string;
  realBytes: number;
  billedBytes: number;
};

export type TrafficEntitySummary = {
  id: string;
  name: string;
  todayRealBytes: number;
  weekRealBytes: number;
  monthRealBytes: number;
  monthBilledBytes: number;
  totalRealBytes: number;
  totalBilledBytes: number;
  peakHourRealBytes: number;
  lowHourRealBytes: number;
  averageDailyRealBytes: number;
  chart: TrafficChartPoint[];
};

export type TrafficHealthSummary = {
  generatedAt: string;
  windows: {
    today: TrafficWindowSummary;
    week: TrafficWindowSummary;
    month: TrafficWindowSummary;
    total: TrafficWindowSummary;
  };
  lineItems: TrafficEntitySummary[];
  nodeItems: TrafficEntitySummary[];
  exitItems: TrafficEntitySummary[];
  groupItems: TrafficEntitySummary[];
};

export type AccessOperationsSettings = {
  probePolicy: {
    exitAutoFailoverEnabled: boolean;
    exitFailureThreshold: number;
    exitRecoveryThreshold: number;
    exitWindowMinutes: number;
    exitProbeIntervalSeconds: number;
    probeQueueBatchSize: number;
    maxPendingProbeTasks: number;
  };
  trafficLogRetention: {
    detailRetentionDays: number;
    pruneEnabled: boolean;
    deleteBatchSize: number;
  };
  databaseBackup: {
    enabled: boolean;
    intervalDays: number;
    retentionDays: number;
  };
};

export type OperationsSummary = {
  activeUsers: number | null;
  activeAccessLines: number | null;
  healthyExitPools: number | null;
  exitProbeProblemCount: number | null;
  exitProbeStateCount: number | null;
  monthlyBilledTrafficGb: number | null;
  configDirtyNodes: number | null;
  onlineUsers: number | null;
  activeConnections: number | null;
  uniqueClientIpCount: number | null;
  uplinkRateBps: number | null;
  downlinkRateBps: number | null;
  accessNodeCount: number | null;
  accessLineCount: number | null;
  lineGroupCount: number | null;
  exitPoolCount: number | null;
  ledgerCount: number | null;
  generatedAt: string;
  latestMetricAt: string;
  metricStatus: string;
  runtimeMetricStatus: RuntimeMetricStatus;
  runtimeMetricLineCount: number | null;
  runtimeMetricExpectedLineCount: number | null;
  trafficHealth: TrafficHealthSummary;
  alerts: ProductionAlert[];
  alertSummary: ProductionAlertSummary;
  recentEvents: OperationsEvent[];
  settings: AccessOperationsSettings;
};

export type OperationsLedgerRankingItem = {
  rank: number;
  accessLineId: string;
  accessLineName: string;
  accessNodeName: string;
  region: string;
  listenHost: string;
  listenPort: number;
  ledgerCount: number;
  deltaUplink: number;
  deltaDownlink: number;
  realBytes: number;
  billedUplink: number;
  billedDownlink: number;
  billedBytes: number;
  latestCollectedAt: string;
};

export type OperationsLedgerRankingTotals = {
  accessLineCount: number;
  ledgerCount: number;
  deltaUplink: number;
  deltaDownlink: number;
  realBytes: number;
  billedUplink: number;
  billedDownlink: number;
  billedBytes: number;
  latestCollectedAt: string;
};

export type OperationsLedgerRanking = {
  source: string;
  limit: number;
  generatedAt: string;
  totals: OperationsLedgerRankingTotals;
  items: OperationsLedgerRankingItem[];
};

export type AdminAuditLog = {
  id: number | string;
  createdAt: string;
  actor: string;
  action: string;
  resource: string;
  result: string;
  summary: string;
};

// 监控中心·平台资源占用率分量：千分比原值（pctMilli /1000 才是百分比）+ 字节用量。
export type PlatformUsage = {
  usedBytes: number;
  totalBytes: number;
  // 已用占比千分比（0..100000），展示时 /1000 得到百分号数值。
  usedPctMilli: number;
  // 剩余占比千分比，与 usedPctMilli 互补，缺字段时由前端 100000-used 兜底。
  freePctMilli: number;
};

// 监控中心·控制面 CPU 占用率（仅千分比，pctMilli /1000 为百分比）。
export type PlatformCpu = {
  pctMilli: number;
};

// 监控中心·磁盘占用（含挂载点与是否挂的宿主盘；host_mounted=false 时面板是容器视图）。
export type PlatformDisk = PlatformUsage & {
  mountPoint: string;
  hostMounted: boolean;
};

// 监控中心·数据库存储占用：库总字节 + 占用前几大表的明细。
export type PlatformDatabaseTable = {
  name: string;
  bytes: number;
};

export type PlatformDatabase = {
  totalBytes: number;
  tables: PlatformDatabaseTable[];
};

// 控制面平台资源指标读模型，对应 GET /api/admin/access-operations/platform-metrics。
export type PlatformMetrics = {
  generatedAt: string;
  cpu: PlatformCpu;
  memory: PlatformUsage;
  disk: PlatformDisk;
  database: PlatformDatabase;
};
