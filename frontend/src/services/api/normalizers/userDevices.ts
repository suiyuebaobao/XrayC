// 本文件负责用户设备/IP 观察列表的响应标准化。
// 设备管理只展示订阅拉取和节点使用 IP，不表达设备绑定。
// 归一化仅做字段名兼容，不在前端重新执行去重逻辑。
// 后端返回的来源和计数是最终可信口径。

import type { AdminUserDeviceIp, AdminUserDeviceIpPage, AdminUserDeviceQuery } from '../types';
import { collectionValue, numberValue, recordValue, stringValue } from '../primitives';

export function normalizeAdminUserDeviceIpPage(
  value: unknown,
  query: AdminUserDeviceQuery = {},
): AdminUserDeviceIpPage {
  const data = recordValue(value);
  const items = collectionValue(data, ['items', 'devices', 'data']).map(normalizeAdminUserDeviceIp);
  return {
    items,
    total: numberValue(data.total),
    page: numberValue(data.page) || query.page || 1,
    pageSize: numberValue(data.pageSize ?? data.page_size) || query.pageSize || 50,
  };
}

function normalizeAdminUserDeviceIp(value: unknown): AdminUserDeviceIp {
  const data = recordValue(value);
  const accessLine = recordValue(data.lastAccessLine ?? data.last_access_line);
  const accessNode = recordValue(data.lastAccessNode ?? data.last_access_node);
  const source = normalizeDeviceSource(data.source);

  return {
    clientIp: stringValue(data.clientIp ?? data.client_ip),
    clientIpHash: stringValue(data.clientIpHash ?? data.client_ip_hash),
    source,
    firstSeenAt: stringValue(data.firstSeenAt ?? data.first_seen_at),
    lastSeenAt: stringValue(data.lastSeenAt ?? data.last_seen_at),
    subscriptionPullCount: numberValue(data.subscriptionPullCount ?? data.subscription_pull_count),
    nodeUseCount: numberValue(data.nodeUseCount ?? data.node_use_count),
    lastAccessLine: {
      id: stringValue(accessLine.id),
      name: stringValue(accessLine.name),
      protocol: stringValue(accessLine.protocol),
      transport: stringValue(accessLine.transport),
    },
    lastAccessNode: {
      id: stringValue(accessNode.id),
      name: stringValue(accessNode.name),
      publicHost: stringValue(accessNode.publicHost ?? accessNode.public_host),
    },
    lastSessionStatus: stringValue(data.lastSessionStatus ?? data.last_session_status) || 'unknown',
    lastActiveConnectionCount: numberValue(
      data.lastActiveConnectionCount ?? data.last_active_connection_count,
    ),
  };
}

function normalizeDeviceSource(value: unknown): AdminUserDeviceIp['source'] {
  const source = stringValue(value);
  return source === 'subscription_pull' || source === 'node_use' || source === 'both'
    ? source
    : 'unknown';
}
