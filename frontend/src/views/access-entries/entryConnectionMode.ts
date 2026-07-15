// 本文件提供入口「连接方式」的纯数据派生：可用模式集、下拉项、按 kind 取首个域名、反推初始模式。
// 连接方式即 EntryAddressType（ip/domain/cf），由它显式驱动地址类型+选域名+协议+CDN，口径与本机出口一致。
// 仅做纯函数派生，不持有页面状态、不请求接口；CDN 开关与协议归一仍在 Dialog 内联动。
// 设计见 §2.2.2：ip=免证书 VLESS(Reality/普通)/SS；domain=域名直连灰云；cf=CF 直连橙云（开 CDN）。
import type { NodeDomain } from '@/services/api';
import { addressTypeForNodeDomainKind, type EntryAddressType } from '@/views/access-entries/entryProtocolMatrix';

// 连接方式下拉文案：免证书 / 灰云 / 橙云。
const CONNECTION_MODE_LABELS: Record<EntryAddressType, string> = {
  ip: 'IP 直连（免证书）',
  domain: '域名直连（灰云）',
  cf: 'CF 直连（橙云）',
};

// 协议下拉下方的地址类型提示文案（与可用协议集同口径）。
export const ADDRESS_TYPE_HINTS: Record<EntryAddressType, string> = {
  ip: 'IP 直连（免证书）：VLESS 选 Reality（抗封伪装，限 TCP/XHTTP/gRPC）或普通（裸连，UDP/XUDP/WS 全开但无伪装、易被识别）；另支持 Shadowsocks。',
  domain: '域名直连（灰云）：VLESS/Trojan 直连 TLS、HY2、Shadowsocks。',
  cf: 'CF 橙云地址：仅 VLESS / Trojan，传输 WS/gRPC/XHTTP + TLS。',
};

// 当前节点支持的连接方式：恒含 ip；有 direct 域名才加 domain，有 cf 域名才加 cf。
export function availableConnectionModes(domains: NodeDomain[]): EntryAddressType[] {
  const modes: EntryAddressType[] = ['ip'];
  if (domains.some((domain) => domain.kind === 'direct')) {
    modes.push('domain');
  }
  if (domains.some((domain) => domain.kind === 'cf')) {
    modes.push('cf');
  }
  return modes;
}

// 连接方式下拉项（只列当前节点可用项），label 取上面的文案。
export function connectionModeOptions(domains: NodeDomain[]) {
  return availableConnectionModes(domains).map((mode) => ({ label: CONNECTION_MODE_LABELS[mode], value: mode }));
}

// 取当前节点指定 kind 的首个域名 id（优先主域名），没有则空串。
export function firstNodeDomainId(domains: NodeDomain[], kind: 'direct' | 'cf'): string {
  const matched = domains.filter((domain) => domain.kind === kind);
  const primary = matched.find((domain) => domain.isPrimary);
  return (primary ?? matched[0])?.id ?? '';
}

// 反推打开弹窗时的初始连接方式：cdnEnabled→cf；否则按选中域名 kind；结果不在可用集里回退 ip。
export function deriveInitialConnectionMode(
  domains: NodeDomain[],
  cdnEnabled: boolean,
  selectedDomainKind: 'direct' | 'cf' | undefined,
): EntryAddressType {
  const mode: EntryAddressType = cdnEnabled ? 'cf' : addressTypeForNodeDomainKind(selectedDomainKind);
  return availableConnectionModes(domains).includes(mode) ? mode : 'ip';
}
