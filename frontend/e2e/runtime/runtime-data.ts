/*
 * 用途：归一化 runtime no-mock e2e 读取到的真实后端响应。
 * 测试断言通过这些类型与格式化函数兼容新旧字段命名。
 */
import {
  collection,
  type JsonRecord,
  numberValue,
  optionalNumberValue,
  recordValue,
  stringValue,
} from './runtime-values';

export type SubscriptionInfo = {
  token: string;
  planName: string;
  expiresAt: string;
  trafficGb: TrafficInfo;
  visibleLines: SubscriptionLine[];
};

export type TrafficInfo = {
  used: number;
  total: number;
};

export type SubscriptionLine = {
  name: string;
  region: string;
  server: string;
  port: number;
  protocol: string;
};

export type OperationsSummary = {
  activeUsers: number | null;
  activeAccessLines: number | null;
  healthyExitPools: number | null;
  monthlyBilledTrafficGb: number | null;
  configDirtyNodes: number | null;
  onlineUsers: number | null;
  activeConnections: number | null;
  accessNodeCount: number | null;
  accessLineCount: number | null;
  exitPoolCount: number | null;
  exitProbeProblemCount: number | null;
  exitProbeStateCount: number | null;
  ledgerCount: number | null;
  runtimeMetricStatus: 'fresh' | 'stale' | 'no_data' | 'unknown';
};

export type ControlPlane = {
  accessLines: AccessLine[];
  exitPools: ExitPool[];
  lineGroups: LineGroup[];
};

export type AccessLine = {
  id: string;
  uuid: string;
  name: string;
  region: string;
  accessNode: string;
  lineGroupName: string;
  exitEndpointName: string;
  listenHost: string;
  listenPort: number;
  inboundProtocol: string;
  networkMode: string;
  status: string;
  exitPoolName: string;
  onlineUsers: number | null;
  activeConnections: number | null;
  uniqueClientIpCount: number | null;
  latencyMs: number | null;
};

export type ExitPool = {
  uuid: string;
  name: string;
  region: string;
  status: string;
  healthyMembers: number;
  totalMembers: number;
  strategy: string;
  activeAssignments: number;
};

export type ExitEndpointInfo = {
  id: string;
  name: string;
  resourceName: string;
  outboundType: string;
  host: string;
  port: number;
  outboundConfig: JsonRecord;
  streamConfig: JsonRecord;
  probeConfig: JsonRecord;
};

export type LineGroup = {
  id: string;
  exitEndpointIds: string[];
};

export type PlanInfo = {
  id: string;
  name: string;
  isDefault: boolean;
  enabled: boolean;
  trafficLimitGb: number;
  billingMultiplier: number;
  priceCents: number;
  currency: string;
  durationDays: number;
  isDeleted: boolean;
};

export function normalizeSubscription(value: unknown): SubscriptionInfo {
  const data = recordValue(value);
  return {
    token: stringValue(data.token ?? data.subscription_token),
    planName: stringValue(data.planName ?? data.plan_name) || '当前套餐',
    expiresAt: stringValue(data.expiresAt ?? data.expires_at),
    trafficGb: normalizeTraffic(data.trafficGb ?? data.traffic_gb ?? data.traffic),
    visibleLines: collection(data.visibleLines ?? data.visible_lines ?? data.access_lines, []).map((line) => {
      const normalized = normalizeAccessLine(line);
      return {
        name: normalized.name,
        region: normalized.region,
        server: normalized.listenHost,
        port: normalized.listenPort,
        protocol: normalized.inboundProtocol,
      };
    }),
  };
}

export function normalizeOperationsSummary(value: unknown): OperationsSummary {
  const outer = recordValue(value);
  const nestedData = recordValue(outer.data);
  const nestedSummary = recordValue(outer.summary);
  const data = Object.keys(nestedSummary).length > 0
    ? nestedSummary
    : Object.keys(nestedData).length > 0
      ? nestedData
      : outer;
  return {
    activeUsers: optionalNumberValue(data.activeUsers ?? data.active_users),
    activeAccessLines: optionalNumberValue(data.activeAccessLines ?? data.active_access_lines ?? data.access_line_count),
    healthyExitPools: optionalNumberValue(
      data.healthyExitPools ?? data.healthy_exit_pools ?? data.healthy_exit_pool_count,
    ),
    monthlyBilledTrafficGb: optionalNumberValue(data.monthlyBilledTrafficGb ?? data.monthly_billed_traffic_gb),
    configDirtyNodes: optionalNumberValue(data.configDirtyNodes ?? data.config_dirty_nodes ?? data.dirty_access_nodes),
    onlineUsers: optionalNumberValue(data.onlineUsers ?? data.online_users),
    activeConnections: optionalNumberValue(data.activeConnections ?? data.active_connections),
    accessNodeCount: optionalNumberValue(data.accessNodeCount ?? data.access_node_count),
    accessLineCount: optionalNumberValue(data.accessLineCount ?? data.access_line_count),
    exitPoolCount: optionalNumberValue(data.exitPoolCount ?? data.exit_pool_count),
    exitProbeProblemCount: optionalNumberValue(data.exitProbeProblemCount ?? data.exit_probe_problem_count),
    exitProbeStateCount: optionalNumberValue(data.exitProbeStateCount ?? data.exit_probe_state_count),
    ledgerCount: optionalNumberValue(data.ledgerCount ?? data.ledger_count),
    runtimeMetricStatus: normalizeRuntimeStatus(
      data.runtimeMetricStatus ?? data.runtime_metric_status ?? data.metricStatus ?? data.metric_status,
    ),
  };
}

export function normalizeControlPlane(value: unknown): ControlPlane {
  const data = recordValue(value);
  return {
    accessLines: collection(data.accessLines ?? data.access_lines ?? data.items, []).map(normalizeAccessLine),
    exitPools: collection(data.exitPools ?? data.exit_pools, []).map(normalizeExitPool),
    lineGroups: collection(data.lineGroups ?? data.line_groups, []).map((lineGroup) => {
      const item = recordValue(lineGroup);
      return {
        id: stringValue(item.id),
        exitEndpointIds: collection(
          item.exitEndpointIds
          ?? item.exit_endpoint_ids
          ?? item.lineIds
          ?? item.line_ids,
          [],
        ).map(stringValue).filter(Boolean),
      };
    }),
  };
}

export function normalizeAccessLine(value: unknown): AccessLine {
  const data = recordValue(value);
  const status = stringValue(data.status);
  return {
    id: stringValue(data.id),
    uuid: stringValue(data.uuid ?? data.id),
    name: stringValue(data.name),
    region: stringValue(data.region ?? data.region_name ?? data.region_code),
    accessNode: stringValue(data.accessNode ?? data.access_node ?? data.access_node_name),
    lineGroupName: stringValue(data.lineGroupName ?? data.line_group_name),
    exitEndpointName: stringValue(data.exitEndpointName ?? data.exit_endpoint_name),
    listenHost: stringValue(data.listenHost ?? data.listen_host ?? data.server),
    listenPort: numberValue(data.listenPort ?? data.listen_port ?? data.port),
    inboundProtocol: stringValue(data.inboundProtocol ?? data.inbound_protocol ?? data.protocol),
    networkMode: stringValue(data.networkMode ?? data.network_mode ?? data.transport ?? data.network),
    status: status === 'disabled' || status === 'degraded' ? status : 'enabled',
    exitPoolName: stringValue(data.exitPoolName ?? data.exit_pool_name),
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
    latencyMs: optionalNumberValue(data.latencyMs ?? data.latency_ms),
  };
}

export function normalizeExitPools(value: unknown): ExitPool[] {
  const data = recordValue(value);
  return collection(Array.isArray(value) ? value : data.exitPools ?? data.exit_pools ?? data.items, [])
    .map(normalizeExitPool);
}

export function normalizeExitEndpoints(value: unknown): ExitEndpointInfo[] {
  return collection(value, ['exitEndpoints', 'exit_endpoints', 'items']).map(normalizeExitEndpoint);
}

export function normalizePlans(value: unknown): PlanInfo[] {
  const data = recordValue(value);
  return collection(Array.isArray(value) ? value : data.plans ?? data.items ?? data.data, []).map(normalizePlan);
}

export function formatKnownNumber(value: number | null) {
  return value === null ? '未知' : new Intl.NumberFormat('zh-CN').format(value);
}

export function formatRuntimeNumber(summary: OperationsSummary) {
  return formatRuntimeMetricNumber(summary.onlineUsers, summary.runtimeMetricStatus);
}

export function formatRuntimeMetricNumber(
  value: number | null,
  runtimeMetricStatus: OperationsSummary['runtimeMetricStatus'],
) {
  if (runtimeMetricStatus === 'no_data') {
    return '未上报';
  }
  if (runtimeMetricStatus === 'unknown') {
    return '未知';
  }
  return formatReportedTableNumber(value);
}

export function formatUserPlanTraffic(plan: PlanInfo) {
  return formatGb(plan.trafficLimitGb);
}

export function formatGb(value: number) {
  return `${Number(value.toFixed(value >= 10 ? 0 : 2))} GB`;
}

export function formatPrice(plan: PlanInfo) {
  return `${plan.currency} ${Number((plan.priceCents / 100).toFixed(2))}`;
}

export function multiplierLabel(value: number) {
  return `${Number(value.toFixed(3))}x`;
}

function normalizeExitPool(value: unknown): ExitPool {
  const data = recordValue(value);
  const status = stringValue(data.status);
  const members = collection(data.members, []);
  return {
    uuid: stringValue(data.uuid ?? data.id),
    name: stringValue(data.name),
    region: stringValue(data.region ?? data.region_name ?? data.region_code),
    status: status === 'degraded' || status === 'offline' ? status : 'healthy',
    healthyMembers: numberValue(data.healthyMembers ?? data.healthy_members),
    totalMembers: numberValue(data.totalMembers ?? data.total_members) || members.length,
    strategy: stringValue(data.strategy),
    activeAssignments: numberValue(data.activeAssignments ?? data.active_assignments),
  };
}

function normalizeExitEndpoint(value: unknown): ExitEndpointInfo {
  const data = recordValue(value);
  return {
    id: stringValue(data.id),
    name: stringValue(data.name),
    resourceName: stringValue(data.resourceName ?? data.resource_name ?? data.exit_resource_name),
    outboundType: stringValue(data.outboundType ?? data.outbound_type),
    host: stringValue(data.host),
    port: numberValue(data.port),
    outboundConfig: recordValue(data.outboundConfig ?? data.outbound_config),
    streamConfig: recordValue(data.streamConfig ?? data.stream_config),
    probeConfig: recordValue(data.probeConfig ?? data.probe_config),
  };
}

function normalizePlan(value: unknown): PlanInfo {
  const data = recordValue(value);
  const trafficBytes = numberValue(data.trafficLimitBytes ?? data.traffic_limit_bytes);
  return {
    id: stringValue(data.id),
    name: stringValue(data.name) || '未命名套餐',
    isDefault: data.isDefault === true || data.is_default === true,
    enabled: data.enabled !== false,
    trafficLimitGb: bytesToGb(trafficBytes),
    billingMultiplier: numberValue(data.billingMultiplier ?? data.billing_multiplier) || 1,
    priceCents: numberValue(data.priceCents ?? data.price_cents ?? data.amountCents ?? data.amount_cents),
    currency: stringValue(data.currency) || 'USDT',
    durationDays: numberValue(data.durationDays ?? data.duration_days) || 30,
    isDeleted: data.isDeleted === true || data.is_deleted === true,
  };
}

function normalizeTraffic(value: unknown): TrafficInfo {
  const data = recordValue(value);
  return {
    used: numberValue(data.used ?? data.used_gb),
    total: numberValue(data.total ?? data.total_gb ?? data.limit_gb),
  };
}

function normalizeRuntimeStatus(value: unknown): OperationsSummary['runtimeMetricStatus'] {
  const status = stringValue(value);
  if (status === 'fresh' || status === 'stale' || status === 'no_data') {
    return status;
  }
  return 'unknown';
}

function formatReportedTableNumber(value: number | null) {
  return value === null ? '未上报' : new Intl.NumberFormat('zh-CN').format(value);
}

function bytesToGb(value: number) {
  return value / 1024 / 1024 / 1024;
}
