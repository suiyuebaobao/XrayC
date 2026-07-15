// 本文件负责中转节点、接入线路、出口池和分组的响应标准化。
// 它把控制面返回的节点健康、探针和运行态指标转换成前端统一结构。
// 管理员后台可读取第三方出口配置，用户侧订阅仍不得暴露这些字段。
// 线路协议或出口类型扩展时，应同步更新这里的枚举兼容逻辑。

import type {
  AccessEntryExitBindingSummary,
  AccessEntrySummary,
  AccessLine,
  AccessNodeSummary,
  AccessNodeRebootResult,
  AccessNodeTlsCertificate,
  AccessNodeTlsRenewResult,
  ControlPlane,
  ExitEndpointConfig,
  ExitEndpointOutboundType,
  ExitEndpointSummary,
  ExitPool,
  ExitPoolMemberStatus,
  ExitResourceSummary,
  LineGroupSummary,
  NodeRuntimeMetrics,
  NodeTrafficSummary,
  NodeTrafficTrend,
  NodeTrafficTrendPoint,
} from '../types';
import {
  arrayValue,
  booleanValue,
  isRecord,
  numberValue,
  optionalNumberValue,
  recordValue,
  stringArrayValue,
  stringValue,
} from '../primitives';
import { normalizeHealthStatus, normalizeRuntimeMetricStatus } from './operations';
import {
  normalizeNodeDomain,
  normalizeSelectedNodeDomain,
} from './nodeDomains';

// 节点多域名相关标准化已拆到 nodeDomains.ts；这里再导出，保持既有 `../normalizers/routing` 导入路径不变。
export {
  normalizeLocalExitLineSummary,
  normalizeNodeDomain,
  normalizeSelectedNodeDomain,
} from './nodeDomains';

export {
  normalizeAccessNodeOneClickInstallResult,
  normalizeAgentInstallGuideResult,
  normalizeDeploymentTask,
  normalizeDeploymentTasksResult,
} from './deployment';

export function normalizeAccessLine(value: unknown): AccessLine {
  const data = recordValue(value);
  const status = stringValue(data.status);
  const inboundProtocol = stringValue(data.inboundProtocol ?? data.inbound_protocol ?? data.protocol);
  const networkMode = normalizeAccessLineNetworkMode(data, inboundProtocol);
  const explicitUdpEnabled = data.udpEnabled ?? data.udp_enabled;
  const runtimeOnlineUsers = optionalNumberValue(
    data.runtimeOnlineUsers
    ?? data.runtime_online_users
    ?? data.onlineUsers
    ?? data.online_users,
  );
  const runtimeActiveConnections = optionalNumberValue(
    data.runtimeActiveConnections
    ?? data.runtime_active_connections
    ?? data.activeConnections
    ?? data.active_connections,
  );
  const runtimeUniqueClientIpCount = optionalNumberValue(
    data.runtimeUniqueClientIpCount
    ?? data.runtime_unique_client_ip_count
    ?? data.uniqueClientIpCount
    ?? data.unique_client_ip_count
    ?? data.uniqueClientIps
    ?? data.unique_client_ips
    ?? data.distinctClientIpCount
    ?? data.distinct_client_ip_count
    ?? data.clientIpCount
    ?? data.client_ip_count,
  );

  return {
    id: numberValue(data.id),
    uuid: stringValue(data.uuid ?? data.id),
    name: stringValue(data.name ?? data.endpoint_name ?? data.endpointName ?? data.resource_name ?? data.resourceName),
    region: stringValue(data.region ?? data.region_name ?? data.region_code),
    accessNode: stringValue(data.accessNode ?? data.access_node ?? data.access_node_name),
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    exitEndpointId: stringValue(data.exitEndpointId ?? data.exit_endpoint_id),
    exitEndpointName: stringValue(
      data.exitEndpointName
      ?? data.exit_endpoint_name
      ?? data.endpointName
      ?? data.endpoint_name
      ?? data.exitResourceName
      ?? data.exit_resource_name,
    ),
    lineGroupId: stringValue(data.lineGroupId ?? data.line_group_id),
    lineGroupName: stringValue(data.lineGroupName ?? data.line_group_name),
    listenHost: stringValue(data.listenHost ?? data.listen_host ?? data.server),
    listenPort: numberValue(data.listenPort ?? data.listen_port ?? data.port),
    inboundProtocol,
    networkMode,
    xhttpMode: stringValue(data.xhttpMode ?? data.xhttp_mode),
    udpEnabled: explicitUdpEnabled === undefined
      ? networkMode === 'udp' || networkMode === 'xudp'
      : booleanValue(explicitUdpEnabled),
    status: status === 'disabled' || status === 'degraded' ? status : 'enabled',
    exitPoolName: stringValue(data.exitPoolName ?? data.exit_pool_name),
    exitPoolId: stringValue(data.exitPoolId ?? data.exit_pool_id),
    onlineUsers: runtimeOnlineUsers,
    runtimeOnlineUsers,
    runtimeActiveConnections,
    uniqueClientIpCount: runtimeUniqueClientIpCount,
    runtimeUniqueClientIpCount,
    uplinkRateBps: optionalNumberValue(
      data.uplinkRateBps
      ?? data.uplink_rate_bps
      ?? data.uploadRateBps
      ?? data.upload_rate_bps
      ?? data.upRateBps
      ?? data.up_rate_bps,
    ),
    downlinkRateBps: optionalNumberValue(
      data.downlinkRateBps
      ?? data.downlink_rate_bps
      ?? data.downloadRateBps
      ?? data.download_rate_bps
      ?? data.downRateBps
      ?? data.down_rate_bps,
    ),
    metricStatus: normalizeRuntimeMetricStatus(data.metricStatus ?? data.metric_status),
    metricCollectedAt: stringValue(
      data.metricCollectedAt
      ?? data.metric_collected_at
      ?? data.latestMetricAt
      ?? data.latest_metric_at
      ?? data.collected_at,
    ),
    latencyMs: optionalNumberValue(data.latencyMs ?? data.latency_ms),
    probeLatencyMs: optionalNumberValue(data.probeLatencyMs ?? data.probe_latency_ms ?? data.latencyMs ?? data.latency_ms),
    probeStatus: stringValue(data.probeStatus ?? data.probe_status ?? data.lastProbeStatus ?? data.last_probe_status),
    lastProbeAt: stringValue(data.lastProbeAt ?? data.last_probe_at ?? data.probed_at),
    probeErrorSummary: stringValue(data.probeErrorSummary ?? data.probe_error_summary ?? data.errorSummary ?? data.error_summary),
    udpPacketEncoding: stringValue(data.udpPacketEncoding ?? data.udp_packet_encoding),
  };
}

export function normalizeAccessEntry(value: unknown): AccessEntrySummary {
  const data = recordValue(value);
  return {
    id: stringValue(data.id),
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name),
    name: stringValue(data.name),
    listenHost: stringValue(data.listenHost ?? data.listen_host),
    listenPort: numberValue(data.listenPort ?? data.listen_port),
    protocol: stringValue(data.protocol) || 'vless',
    transport: stringValue(data.transport) || 'tcp',
    security: stringValue(data.security),
    serverName: stringValue(data.serverName ?? data.server_name),
    vlessQuantumEncryption: booleanValue(data.vlessQuantumEncryption ?? data.vless_quantum_encryption),
    wsPath: stringValue(data.wsPath ?? data.ws_path),
    wsHost: stringValue(data.wsHost ?? data.ws_host),
    cdnEnabled: booleanValue(data.cdnEnabled ?? data.cdn_enabled),
    cdnProvider: stringValue(data.cdnProvider ?? data.cdn_provider),
    cdnHostname: stringValue(data.cdnHostname ?? data.cdn_hostname),
    enabled: data.enabled === undefined ? true : booleanValue(data.enabled),
    sortWeight: numberValue(data.sortWeight ?? data.sort_weight) || 100,
    nodeDomainId: stringValue(data.nodeDomainId ?? data.node_domain_id),
    nodeDomain: normalizeSelectedNodeDomain(data.nodeDomain ?? data.node_domain),
    createdAt: stringValue(data.createdAt ?? data.created_at),
    updatedAt: stringValue(data.updatedAt ?? data.updated_at),
  };
}

export function normalizeAccessEntryExitBinding(value: unknown): AccessEntryExitBindingSummary {
  const data = recordValue(value);
  return {
    id: stringValue(data.id),
    accessEntryId: stringValue(data.accessEntryId ?? data.access_entry_id),
    accessEntryName: stringValue(data.accessEntryName ?? data.access_entry_name),
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name),
    exitEndpointId: stringValue(data.exitEndpointId ?? data.exit_endpoint_id),
    exitEndpointName: stringValue(data.exitEndpointName ?? data.exit_endpoint_name),
    exitPoolId: stringValue(data.exitPoolId ?? data.exit_pool_id),
    exitPoolName: stringValue(data.exitPoolName ?? data.exit_pool_name),
    name: stringValue(data.name),
    enabled: data.enabled === undefined ? true : booleanValue(data.enabled),
    sortWeight: numberValue(data.sortWeight ?? data.sort_weight) || 100,
    remark: stringValue(data.remark),
    createdAt: stringValue(data.createdAt ?? data.created_at),
    updatedAt: stringValue(data.updatedAt ?? data.updated_at),
  };
}

export function normalizeExitPool(value: unknown, fallbackMembers: ExitEndpointSummary[] = []): ExitPool {
  const data = recordValue(value);
  const status = stringValue(data.status);
  const members = arrayValue(data.members).map(normalizeExitEndpoint);

  return {
    id: numberValue(data.id),
    uuid: stringValue(data.uuid ?? data.id),
    name: stringValue(data.name),
    region: stringValue(data.region ?? data.region_name ?? data.region_code),
    status: status === 'degraded' || status === 'offline' ? status : 'healthy',
    healthyMembers: numberValue(data.healthyMembers ?? data.healthy_members),
    totalMembers: numberValue(data.totalMembers ?? data.total_members) || members.length || fallbackMembers.length,
    strategy: stringValue(data.strategy),
    activeAssignments: numberValue(data.activeAssignments ?? data.active_assignments),
    members: members.length > 0 ? members : fallbackMembers,
  };
}

export function normalizeControlPlane(data: Record<string, unknown>): ControlPlane {
  const detailedPools = new Map(
    arrayValue(data.exit_pools).map((pool) => {
      const normalized = normalizeExitPool(pool);
      return [normalized.uuid, normalized.members] as const;
    }),
  );
  const summaryPools = arrayValue(data.exitPools ?? data.exit_pools ?? data.items).map((pool) => {
    const raw = recordValue(pool);
    const uuid = stringValue(raw.uuid ?? raw.id);
    return normalizeExitPool(pool, detailedPools.get(uuid) ?? []);
  });

  const lineGroups = arrayValue(data.lineGroups ?? data.line_groups).map(normalizeLineGroup);
  const primaryLineGroups: ControlPlane['primaryLineGroups'] = [];

  return {
    accessNodes: arrayValue(data.accessNodes ?? data.access_nodes).map(normalizeAccessNode),
    accessLines: arrayValue(data.accessLines ?? data.access_lines ?? data.items).map(normalizeAccessLine),
    exitPools: summaryPools,
    primaryLineGroups,
    lineGroups,
  };
}

export function normalizeAccessNode(value: unknown): AccessNodeSummary {
  const data = recordValue(value);

  return {
    id: stringValue(data.id),
    name: stringValue(data.name),
    publicHost: stringValue(data.publicHost ?? data.public_host),
    publicPort: numberValue(data.publicPort ?? data.public_port) || 443,
    sshHost: stringValue(data.sshHost ?? data.ssh_host),
    remark: stringValue(data.remark),
    ipDirectAddress: stringValue(data.ipDirectAddress ?? data.ip_direct_address),
    certDomain: stringValue(data.certDomain ?? data.cert_domain),
    acmeEmail: stringValue(data.acmeEmail ?? data.acme_email),
    cfEnabled: booleanValue(data.cfEnabled ?? data.cf_enabled),
    cfDomain: stringValue(data.cfDomain ?? data.cf_domain),
    domains: arrayValue(data.domains ?? data.node_domains).map(normalizeNodeDomain),
    status: stringValue(data.status),
    agentVersion: stringValue(data.agentVersion ?? data.agent_version),
    configDirty: data.configDirty === true || data.config_dirty === true,
    configSynced: data.configSynced === true || data.config_synced === true,
    lastHeartbeatAt: stringValue(data.lastHeartbeatAt ?? data.last_heartbeat_at),
    lastTrafficReportAt: stringValue(data.lastTrafficReportAt ?? data.last_traffic_report_at),
    lastTrafficSuccessAt: stringValue(data.lastTrafficSuccessAt ?? data.last_traffic_success_at),
    heartbeatAgeSeconds: optionalNumberValue(data.heartbeatAgeSeconds ?? data.heartbeat_age_seconds),
    configDirtyAt: stringValue(data.configDirtyAt ?? data.config_dirty_at),
    configDirtyReason: stringValue(data.configDirtyReason ?? data.config_dirty_reason),
    healthStatus: normalizeHealthStatus(data.healthStatus ?? data.health_status),
    healthReason: stringValue(data.healthReason ?? data.health_reason),
    tlsCertificates: arrayValue(data.tlsCertificates ?? data.tls_certificates).map(normalizeAccessNodeTlsCertificate),
    tlsCertLastReportAt: stringValue(data.tlsCertLastReportAt ?? data.tls_cert_last_report_at),
    tlsRenewRequestId: stringValue(data.tlsRenewRequestId ?? data.tls_renew_request_id),
    tlsRenewRequestedAt: stringValue(data.tlsRenewRequestedAt ?? data.tls_renew_requested_at),
    tlsRenewCompletedAt: stringValue(data.tlsRenewCompletedAt ?? data.tls_renew_completed_at),
    tlsRenewStatus: stringValue(data.tlsRenewStatus ?? data.tls_renew_status),
    tlsRenewMessage: stringValue(data.tlsRenewMessage ?? data.tls_renew_message),
    // connmark 缺省可用：仅显式 false 才判不可用（缺省按 true，避免老读模型误报需重启）。
    kernelConnmarkAvailable: data.kernelConnmarkAvailable !== false && data.kernel_connmark_available !== false,
    // 内核待升级缺省 false：只有显式置真才提示需重启。
    kernelUpgradePending: booleanValue(data.kernelUpgradePending ?? data.kernel_upgrade_pending),
    rebootStatus: stringValue(data.rebootStatus ?? data.reboot_status),
    rebootMessage: stringValue(data.rebootMessage ?? data.reboot_message),
    rebootRequestedAt: stringValue(data.rebootRequestedAt ?? data.reboot_requested_at),
    rebootCompletedAt: stringValue(data.rebootCompletedAt ?? data.reboot_completed_at),
    // 节点资源指标：后端无数据时挂 null，前端原样保留以区分「未上报」与「0」。
    runtimeMetrics: normalizeNodeRuntimeMetrics(data.runtimeMetrics ?? data.runtime_metrics),
  };
}

// 把控制面节点上挂的 runtime_metrics 转成前端结构；后端无数据（null/非对象）时返回 null。
function normalizeNodeRuntimeMetrics(value: unknown): NodeRuntimeMetrics | null {
  if (value === null || value === undefined || !isRecord(value)) {
    return null;
  }
  return {
    cpuPctMilli: numberValue(value.cpuPctMilli ?? value.cpu_pct_milli),
    memUsedBytes: numberValue(value.memUsedBytes ?? value.mem_used_bytes),
    memTotalBytes: numberValue(value.memTotalBytes ?? value.mem_total_bytes),
    diskUsedBytes: numberValue(value.diskUsedBytes ?? value.disk_used_bytes),
    diskTotalBytes: numberValue(value.diskTotalBytes ?? value.disk_total_bytes),
    collectedAt: stringValue(value.collectedAt ?? value.collected_at),
  };
}

// 监控中心·节点流量汇总归一（snake→camel）：一行 = 一个节点某区间上/下/总字节。
export function normalizeNodeTrafficSummary(value: unknown): NodeTrafficSummary {
  const data = recordValue(value);
  return {
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name),
    uplinkBytes: numberValue(data.uplinkBytes ?? data.uplink_bytes),
    downlinkBytes: numberValue(data.downlinkBytes ?? data.downlink_bytes),
    totalBytes: numberValue(data.totalBytes ?? data.total_bytes),
  };
}

// 监控中心·节点流量趋势单桶点归一（snake→camel）。
function normalizeNodeTrafficTrendPoint(value: unknown): NodeTrafficTrendPoint {
  const data = recordValue(value);
  return {
    bucketStartMs: numberValue(data.bucketStartMs ?? data.bucket_start_ms),
    uplinkBytes: numberValue(data.uplinkBytes ?? data.uplink_bytes),
    downlinkBytes: numberValue(data.downlinkBytes ?? data.downlink_bytes),
    totalBytes: numberValue(data.totalBytes ?? data.total_bytes),
  };
}

// 监控中心·节点流量趋势响应归一：趋势点数组 + 区间汇总（缺 summary 时按 0 兜底，不崩）。
export function normalizeNodeTrafficTrend(value: unknown): NodeTrafficTrend {
  const data = recordValue(value);
  const summary = recordValue(data.summary);
  return {
    points: arrayValue(data.points ?? data.items).map(normalizeNodeTrafficTrendPoint),
    summary: {
      uplinkBytes: numberValue(summary.uplinkBytes ?? summary.uplink_bytes),
      downlinkBytes: numberValue(summary.downlinkBytes ?? summary.downlink_bytes),
      totalBytes: numberValue(summary.totalBytes ?? summary.total_bytes),
    },
  };
}

function normalizeAccessNodeTlsCertificate(value: unknown): AccessNodeTlsCertificate {
  const data = recordValue(value);
  return {
    domain: stringValue(data.domain),
    status: stringValue(data.status) || 'unknown',
    notBefore: stringValue(data.notBefore ?? data.not_before),
    notAfter: stringValue(data.notAfter ?? data.not_after ?? data.expiresAt ?? data.expires_at),
    daysRemaining: optionalNumberValue(data.daysRemaining ?? data.days_remaining ?? data.remainingDays ?? data.remaining_days),
    errorSummary: stringValue(data.errorSummary ?? data.error_summary),
  };
}

export function normalizeAccessNodeTlsRenewResult(value: unknown): AccessNodeTlsRenewResult {
  const data = recordValue(value);
  return {
    requestId: stringValue(data.requestId ?? data.request_id),
    status: stringValue(data.status),
    domains: stringArrayValue(data.domains),
  };
}

export function normalizeAccessNodeRebootResult(value: unknown): AccessNodeRebootResult {
  const data = recordValue(value);
  return {
    requestId: stringValue(data.requestId ?? data.request_id),
    status: stringValue(data.status),
  };
}

export function normalizeExitResource(value: unknown): ExitResourceSummary {
  const data = recordValue(value);
  return {
    id: stringValue(data.id ?? data.uuid),
    name: stringValue(data.name),
    region: stringValue(data.region ?? data.region_code),
    providerName: stringValue(data.providerName ?? data.provider_name),
    ownership: stringValue(data.ownership),
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name),
    enabled: data.enabled !== false,
    status: stringValue(data.status),
    lastProbeAt: stringValue(data.lastProbeAt ?? data.last_probe_at),
    lastProbeStatus: stringValue(data.lastProbeStatus ?? data.last_probe_status),
    createdAt: stringValue(data.createdAt ?? data.created_at),
  };
}

export function normalizeCreateLocalExitLinesResult(value: unknown) {
  const data = recordValue(value);
  return {
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name),
    createdCount: numberValue(data.createdCount ?? data.created_count),
    createdLines: arrayValue(data.createdLines ?? data.created_lines).map((line) => {
      const item = recordValue(line);
      return {
        exitResourceId: stringValue(item.exitResourceId ?? item.exit_resource_id),
        exitEndpointId: stringValue(item.exitEndpointId ?? item.exit_endpoint_id),
        resourceName: stringValue(item.resourceName ?? item.resource_name),
        endpointName: stringValue(item.endpointName ?? item.endpoint_name),
        outboundType: normalizeExitEndpointOutboundType(item.outboundType ?? item.outbound_type),
        networkMode: stringValue(item.networkMode ?? item.network_mode),
        host: stringValue(item.host),
        port: numberValue(item.port),
        enabled: item.enabled !== false,
      };
    }),
  };
}

export function normalizeExitEndpoint(value: unknown): ExitEndpointSummary {
  const data = recordValue(value);
  const outboundType = normalizeExitEndpointOutboundType(data.outboundType ?? data.outbound_type);
  const normalizedStatus = normalizeExitPoolMemberStatus(data.status ?? data.memberStatus ?? data.member_status);
  const status = normalizedStatus ?? (data.healthy === true ? 'healthy' : 'offline');
  const healthy = typeof data.healthy === 'boolean'
    ? data.healthy
    : status === 'healthy' || status === 'degraded' || status === 'draining';

  return {
    id: stringValue(data.id),
    exitResourceId: stringValue(data.exitResourceId ?? data.exit_resource_id),
    resourceName: stringValue(data.resourceName ?? data.resource_name ?? data.exit_resource_name),
    name: stringValue(data.name),
    outboundType,
    host: stringValue(data.host),
    hostRedacted: data.hostRedacted === true || data.host_redacted === true,
    port: numberValue(data.port),
    outboundConfig: endpointConfigValue(data.outboundConfig ?? data.outbound_config),
    streamConfig: endpointConfigValue(data.streamConfig ?? data.stream_config),
    probeConfig: endpointConfigValue(data.probeConfig ?? data.probe_config),
    enabled: data.enabled !== false,
    exitResourceEnabled: data.exitResourceEnabled !== false && data.exit_resource_enabled !== false,
    weight: numberValue(data.weight),
    priority: numberValue(data.priority) || 100,
    allowNewAssignments: data.allowNewAssignments !== false && data.allow_new_assignments !== false,
    status,
    statusKnown: normalizedStatus !== null,
    healthy,
    lastProbeAt: stringValue(data.lastProbeAt ?? data.last_probe_at),
    lastProbeStatus: stringValue(data.lastProbeStatus ?? data.last_probe_status),
    createdAt: stringValue(data.createdAt ?? data.created_at),
  };
}

function endpointConfigValue(value: unknown): ExitEndpointConfig {
  const data = recordValue(value);
  return { ...data };
}

export function normalizeExitEndpointOutboundType(value: unknown): ExitEndpointOutboundType {
  const outboundType = stringValue(value).trim().toLowerCase();
  if (
    outboundType === 'direct'
    || outboundType === 'socks'
    || outboundType === 'http'
    || outboundType === 'vless'
    || outboundType === 'trojan'
    || outboundType === 'shadowsocks'
    || outboundType === 'hysteria'
  ) {
    return outboundType;
  }

  if (outboundType === 'socks5') {
    return 'socks';
  }
  if (outboundType === 'ss') {
    return 'shadowsocks';
  }
  if (outboundType === 'hy2' || outboundType === 'hysteria2') {
    return 'hysteria';
  }

  return 'socks';
}

export function normalizeExitPoolMemberStatus(value: unknown): ExitPoolMemberStatus | null {
  const status = stringValue(value).trim().toLowerCase();
  if (
    status === 'healthy'
    || status === 'degraded'
    || status === 'draining'
    || status === 'offline'
    || status === 'unknown'
  ) {
    return status;
  }

  return null;
}

function normalizeLineGroup(value: unknown): LineGroupSummary {
  const data = recordValue(value);
  const enabledValue = data.enabled ?? data.isEnabled ?? data.is_enabled;
  const exitEndpointIds = arrayValue(
    data.exitEndpointIds
    ?? data.exit_endpoint_ids
    ?? data.lineIds
    ?? data.line_ids,
  ).map(stringValue).filter(Boolean);
  const bindingNodeIds = arrayValue(
    data.bindingNodeIds
    ?? data.binding_node_ids
    ?? data.accessEntryExitBindingIds
    ?? data.access_entry_exit_binding_ids
    ?? data.entryExitBindingIds
    ?? data.entry_exit_binding_ids,
  ).map(stringValue).filter(Boolean);

  return {
    id: stringValue(data.id),
    name: stringValue(data.name),
    groupLevel: 'group',
    parentGroupId: '',
    parentGroupName: '',
    countryCode: stringValue(data.countryCode ?? data.country_code),
    enabled: enabledValue === undefined ? true : booleanValue(enabledValue),
    sortOrder: numberValue(data.sortOrder ?? data.sort_order ?? data.sortWeight ?? data.sort_weight),
    billingMultiplier: numberValue(data.billingMultiplier ?? data.billing_multiplier) || 1,
    exitEndpointIds,
    lineIds: exitEndpointIds,
    bindingNodeIds,
    dedicatedRules: stringArrayValue(data.dedicatedRules ?? data.dedicated_rules ?? data.subscriptionRules ?? data.subscription_rules),
    ruleSetBindings: arrayValue(data.ruleSetBindings ?? data.rule_set_bindings).map(normalizeLineGroupRuleSetBinding),
  };
}

function normalizeLineGroupRuleSetBinding(value: unknown) {
  const data = recordValue(value);
  return {
    ruleSetId: stringValue(data.ruleSetId ?? data.rule_set_id),
    ruleSetName: stringValue(data.ruleSetName ?? data.rule_set_name),
    enabled: data.enabled === undefined ? true : booleanValue(data.enabled),
    position: numberValue(data.position),
    rules: stringArrayValue(data.rules),
  };
}

export function normalizeSubscriptionRuleSet(value: unknown) {
  const data = recordValue(value);
  return {
    id: stringValue(data.id),
    name: stringValue(data.name),
    description: stringValue(data.description),
    enabled: data.enabled === undefined ? true : booleanValue(data.enabled),
    rules: stringArrayValue(data.rules),
    bindingCount: numberValue(data.bindingCount ?? data.binding_count),
    createdAt: stringValue(data.createdAt ?? data.created_at),
    updatedAt: stringValue(data.updatedAt ?? data.updated_at),
  };
}

function normalizeAccessLineNetworkMode(data: Record<string, unknown>, inboundProtocol: string) {
  const explicit = stringValue(
    data.networkMode
    ?? data.network_mode
    ?? data.streamNetwork
    ?? data.stream_network
    ?? data.inboundTransport
    ?? data.inbound_transport
    ?? data.transport
    ?? data.network,
  ).trim().toLowerCase();
  if (explicit) {
    return explicit;
  }
  const protocol = inboundProtocol.trim().toLowerCase();
  const plusIndex = protocol.indexOf('+');
  if (plusIndex >= 0 && protocol.slice(plusIndex + 1)) {
    return protocol.slice(plusIndex + 1);
  }
  if (protocol === 'hysteria' || protocol === 'hysteria2' || protocol === 'hy2') {
    return 'udp';
  }
  return 'tcp';
}
