// 本文件定义中转节点、入口管理、出口管理和部署相关的前端 API 类型。
// 这些类型是后台中转控制面、健康检查和套餐分组授权共同使用的结构。
// 文件只描述数据形状，不包含请求、格式化或 UI 逻辑。
// 管理员后台可读取线路明文配置，用户订阅和公开接口仍不能暴露出口。

import type { HealthStatus, RuntimeMetricStatus } from './operations';
import type { NodeDomain, NodeDomainInput, SelectedNodeDomain } from './nodeDomains';

export type AccessLine = {
  id: number;
  uuid: string;
  name: string;
  region: string;
  accessNode: string;
  accessNodeId: string;
  exitEndpointId: string;
  exitEndpointName: string;
  lineGroupId: string;
  lineGroupName: string;
  listenHost: string;
  listenPort: number;
  inboundProtocol: string;
  networkMode: string;
  xhttpMode: string;
  udpEnabled: boolean;
  status: 'enabled' | 'disabled' | 'degraded';
  exitPoolName: string;
  exitPoolId: string;
  onlineUsers: number | null;
  runtimeOnlineUsers: number | null;
  runtimeActiveConnections: number | null;
  uniqueClientIpCount: number | null;
  runtimeUniqueClientIpCount: number | null;
  uplinkRateBps: number | null;
  downlinkRateBps: number | null;
  metricStatus: RuntimeMetricStatus;
  metricCollectedAt: string;
  latencyMs: number | null;
  probeLatencyMs: number | null;
  probeStatus: string;
  lastProbeAt: string;
  probeErrorSummary: string;
  udpPacketEncoding: string;
};

export type AccessEntrySummary = {
  id: string;
  accessNodeId: string;
  accessNodeName: string;
  name: string;
  listenHost: string;
  listenPort: number;
  protocol: string;
  transport: string;
  security: string;
  serverName: string;
  // VLESS 量子加密（后量子）是否开启：后端按 vless_decryption 非空派生为该 bool 返回，仅非 Reality 有效。
  vlessQuantumEncryption: boolean;
  wsPath: string;
  wsHost: string;
  cdnEnabled: boolean;
  cdnProvider: string;
  cdnHostname: string;
  enabled: boolean;
  sortWeight: number;
  // 选中的节点域名 id（免证书入口留空）。
  nodeDomainId: string;
  // 解析出的选中域名（id+domain+kind），后端读模型 node_domain 字段。
  nodeDomain: SelectedNodeDomain | null;
  createdAt: string;
  updatedAt: string;
};

export type AccessEntryPayload = {
  access_node_id: string;
  name?: string;
  listen_host?: string;
  listen_port: number;
  protocol?: string;
  transport?: string;
  security?: string;
  server_name?: string;
  // VLESS 量子加密（后量子）开关；后端据此生成/清空 vless_decryption + vless_encryption。
  vless_quantum_encryption?: boolean;
  ws_path?: string;
  ws_host?: string;
  cdn_enabled?: boolean;
  cdn_provider?: string;
  cdn_hostname?: string;
  enabled?: boolean;
  sort_weight?: number;
  // 选中的节点域名 id（免证书入口留空）；后端 Phase 6 对齐同名字段。
  node_domain_id?: string | null;
};

export type AccessEntryExitBindingSummary = {
  id: string;
  accessEntryId: string;
  accessEntryName: string;
  accessNodeId: string;
  accessNodeName: string;
  exitEndpointId: string;
  exitEndpointName: string;
  exitPoolId: string;
  exitPoolName: string;
  name: string;
  enabled: boolean;
  sortWeight: number;
  remark: string;
  createdAt: string;
  updatedAt: string;
};

export type AccessEntryExitBindingPayload = {
  exit_endpoint_id: string;
  exit_pool_id?: string;
  name?: string;
  enabled?: boolean;
  sort_weight?: number;
  remark?: string;
};

export type LineGroupBindingNodePayload = {
  binding_node_ids: string[];
};

export type ExitPool = {
  id: number;
  uuid: string;
  name: string;
  region: string;
  status: 'healthy' | 'degraded' | 'offline';
  healthyMembers: number;
  totalMembers: number;
  strategy: string;
  activeAssignments: number;
  members: ExitEndpointSummary[];
};

export type ExitPoolMemberStatus = 'healthy' | 'degraded' | 'draining' | 'offline' | 'unknown';

export type CurrentExitEndpointOutboundType = 'socks' | 'http' | 'vless' | 'trojan' | 'shadowsocks' | 'hysteria';

export type LegacyExitEndpointOutboundType = 'direct';

export type ExitEndpointOutboundType = LegacyExitEndpointOutboundType | CurrentExitEndpointOutboundType;

export type ThirdPartyExitEndpointOutboundType = CurrentExitEndpointOutboundType;

export type ExitEndpointConfig = Record<string, unknown>;

export type CreateExitEndpointPayload = {
  exit_resource_id: string;
  name?: string;
  outbound_type: ThirdPartyExitEndpointOutboundType;
  host?: string;
  port?: number;
  outbound_config?: ExitEndpointConfig;
  stream_config?: ExitEndpointConfig;
  probe_config?: ExitEndpointConfig;
  enabled?: boolean;
};

export type UpdateExitEndpointPayload = {
  exit_resource_id?: string;
  name?: string;
  outbound_type?: ThirdPartyExitEndpointOutboundType;
  host?: string;
  port?: number;
  outbound_config?: ExitEndpointConfig;
  stream_config?: ExitEndpointConfig;
  probe_config?: ExitEndpointConfig;
  enabled?: boolean;
};

export type AccessNodeSummary = {
  id: string;
  name: string;
  publicHost: string;
  publicPort: number;
  sshHost: string;
  remark: string;
  ipDirectAddress: string;
  certDomain: string;
  acmeEmail: string;
  cfEnabled: boolean;
  cfDomain: string;
  // 多域名清单：后端读模型按 kind 聚合的直连/CF 域名列表（含证书状态、主域名标记）。
  domains: NodeDomain[];
  status: string;
  agentVersion: string;
  configDirty: boolean;
  configSynced: boolean;
  lastHeartbeatAt: string;
  lastTrafficReportAt: string;
  lastTrafficSuccessAt: string;
  heartbeatAgeSeconds: number | null;
  configDirtyAt: string;
  configDirtyReason: string;
  healthStatus: HealthStatus;
  healthReason: string;
  tlsCertificates: AccessNodeTlsCertificate[];
  tlsCertLastReportAt: string;
  tlsRenewRequestId: string;
  tlsRenewRequestedAt: string;
  tlsRenewCompletedAt: string;
  tlsRenewStatus: string;
  tlsRenewMessage: string;
  // 内核能力与重启状态：connmark 不可用或内核待升级时前端提示需重启；reboot* 跟踪重启请求生命周期。
  kernelConnmarkAvailable: boolean;
  kernelUpgradePending: boolean;
  rebootStatus: string;
  rebootMessage: string;
  rebootRequestedAt: string;
  rebootCompletedAt: string;
  // 监控中心·节点资源：每节点最新一条运行态指标，后端无数据时为 null（未上报/待 agent 升级）。
  runtimeMetrics: NodeRuntimeMetrics | null;
};

// 监控中心·节点业务流量汇总：某区间内该节点上/下/总字节，面板每行「今日」量与详情汇总复用。
export type NodeTrafficSummary = {
  partialArchivedHours?: number;
  accessNodeId: string;
  accessNodeName: string;
  uplinkBytes: number;
  downlinkBytes: number;
  totalBytes: number;
};

// 监控中心·节点流量趋势单个时间桶点（桶起始毫秒 + 该桶上/下/总字节）。
export type NodeTrafficTrendPoint = {
  bucketStartMs: number;
  uplinkBytes: number;
  downlinkBytes: number;
  totalBytes: number;
};

// 监控中心·节点流量趋势响应：按桶排列的趋势点 + 整区间汇总数字。
export type NodeTrafficTrend = {
  partialArchivedHours?: number;
  points: NodeTrafficTrendPoint[];
  summary: {
    uplinkBytes: number;
    downlinkBytes: number;
    totalBytes: number;
  };
};

// 监控中心·节点运行态指标（CPU 千分比 + 内存/磁盘字节用量），挂在控制面读模型上。
export type NodeRuntimeMetrics = {
  // CPU 占用千分比（0..100000），展示 /1000 得百分比。
  cpuPctMilli: number;
  memUsedBytes: number;
  memTotalBytes: number;
  diskUsedBytes: number;
  diskTotalBytes: number;
  collectedAt: string;
};

export type AccessNodeTlsCertificate = {
  domain: string;
  status: string;
  notBefore: string;
  notAfter: string;
  daysRemaining: number | null;
  errorSummary: string;
};

export type AccessNodeTlsRenewResult = {
  requestId: string;
  status: string;
  domains: string[];
};

// 节点重启请求结果：后端记一个重启请求，经心跳下发给 agent 带安全自检后重启宿主。
export type AccessNodeRebootResult = {
  requestId: string;
  status: string;
};

export type UpdateAccessNodePayload = {
  name?: string;
  public_host?: string;
  public_port?: number;
  ssh_host?: string;
  remark?: string;
  ip_direct_address?: string;
  cert_domain?: string;
  acme_email?: string;
  cf_domain?: string;
  // 多域名列表（直连 + CF）；后端 Phase 6 对齐同名字段，旧单域名字段仍兼容。
  domains?: NodeDomainInput[];
};

export type CreateAccessNodePayload = {
  name: string;
  public_host: string;
  public_port: number;
  ssh_host?: string;
  agent_token?: string;
  remark?: string;
  ip_direct_address?: string;
  cert_domain?: string;
  acme_email?: string;
  cf_domain?: string;
  // 多域名列表（直连 + CF）；后端 Phase 6 对齐同名字段，旧单域名字段仍兼容。
  domains?: NodeDomainInput[];
};

export type AccessNodeDeleteResult = {
  deletedNodeIds: string[];
  deletedNodeCount: number;
  deletedAccessLineCount: number;
  deletedLocalResourceCount: number;
  deletedLocalPoolCount: number;
};

export type AgentInstallGuidePayload = {
  accessNodeId?: string;
  installDir: string;
  composeProjectName: string;
  panelUrl: string;
  xrayApiServer?: string;
  xrayApiPort: number;
  heartbeatIntervalSeconds?: number;
  trafficIntervalSeconds?: number;
  sessionIdleSeconds?: number;
  expectedListenPorts?: number[];
  tlsCertDomains?: string[];
  disableLegacySystemdUnits?: boolean;
  forceReinstall: boolean;
};

export type AgentInstallGuideResult = {
  guideId: string;
  accessNodeId: string;
  accessNodeName: string;
  status: 'ready' | 'unknown';
  summary: string;
  installDir: string;
  composeProject: string;
  environmentText: string;
  installCommand: string;
  task: DeploymentTask | null;
  reportToken: string;
  steps: Array<{
    title: string;
    detail: string;
    command: string;
  }>;
};

export type AccessNodeOneClickInstallPayload = {
  name: string;
  publicHost: string;
  publicPort: number;
  remark?: string;
  sshHost: string;
  sshPort: number;
  sshUser: string;
  sshPassword?: string;
  sshPrivateKey?: string;
  controlPlaneUrl?: string;
  installDir: string;
  composeProject: string;
  ipDirectAddress?: string;
  tlsCertDomains?: string[];
  acmeEmail?: string;
  cfDomain?: string;
  cfApiToken?: string;
  forceReinstall: boolean;
};

export type AccessNodeOneClickInstallResult = {
  status: string;
  accessNodeId: string;
  task: DeploymentTask | null;
};

export type DeploymentTask = {
  id: string;
  kind: string;
  status: 'waiting_for_server' | 'running' | 'succeeded' | 'failed' | 'canceled' | 'unknown';
  targetType: string;
  targetId: string;
  title: string;
  summary: string;
  currentStep: string;
  progressPercent: number;
  safeMetadata: Record<string, unknown>;
  steps: Array<{
    key: string;
    title: string;
    detail: string;
    status: string;
  }>;
  result: Record<string, unknown>;
  errorSummary: string;
  createdAt: string;
  updatedAt: string;
  startedAt: string;
  completedAt: string;
};

export type DeploymentTasksResult = {
  items: DeploymentTask[];
};

export type ExitResourceSummary = {
  id: string;
  name: string;
  region: string;
  providerName: string;
  ownership: string;
  accessNodeId: string;
  accessNodeName: string;
  enabled: boolean;
  status: string;
  lastProbeAt: string;
  lastProbeStatus: string;
  createdAt: string;
};

export type CreateLocalExitLinePayload = {
  resource_name: string;
  endpoint_name?: string;
  region_code?: string;
  outbound_type: ThirdPartyExitEndpointOutboundType;
  network_mode: string;
  host: string;
  port: number;
  outbound_config?: ExitEndpointConfig;
  stream_config?: ExitEndpointConfig;
  probe_config?: ExitEndpointConfig;
  enabled?: boolean;
  // 选中的节点域名 id（免证书出口留空）；后端 Phase 6 对齐同名字段。
  node_domain_id?: string | null;
};

export type CreateLocalExitLinesResult = {
  accessNodeId: string;
  accessNodeName: string;
  createdCount: number;
  createdLines: Array<{
    exitResourceId: string;
    exitEndpointId: string;
    resourceName: string;
    endpointName: string;
    outboundType: ExitEndpointOutboundType;
    networkMode: string;
    host: string;
    port: number;
    enabled: boolean;
  }>;
};

export type ExitEndpointSummary = {
  id: string;
  exitResourceId: string;
  resourceName: string;
  name: string;
  outboundType: ExitEndpointOutboundType;
  host: string;
  hostRedacted: boolean;
  port: number;
  outboundConfig: ExitEndpointConfig;
  streamConfig: ExitEndpointConfig;
  probeConfig: ExitEndpointConfig;
  enabled: boolean;
  exitResourceEnabled: boolean;
  weight: number;
  priority: number;
  allowNewAssignments: boolean;
  status: ExitPoolMemberStatus;
  statusKnown: boolean;
  healthy: boolean;
  lastProbeAt: string;
  lastProbeStatus: string;
  createdAt: string;
};

export type LineGroupSummary = {
  id: string;
  name: string;
  parentGroupId: string;
  parentGroupName: string;
  groupLevel: 'group';
  countryCode: string;
  enabled: boolean;
  sortOrder: number;
  billingMultiplier: number;
  exitEndpointIds: string[];
  lineIds: string[];
  bindingNodeIds: string[];
  dedicatedRules: string[];
  ruleSetBindings: LineGroupRuleSetBinding[];
};

export type LineGroupRuleSetBinding = {
  ruleSetId: string;
  ruleSetName: string;
  enabled: boolean;
  position: number;
  rules: string[];
};

export type LineGroupRuleSetBindingPayload = {
  rule_set_id: string;
  enabled?: boolean;
  position?: number;
};

export type SubscriptionRuleSet = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  rules: string[];
  bindingCount: number;
  createdAt: string;
  updatedAt: string;
};

export type SubscriptionRuleSetPayload = {
  name: string;
  description?: string;
  enabled?: boolean;
  rules?: string[];
};

export type PrimaryLineGroupSummary = {
  id: string;
  name: string;
  enabled: boolean;
  sortOrder: number;
  billingMultiplier: number;
};

export type CreateLineGroupPayload = {
  name: string;
  country_code?: string;
  enabled?: boolean;
  sort_weight?: number;
  dedicated_rules?: string[];
  rule_set_bindings?: LineGroupRuleSetBindingPayload[];
};

export type UpdateLineGroupPayload = {
  name: string;
  country_code?: string;
  enabled?: boolean;
  sort_weight?: number;
  dedicated_rules?: string[];
  rule_set_bindings?: LineGroupRuleSetBindingPayload[];
};

export type ControlPlane = {
  accessNodes: AccessNodeSummary[];
  accessLines: AccessLine[];
  exitPools: ExitPool[];
  primaryLineGroups: PrimaryLineGroupSummary[];
  lineGroups: LineGroupSummary[];
};
