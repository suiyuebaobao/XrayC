// 本文件计算某中转节点「已用端口」集合，供入口 / 本机出口表单的智能默认端口与即时校验避开。
// 已用端口 = 该节点所有启用入口的 listen_port ∪ 该节点自建（self_hosted）本机出口 endpoint 的 port。
// 全部用前端已加载的读模型数据（入口列表 / 出口 endpoint / 出口资源）计算，不新增后端接口。
// 仅做纯数据聚合，不请求接口、不持有页面状态。
import type {
  AccessEntrySummary,
  ExitEndpointSummary,
  ExitResourceSummary,
} from '@/services/api';

// 计算节点已用端口集合。
// - entries：启用且属于该节点的入口 listen_port（与后端端口唯一性护栏同口径：access_node_id + enabled + listen_port）。
// - exitEndpoints × exitResources：endpoint → resource → accessNodeId，取该节点自建（self_hosted）出口的 port。
// - excludeEntryId：编辑既有入口时排除自身，避免把自己的当前端口当成「已占用」误报。
export function collectNodeUsedPorts(
  nodeId: string,
  entries: AccessEntrySummary[],
  exitEndpoints: ExitEndpointSummary[],
  exitResources: ExitResourceSummary[],
  excludeEntryId?: string,
): Set<number> {
  const used = new Set<number>();
  if (!nodeId) {
    return used;
  }
  for (const entry of entries) {
    if (
      entry.accessNodeId === nodeId
      && entry.enabled
      && entry.id !== excludeEntryId
      && entry.listenPort > 0
    ) {
      used.add(entry.listenPort);
    }
  }
  // 该节点自建本机出口对应的出口资源 id 集合（第三方出口在远端主机、不占本节点端口，排除）。
  const selfHostedResourceIds = new Set(
    exitResources
      .filter((resource) => resource.accessNodeId === nodeId && resource.ownership === 'self_hosted')
      .map((resource) => resource.id),
  );
  for (const endpoint of exitEndpoints) {
    if (selfHostedResourceIds.has(endpoint.exitResourceId) && endpoint.port > 0) {
      used.add(endpoint.port);
    }
  }
  return used;
}
