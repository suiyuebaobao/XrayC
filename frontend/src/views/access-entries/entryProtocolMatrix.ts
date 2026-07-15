// 本文件定义「入口地址类型 → 可用协议集」的前端过滤口径，与后端 validation 护栏对齐。
// 地址类型由入口当前地址推导：CF（橙云）/ 域名直连（灰云）/ IP 直连。
// 仅做纯数据映射，不请求接口、不持有页面状态；后端护栏仍为最终兜底。
// 总口径见设计方案 §3：IP→VLESS(Reality/普通)/SS；域名→全族；CF→(VLESS|Trojan)+WS/gRPC/XHTTP+TLS。
import { looksLikeDnsName } from '@/views/access-lines/format';

// 入口对外地址的三种类型。
export type EntryAddressType = 'ip' | 'domain' | 'cf';

// 入口协议（与 format.ts 的 protocolOptions value 对齐）。
export type EntryProtocol = 'vless' | 'trojan' | 'hysteria' | 'shadowsocks';

// 地址类型 → 可选协议集合（总口径）。
// - IP 直连：免证书的 VLESS（Reality 抗封 / 普通裸连两种安全模式，由 Dialog 的 securityOptions 给）与 Shadowsocks。
// - 域名直连（灰云）：四类协议全开，含 Trojan/HY2 及 VLESS/Trojan 的 WS/gRPC/XHTTP+TLS。
// - CF（橙云）：仅 VLESS 与 Trojan（走 WS/gRPC/XHTTP+TLS）。
const PROTOCOLS_BY_ADDRESS_TYPE: Record<EntryAddressType, EntryProtocol[]> = {
  ip: ['vless', 'shadowsocks'],
  domain: ['vless', 'trojan', 'hysteria', 'shadowsocks'],
  cf: ['vless', 'trojan'],
};

// CF（橙云）只代理固定的 HTTPS 端口；入口监听端口填别的（如 22443）会导致客户端到
// CF 边缘超时连不上。这里固定为 Cloudflare 支持的 HTTPS 端口集合，与后端校验同口径。
// 官方口径：443/2053/2083/2087/2096/8443。
export const CF_SUPPORTED_HTTPS_PORTS = [443, 2053, 2083, 2087, 2096, 8443] as const;

// 判断端口是否落在 CF 支持的 HTTPS 端口集合内。
export function isCfSupportedHttpsPort(port: number): boolean {
  return (CF_SUPPORTED_HTTPS_PORTS as readonly number[]).includes(port);
}

// 返回某地址类型下可用的协议集合（拷贝，避免调用方修改常量）。
export function protocolsForAddressType(type: EntryAddressType): EntryProtocol[] {
  return [...PROTOCOLS_BY_ADDRESS_TYPE[type]];
}

// 判断协议是否在该地址类型下可用。
export function isProtocolAllowedForAddressType(type: EntryAddressType, protocol: string): boolean {
  return PROTOCOLS_BY_ADDRESS_TYPE[type].includes(protocol as EntryProtocol);
}

// 由入口表单当前地址推导地址类型：
// - 选中了节点域名 → 按该域名 kind（cf→cf；direct→domain）；
// - 未选域名（免证书）→ 退回旧推导：cdnEnabled→cf；listenHost 是域名→domain；其余→ip。
export function entryAddressType(form: {
  cdnEnabled: boolean;
  listenHost: string;
  nodeDomainKind?: 'direct' | 'cf' | '';
}): EntryAddressType {
  // 选中的节点域名 kind 优先：选了就以它为准（与后端按选中域名 kind 判定护栏同口径）。
  if (form.nodeDomainKind === 'cf') {
    return 'cf';
  }
  if (form.nodeDomainKind === 'direct') {
    return 'domain';
  }
  // 未选域名（免证书）：沿用旧地址推导，cdn/域名形态/IP。
  if (form.cdnEnabled) {
    return 'cf';
  }
  return looksLikeDnsName(form.listenHost.trim()) ? 'domain' : 'ip';
}

// 由选中的节点域名 kind 直接映射地址类型；未选（空/undefined）= 免证书 = ip。
export function addressTypeForNodeDomainKind(kind: 'direct' | 'cf' | '' | undefined): EntryAddressType {
  if (kind === 'cf') {
    return 'cf';
  }
  if (kind === 'direct') {
    return 'domain';
  }
  return 'ip';
}
