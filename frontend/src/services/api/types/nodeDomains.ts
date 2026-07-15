// 本文件定义节点多域名相关的前端 API 类型（从 routing.ts 拆出以满足单文件长度门禁）。
// 覆盖：节点视图域名清单、节点写入域名行、入口/出口选中域名引用、已建本机出口与就地编辑。
// 文件只描述数据形状，不含请求、格式化或 UI 逻辑；订阅/公开接口仍不得暴露出口凭据。
// 域名 kind：direct=域名直连（灰云，HTTP-01）；cf=CF 直连（橙云，DNS-01 签自己证书）。
import type { ExitEndpointConfig, ExitEndpointOutboundType } from './routing';

// 节点域名 kind：direct=域名直连（灰云，HTTP-01 签证书）；cf=CF 直连（橙云，DNS-01 签自己证书）。
export type NodeDomainKind = 'direct' | 'cf';

// 节点视图里的单个域名：后端读模型 node.domains[] 的一项。
export type NodeDomain = {
  id: string;
  domain: string;
  kind: NodeDomainKind;
  isPrimary: boolean;
  certStatus: string;
};

// 节点写入（创建/更新）时提交的域名行；旧 cert_domain/cf_domain 后端仍兼容。
export type NodeDomainInput = {
  domain: string;
  kind: NodeDomainKind;
  is_primary?: boolean;
};

// 入口/出口视图里选中的节点域名引用（后端读模型 node_domain：id+domain+kind）。
export type SelectedNodeDomain = {
  id: string;
  domain: string;
  kind: NodeDomainKind;
};

// 已建本机出口线路（GET .../local-exit-lines 返回该节点 self_hosted 出口）。
export type LocalExitLineSummary = {
  exitEndpointId: string;
  exitResourceId: string;
  resourceName: string;
  endpointName: string;
  outboundType: ExitEndpointOutboundType;
  networkMode: string;
  host: string;
  port: number;
  enabled: boolean;
  nodeDomainId: string;
  nodeDomain: SelectedNodeDomain | null;
};

// 本机出口就地编辑提交字段（PUT .../local-exit-lines/:endpointId）。
export type UpdateLocalExitLinePayload = {
  endpoint_name?: string;
  host?: string;
  port?: number;
  outbound_config?: ExitEndpointConfig;
  stream_config?: ExitEndpointConfig;
  enabled?: boolean;
  node_domain_id?: string | null;
};
