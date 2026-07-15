// 本文件集中入口管理保存时的纯函数：网络模式归一与按模式构造 payload。
// 从 AccessEntriesPage 拆出，承载 CDN 字段到后端 payload 的映射逻辑。
// 启用 CDN 时透传 provider/hostname，停用时清空，避免脏值入库。
// 文件只做数据变换，不请求接口、不读写页面状态。
import type { AccessEntryPayload } from '@/services/api';
import { expandLinesByTransport } from '@/views/access-lines/format';

export type EntryPayloadFormState = {
  accessNodeId: string;
  name: string;
  listenHost: string;
  listenPort: number;
  protocol: string;
  transport: string;
  transports: string[];
  // 承载（L4 单选合并值）：tcp / tcp,udp / tcp,udp,xudp；提交时降维成入口后端接受的单值模式。
  carriage: string;
  security: string;
  serverName: string;
  // VLESS 量子加密（后量子）开关：仅非 Reality VLESS 提交 true。
  vlessQuantumEncryption: boolean;
  wsPath: string;
  wsHost: string;
  cdnEnabled: boolean;
  cdnProvider: string;
  cdnHostname: string;
  // 选中的节点域名 id（免证书入口留空）。
  nodeDomainId: string;
  enabled: boolean;
  sortWeight: number;
};

// 把扇出后的 networkMode 映射成入口后端 access_entry_network_mode 接受的单值模式：
// 入口不接受合并字符串 tcp,udp —— 一端口同时收 TCP/UDP 用单值 udp（udp_enabled）表达；
// tcp,udp,xudp 用 xudp（packetEncoding）表达；HY2 传输映射成 udp；其余传输（tcp/ws/grpc/xhttp）原样。
export function entryNetworkModeFromExpanded(networkMode: string): string {
  const value = networkMode.trim().toLowerCase();
  if (value === 'hysteria') {
    return 'udp';
  }
  if (value.includes('xudp')) {
    return 'xudp';
  }
  if (value.includes('udp')) {
    return 'udp';
  }
  return value;
}

// 由「传输多选 + 承载单选」扇出入口网络模式：每个传输生成一条；承载在同一 RAW(tcp) 传输内合并，
// 合并值再降维成入口接受的单值模式。SS 选 TCP+UDP（单传输 RAW + 承载 tcp,udp）→ 仅 1 条 udp，不再 2 条。
export function selectedEntryNetworkModes(form: EntryPayloadFormState): string[] {
  const transports = form.transports.length > 0 ? form.transports : [form.transport || 'tcp'];
  const carriage = form.carriage || 'tcp';
  const modes = expandLinesByTransport(transports, carriage).map((line) =>
    entryNetworkModeFromExpanded(line.networkMode),
  );
  return modes.filter((mode, index, values) => mode && values.indexOf(mode) === index);
}

// 编辑既有入口时把已存的单值网络模式反推回「传输 + 承载」，供表单回填；
// 打开弹窗后 normalize 会再据当前协议/安全收敛，这里只给一个合理初值。
export function entryTransportsCarriageFromMode(
  protocol: string,
  mode: string,
): { transports: string[]; carriage: string } {
  const normalizedProtocol = protocol.trim().toLowerCase();
  const normalizedMode = mode.trim().toLowerCase();
  if (normalizedProtocol === 'hysteria') {
    return { transports: ['hysteria'], carriage: 'udp' };
  }
  if (normalizedMode === 'ws' || normalizedMode === 'grpc' || normalizedMode === 'xhttp') {
    return { transports: [normalizedMode], carriage: 'tcp' };
  }
  if (normalizedMode === 'xudp') {
    return { transports: ['tcp'], carriage: 'tcp,udp,xudp' };
  }
  if (normalizedMode === 'udp') {
    return { transports: ['tcp'], carriage: 'tcp,udp' };
  }
  return { transports: ['tcp'], carriage: 'tcp' };
}

// 按单个网络模式构造一条入口 payload；多模式时端口与排序按 index 递增。
export function entryPayloadForMode(
  form: EntryPayloadFormState,
  mode: string,
  index: number,
  total: number,
): AccessEntryPayload {
  const usesPath = ['ws', 'xhttp', 'grpc'].includes(mode);
  return {
    access_node_id: form.accessNodeId,
    name: total > 1 ? `${form.name.trim()} ${networkModeLabel(mode)}` : form.name.trim(),
    listen_host: form.listenHost.trim(),
    listen_port: Number(form.listenPort || 443) + index,
    protocol: form.protocol,
    transport: mode,
    security: form.security,
    server_name: form.serverName.trim(),
    // 量子加密只对非 Reality VLESS 生效；其余协议/Reality 一律提交 false，避免隐藏开关带脏值。
    vless_quantum_encryption:
      form.protocol === 'vless' && form.security !== 'reality' ? form.vlessQuantumEncryption : false,
    ws_path: usesPath ? form.wsPath.trim() : '',
    ws_host: usesPath ? form.wsHost.trim() : '',
    cdn_enabled: form.cdnEnabled,
    cdn_provider: form.cdnEnabled ? form.cdnProvider : '',
    cdn_hostname: form.cdnEnabled ? form.cdnHostname.trim() : '',
    // 选中的节点域名 id（免证书留空走 null）；后端 Phase 6 对齐同名字段。
    node_domain_id: form.nodeDomainId.trim() || null,
    enabled: form.enabled,
    sort_weight: Number(form.sortWeight || 100) + index,
  };
}

export function networkModeLabel(mode: string): string {
  const labels: Record<string, string> = {
    tcp: 'TCP',
    udp: 'UDP',
    xhttp: 'XHTTP',
    xudp: 'XUDP',
    ws: 'WS',
    grpc: 'gRPC',
  };
  return labels[mode] ?? mode.toUpperCase();
}
