// 本文件封装入口表单「地址自动带出」的纯逻辑：从所选中转节点带出直连地址与 CF 域名。
// 直连入口默认取节点直连地址（优先证书域名，无则客户连接地址），CF 入口默认取节点橙云域名。
// 自动带出但仍可编辑覆盖：管理员手改过的字段标记为脏，切换节点/协议时不覆盖脏值。
// 文件只做数据变换与脏标记管理，不请求接口、不读写页面其它状态。
import { reactive } from 'vue';
import type { AccessNodeSummary } from '@/services/api';
import {
  defaultEntryServerName,
  defaultEntryTransportHost,
  defaultEntryTransportPath,
  looksLikeDnsName,
  type VlessEntrySecurityMode,
} from '@/views/access-lines/format';

// 入口表单里与地址自动带出相关的最小字段子集。
export type AddressAutofillFormState = {
  accessNodeId: string;
  listenHost: string;
  cdnEnabled: boolean;
  cdnHostname: string;
  serverName: string;
  wsPath: string;
  wsHost: string;
};

// 节点的直连地址：优先灰云证书域名，无则回退客户连接地址。
export function nodeDirectAddress(node: AccessNodeSummary | null | undefined): string {
  if (!node) {
    return '';
  }
  return (node.certDomain || node.publicHost || '').trim();
}

// 节点的 CF（橙云）域名。
export function nodeCfDomain(node: AccessNodeSummary | null | undefined): string {
  return (node?.cfDomain || '').trim();
}

// 按连接方式取入口监听地址默认值：
// - ip（IP 直连）：取节点 IP 直连地址（ip_direct_address），空则回退客户连接地址 publicHost；绝不带出证书域名。
// - cf（CF 直连）：取节点 CF（橙云）域名。
// - domain（域名直连）：优先当前选中域名，其次证书域名，再回退 publicHost。
export function nodeListenHostForMode(
  node: AccessNodeSummary | null | undefined,
  mode: 'ip' | 'domain' | 'cf',
  selectedDomain = '',
): string {
  if (!node) {
    return '';
  }
  if (mode === 'ip') {
    return (node.ipDirectAddress || node.publicHost || '').trim();
  }
  if (mode === 'cf') {
    return (node.cfDomain || '').trim();
  }
  return (selectedDomain || node.certDomain || node.publicHost || '').trim();
}

// 脏标记：记录管理员是否手动改过对应字段，改过则后续不再自动覆盖。
export type AddressDirtyFlags = {
  listenHost: boolean;
  cdnHostname: boolean;
  serverName: boolean;
  wsPath: boolean;
  wsHost: boolean;
};

export function createAddressDirtyFlags(): AddressDirtyFlags {
  return reactive<AddressDirtyFlags>({
    listenHost: false,
    cdnHostname: false,
    serverName: false,
    wsPath: false,
    wsHost: false,
  });
}

// 入口表单里与协议字段默认值相关的最小子集（Server Name / 路径 / Host 由协议×安全×网络模式派生）。
export type ProtocolFieldDefaultsFormState = {
  protocol: string;
  security: string;
  transports: string[];
  listenHost: string;
  cdnHostname: string;
  serverName: string;
  wsPath: string;
  wsHost: string;
};

// 把入口的 security 归一为 VLESS 安全模式标记（仅用于派生 SNI 默认值；非 VLESS 走各自协议分支不看它）。
function vlessSecurityMode(security: string): VlessEntrySecurityMode {
  if (security === 'reality') {
    return 'reality';
  }
  return security === 'tls' ? 'tls' : 'none';
}

// 按当前协议×安全×网络模式，把未手改的 Server Name / 路径 / Host 刷新为默认值。
// addressDomain：监听地址是域名就用它，否则回退节点直连证书域名（IP 直连时 SNI 默认与域名无关，由 Reality decoy 兜底）。
export function applyProtocolFieldDefaults(
  form: ProtocolFieldDefaultsFormState,
  node: AccessNodeSummary | null | undefined,
  dirty: AddressDirtyFlags,
): void {
  const listenHost = form.listenHost.trim();
  const addressDomain = looksLikeDnsName(listenHost) ? listenHost : nodeDirectAddress(node);
  const modes = form.transports.filter((mode) => mode);
  if (!dirty.serverName) {
    form.serverName = defaultEntryServerName(form.protocol, vlessSecurityMode(form.security), addressDomain);
  }
  if (!dirty.wsPath) {
    form.wsPath = defaultEntryTransportPath(modes);
  }
  if (!dirty.wsHost) {
    form.wsHost = defaultEntryTransportHost(modes, { cdnHostname: form.cdnHostname, addressDomain });
  }
}

// 打开弹窗时重置脏标记：创建态全部视为未手改；编辑态若已有自定义值则视为已手改，避免覆盖既有入口地址。
export function resetAddressDirtyFlags(
  dirty: AddressDirtyFlags,
  form: AddressAutofillFormState,
  node: AccessNodeSummary | null | undefined,
  mode: 'create' | 'edit',
): void {
  if (mode === 'create') {
    dirty.listenHost = false;
    dirty.cdnHostname = false;
    dirty.serverName = false;
    dirty.wsPath = false;
    dirty.wsHost = false;
    return;
  }
  // 编辑态：值若与节点默认一致则仍可随节点变化，否则视为管理员自定义，尊重不覆盖。
  dirty.listenHost = form.listenHost.trim() !== '' && form.listenHost.trim() !== nodeDirectAddress(node);
  dirty.cdnHostname = form.cdnHostname.trim() !== '' && form.cdnHostname.trim() !== nodeCfDomain(node);
  // 编辑态：Server Name / 路径 / Host 只要入口已有值就视为管理员自定义，保留不覆盖。
  dirty.serverName = form.serverName.trim() !== '';
  dirty.wsPath = form.wsPath.trim() !== '';
  dirty.wsHost = form.wsHost.trim() !== '';
}

// 按当前所选节点 + 连接方式把未手改的地址字段刷新为节点默认值。
// 监听地址（listenHost）按连接方式分流：IP 直连取节点 IP、域名直连取选中域名、CF 取 CF 域名。
// CF 域名（cdnHostname）只要未手改就刷新。
export function applyNodeAddressDefaults(
  form: AddressAutofillFormState,
  node: AccessNodeSummary | null | undefined,
  dirty: AddressDirtyFlags,
  mode: 'ip' | 'domain' | 'cf' = 'ip',
  selectedDomain = '',
): void {
  if (!node) {
    return;
  }
  if (!dirty.listenHost) {
    form.listenHost = nodeListenHostForMode(node, mode, selectedDomain);
  }
  if (!dirty.cdnHostname && form.cdnEnabled) {
    form.cdnHostname = nodeCfDomain(node);
  }
}
