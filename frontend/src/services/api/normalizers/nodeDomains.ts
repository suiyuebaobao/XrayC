// 本文件负责节点多域名相关响应的标准化（从 routing.ts 拆出以满足单文件长度门禁）。
// 覆盖：节点视图域名行、入口/出口选中域名引用、已建本机出口线路。
// 管理员后台可读取这些配置，用户侧订阅和公开接口仍不得暴露出口凭据。
// kind 仅 direct/cf 合法，其它一律按 direct 兜底，与后端读模型同口径。
import type {
  LocalExitLineSummary,
  NodeDomain,
  NodeDomainKind,
  SelectedNodeDomain,
} from '../types';
import { booleanValue, numberValue, recordValue, stringValue } from '../primitives';
import { normalizeExitEndpointOutboundType } from './routing';

// 标准化节点 kind：仅 direct/cf 合法，其它一律按 direct 兜底。
function normalizeNodeDomainKind(value: unknown): NodeDomainKind {
  return stringValue(value) === 'cf' ? 'cf' : 'direct';
}

// 标准化节点视图里的单个域名行（node.domains[] 的一项）。
export function normalizeNodeDomain(value: unknown): NodeDomain {
  const data = recordValue(value);
  return {
    id: stringValue(data.id),
    domain: stringValue(data.domain),
    kind: normalizeNodeDomainKind(data.kind),
    isPrimary: booleanValue(data.isPrimary ?? data.is_primary),
    certStatus: stringValue(data.certStatus ?? data.cert_status) || 'unknown',
  };
}

// 标准化入口/出口视图里选中的节点域名引用（node_domain：id+domain+kind）；缺则 null。
export function normalizeSelectedNodeDomain(value: unknown): SelectedNodeDomain | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  const data = recordValue(value);
  const id = stringValue(data.id);
  const domain = stringValue(data.domain);
  if (!id && !domain) {
    return null;
  }
  return { id, domain, kind: normalizeNodeDomainKind(data.kind) };
}

// 标准化已建本机出口线路（GET .../local-exit-lines 的一项）。
export function normalizeLocalExitLineSummary(value: unknown): LocalExitLineSummary {
  const data = recordValue(value);
  return {
    exitEndpointId: stringValue(data.exitEndpointId ?? data.exit_endpoint_id ?? data.id),
    exitResourceId: stringValue(data.exitResourceId ?? data.exit_resource_id),
    resourceName: stringValue(data.resourceName ?? data.resource_name),
    endpointName: stringValue(data.endpointName ?? data.endpoint_name ?? data.name),
    outboundType: normalizeExitEndpointOutboundType(data.outboundType ?? data.outbound_type),
    networkMode: stringValue(data.networkMode ?? data.network_mode) || 'tcp',
    host: stringValue(data.host),
    port: numberValue(data.port),
    enabled: data.enabled === undefined ? true : booleanValue(data.enabled),
    nodeDomainId: stringValue(data.nodeDomainId ?? data.node_domain_id),
    nodeDomain: normalizeSelectedNodeDomain(data.nodeDomain ?? data.node_domain),
  };
}
