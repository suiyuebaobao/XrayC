// 本文件定义中转节点页面拆分组件共享的本地类型。
// 它扩展 API 返回的中转节点，用于聚合入口和运行态统计。
// 表单草稿类型集中放在这里，避免组件直接依赖页面实现。
// 文件只描述前端视图结构，不请求接口、不修改共享 API 类型。
import type {
  AccessLine,
  AccessNodeSummary,
  DeploymentTask,
  ThirdPartyExitEndpointOutboundType,
} from '@/services/api';

export type RelayNodeView = AccessNodeSummary & {
  lines: AccessLine[];
  boundLineNames: string[];
  onlineUsers: number | null;
  activeConnections: number | null;
};

// 节点域名表单的一行：直连或 CF 域名，可标主域名；CF 行带安装期透传的 API Token（不入控制台库）。
export type NodeDomainFormRow = {
  domain: string;
  kind: 'direct' | 'cf';
  isPrimary: boolean;
  // 仅 CF 行用：安装期透传给节点本机做 DNS-01，不写入控制台保存载荷。
  cfApiToken: string;
};

export type NodeForm = {
  name: string;
  publicPort: number;
  sshHost: string;
  agentToken: string;
  remark: string;
  // IP 直连地址（SSH 无关，Reality/SS 直连）；域名/CF 直连改由 domains 多行列表维护。
  ipDirectAddress: string;
  // 多行域名列表：直连（灰云，HTTP-01）+ CF（橙云，DNS-01），各自可设主域名。
  domains: NodeDomainFormRow[];
  acmeEmail: string;
};

export type LocalExitLinesForm = {
  nodeId: string;
  regionCode: string;
  lines: LocalExitLineForm[];
};

export type LocalExitLineForm = {
  resourceName: string;
  endpointName: string;
  outboundType: ThirdPartyExitEndpointOutboundType;
  // 传输（streamSettings.network，可多选，每选一个生成一条线路）：RAW(tcp)/XHTTP；HY2 固定 udp。
  transports: string[];
  // 承载（L4 单选）：RAW 传输的 network_mode，如 tcp/udp/tcp,udp（仅 SS/SOCKS 合并）/xudp（VLESS）。
  carriage: string;
  vlessSecurity: 'none' | 'reality' | 'tls';
  host: string;
  port: number;
  uuid: string;
  username: string;
  password: string;
  method: string;
  serverName: string;
  tlsCertificateFile: string;
  tlsKeyFile: string;
  // 选中的节点域名 id（免证书出口留空）；护栏按选中域名 kind 过滤协议。
  nodeDomainId: string;
  enabled: boolean;
};

export type InstallGuideForm = {
  nodeName: string;
  publicHost: string;
  installDir: string;
  composeProjectName: string;
  panelUrl: string;
  xrayApiPort: number;
  tlsCertDomains: string;
  forceReinstall: boolean;
  disableLegacySystemdUnits: boolean;
};

export type OneClickInstallForm = {
  nodeName: string;
  publicPort: number;
  remark: string;
  sshHost: string;
  sshPort: number;
  sshUser: string;
  sshAuthMethod: 'password' | 'privateKey';
  sshPassword: string;
  sshPrivateKey: string;
  installDir: string;
  composeProject: string;
  // 三行对外直连地址（各可选，至少填一个）。
  ipDirectAddress: string;
  certDomain: string;
  cfDomain: string;
  acmeEmail: string;
  // CF API Token：仅安装期透传给 agent 写本机 ini 做 DNS-01，不保存。
  cfApiToken: string;
  forceReinstall: boolean;
};

export type InstallGuideView = {
  summary: string;
  installCommand: string;
  environmentText: string;
  task: DeploymentTask | null;
  steps: Array<{
    title: string;
    detail: string;
    command: string;
  }>;
};
