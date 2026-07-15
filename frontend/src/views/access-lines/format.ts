// 本文件封装中转节点页面的展示格式、选项和纯工具函数。
// 这些函数供拆分后的页面组件和 useAccessLinesPage 共同复用。
// 它不持有页面状态，也不发起 API 请求，避免和业务动作耦合。
// 状态标签、数字格式化和 JSON 解析规则保持原页面行为一致。
import type { AccessLine, ExitEndpointOutboundType, ExitPool } from '@/services/api';
import type { RelayNodeView } from '@/views/access-lines/types';

export type VlessEntrySecurityMode = 'none' | 'reality' | 'tls';

export const protocolOptions = [
  { label: 'VLESS', value: 'vless' },
  { label: 'Trojan', value: 'trojan' },
  { label: 'HY2', value: 'hysteria' },
  { label: 'Shadowsocks', value: 'shadowsocks' },
];

export const vlessEntrySecurityOptions: Array<{ label: string; value: VlessEntrySecurityMode }> = [
  { label: '普通 VLESS', value: 'none' },
  { label: 'Reality', value: 'reality' },
  { label: 'TLS', value: 'tls' },
];

export function protocolOptionsForPoolRegions(regions: string[]) {
  const hk = regions.some(isHongKongRegionCode);
  return protocolOptions.map((option) => ({
    ...option,
    disabled: hk && option.value === 'hysteria',
  }));
}

export function selectedPoolRegions(pools: ExitPool[], poolIds: string[]) {
  const selected = new Set(poolIds);
  return pools
    .filter((pool) => selected.has(pool.uuid))
    .map((pool) => pool.region);
}

export function ensureProtocolAllowedForPoolRegions(protocol: string, regions: string[]) {
  if (protocol === 'hysteria' && regions.some(isHongKongRegionCode)) {
    throw new Error('香港线路不支持 HY2，请改用 VLESS、Trojan 或 Shadowsocks。');
  }
}

export function isHongKongRegionCode(regionCode: string) {
  const normalized = regionCode.trim().toUpperCase();
  return ['HK', 'HKG', 'HONGKONG', 'HONG_KONG', '香港'].includes(normalized);
}

export function transportOptionsFor(protocol: string) {
  if (protocol === 'vless') {
    return [
      { label: 'TCP', value: 'tcp' },
      { label: 'XHTTP', value: 'xhttp' },
      { label: 'WebSocket', value: 'ws' },
      { label: 'gRPC', value: 'grpc' },
    ];
  }
  if (protocol === 'hysteria') {
    return [{ label: 'HY2', value: 'hysteria' }];
  }
  if (protocol === 'shadowsocks') {
    return [{ label: 'TCP', value: 'tcp' }];
  }

  return [
    { label: 'TCP', value: 'tcp' },
    { label: 'XHTTP', value: 'xhttp' },
    { label: 'WebSocket', value: 'ws' },
    { label: 'gRPC', value: 'grpc' },
  ];
}

export const networkModeOptions = [
  { label: 'TCP', value: 'tcp' },
  { label: 'UDP', value: 'udp' },
  { label: 'XHTTP', value: 'xhttp' },
  { label: 'XUDP', value: 'xudp' },
  { label: 'WebSocket', value: 'ws' },
  { label: 'gRPC', value: 'grpc' },
];

// 网络模式两维拆分（设计 §A.1）：
// - 传输（streamSettings.network）互斥：tcp(RAW)/ws/grpc/xhttp，选多个 = 多条线路；
// - 承载（TCP/UDP/XUDP）可合并：做成单选「合并选项」= 一条线路（不再逐 mode 拆条）。
// 这里的承载选项的 value 即直接写入后端的 network_mode（合并值如 'tcp,udp'）。
export type DiscreteOption = { label: string; value: string; disabled?: boolean };

// 传输（互斥）选项：每个选中传输生成一条线路。
// VLESS Reality 不含 WS；HY2 固定 hysteria/UDP（无传输下拉）；SS 无传输层。
export function transportOptionsForProtocolSecurity(
  protocol: string,
  security: VlessEntrySecurityMode = 'none',
): DiscreteOption[] {
  const normalized = protocol.trim().toLowerCase();
  if (['hysteria', 'hy2', 'hysteria2'].includes(normalized)) {
    return [{ label: 'HY2', value: 'hysteria' }];
  }
  if (normalized === 'shadowsocks') {
    // SS 没有传输层（只有承载），传输固定 RAW(tcp)，下拉只一项。
    return [{ label: 'RAW', value: 'tcp' }];
  }
  const raw: DiscreteOption = { label: 'RAW（TCP）', value: 'tcp' };
  const xhttp: DiscreteOption = { label: 'XHTTP', value: 'xhttp' };
  const ws: DiscreteOption = { label: 'WebSocket', value: 'ws' };
  const grpc: DiscreteOption = { label: 'gRPC', value: 'grpc' };
  if (normalized === 'vless' && security === 'reality') {
    // Reality 不开 WS。
    return [raw, xhttp, grpc];
  }
  // VLESS 非 Reality / Trojan：四种传输全放。
  return [raw, xhttp, ws, grpc];
}

// 承载（TCP/UDP/XUDP）合并选项：每项 = 一条线路的 network_mode 承载值（可合并字符串）。
// 设计 §A.2：SS=TCP/UDP/TCP+UDP；VLESS 非 reality=TCP/TCP+UDP/TCP+UDP+XUDP；
// VLESS reality=仅 TCP；trojan=TCP/TCP+UDP；hysteria=仅 UDP。
export function carriageOptionsFor(
  protocol: string,
  security: VlessEntrySecurityMode = 'none',
): DiscreteOption[] {
  const normalized = protocol.trim().toLowerCase();
  if (['hysteria', 'hy2', 'hysteria2'].includes(normalized)) {
    return [{ label: 'UDP', value: 'udp' }];
  }
  if (normalized === 'shadowsocks') {
    return [
      { label: 'TCP', value: 'tcp' },
      { label: 'UDP', value: 'udp' },
      { label: 'TCP + UDP', value: 'tcp,udp' },
    ];
  }
  if (normalized === 'trojan') {
    return [
      { label: 'TCP', value: 'tcp' },
      { label: 'TCP + UDP', value: 'tcp,udp' },
    ];
  }
  if (normalized === 'vless') {
    if (security === 'reality') {
      return [{ label: '仅 TCP', value: 'tcp' }];
    }
    return [
      { label: 'TCP', value: 'tcp' },
      { label: 'TCP + UDP', value: 'tcp,udp' },
      { label: 'TCP + UDP + XUDP', value: 'tcp,udp,xudp' },
    ];
  }
  return [{ label: 'TCP', value: 'tcp' }];
}

// 把承载合并值归一到该协议×security 允许的合并项；不在选项里则回落第一项。
export function normalizeCarriageValue(
  protocol: string,
  security: VlessEntrySecurityMode,
  carriage: string,
): string {
  const options = carriageOptionsFor(protocol, security);
  const allowed = new Set(options.map((option) => option.value));
  const value = carriage.trim().toLowerCase();
  return allowed.has(value) ? value : (options[0]?.value ?? 'tcp');
}

// 单条线路的扇出产物：
// - transport：streamSettings.network（tcp(RAW)/ws/grpc/xhttp/hysteria）；
// - networkMode：写入后端的 network_mode。RAW 传输承载合并值（tcp/udp/tcp,udp/tcp,udp,xudp）；
//   ws/grpc/xhttp 传输的 network_mode 即传输名自身（这些传输不再额外拆 TCP/UDP 承载）。
export type ExpandedLine = { transport: string; networkMode: string };

// 扇出口径（设计 §A.4）：对每个选中传输生成一条线路；
// 同一传输内的 TCP/UDP/XUDP 不再各拆一条 —— SS 选 TCP+UDP 只产 1 条 network_mode='tcp,udp'。
// 非 RAW 传输（ws/grpc/xhttp/hysteria）的 network_mode 取传输名自身，承载合并值只作用于 RAW(tcp)。
export function expandLinesByTransport(transports: string[], carriage: string): ExpandedLine[] {
  const merged = carriage.trim().toLowerCase() || 'tcp';
  const uniqueTransports = transports
    .map((transport) => transport.trim().toLowerCase())
    .filter((transport, index, values) => transport && values.indexOf(transport) === index);
  const list = uniqueTransports.length > 0 ? uniqueTransports : ['tcp'];
  return list.map((transport) => ({
    transport,
    networkMode: transport === 'tcp' ? merged : transport,
  }));
}

export function networkModeOptionsFor(protocol: string) {
  return networkModeOptionsForProtocolSecurity(protocol, 'none');
}

export function networkModeOptionsForProtocolSecurity(
  protocol: string,
  security: VlessEntrySecurityMode = 'none',
) {
  const normalized = protocol.trim().toLowerCase();
  const isHy2 = ['hysteria', 'hy2', 'hysteria2'].includes(normalized);
  const allowed = new Set(allowedNetworkModesForProtocolSecurity(normalized, security));
  return networkModeOptions.map((option) => ({
    ...option,
    disabled: !allowed.has(option.value) || (isHy2 && option.value !== 'udp'),
  }));
}

export function normalizedNetworkModesForProtocol(protocol: string, modes: string[]) {
  return normalizedNetworkModesForProtocolSecurity(protocol, 'none', modes);
}

export function normalizedNetworkModesForProtocolSecurity(
  protocol: string,
  security: VlessEntrySecurityMode,
  modes: string[],
) {
  const allowed = new Set(
    networkModeOptionsForProtocolSecurity(protocol, security)
      .filter((option) => !option.disabled)
      .map((option) => option.value),
  );
  const normalized = modes
    .map(normalizeNetworkModeValue)
    .filter((mode, index, values) => allowed.has(mode) && values.indexOf(mode) === index);

  return normalized.length > 0
    ? normalized
    : [networkModeOptionsForProtocolSecurity(protocol, security).find((option) => !option.disabled)?.value ?? 'tcp'];
}

export function defaultEntryInboundConfig(protocol: string, security: VlessEntrySecurityMode, listenHost = '') {
  const serverName = looksLikeDnsName(listenHost) ? listenHost : '';
  if (protocol.trim().toLowerCase() !== 'vless') {
    return '{}';
  }
  if (security === 'reality') {
    return JSON.stringify({
      security: 'reality',
      server_name: serverName,
      dest: serverName ? `${serverName}:443` : 'www.cloudflare.com:443',
      fingerprint: 'chrome',
    }, null, 2);
  }
  if (security === 'tls') {
    return JSON.stringify({
      security: 'tls',
      server_name: serverName,
      certificate_file: serverName ? `/etc/letsencrypt/live/${serverName}/fullchain.pem` : '',
      key_file: serverName ? `/etc/letsencrypt/live/${serverName}/privkey.pem` : '',
    }, null, 2);
  }
  return '{}';
}

// 入口 Server Name(SNI/伪装域名)默认值：
// - VLESS+Reality：用免证书伪装大站（decoy），与后端 server_names 兜底口径一致；
// - 走 TLS 的（VLESS+TLS / Trojan / HY2）：用自有证书域名（addressDomain）；
// - 普通 VLESS / Shadowsocks：无需 SNI，留空。
export function defaultEntryServerName(
  protocol: string,
  security: VlessEntrySecurityMode,
  addressDomain: string,
): string {
  const normalized = protocol.trim().toLowerCase();
  const domain = addressDomain.trim();
  if (normalized === 'vless') {
    if (security === 'reality') {
      return 'www.cloudflare.com';
    }
    return security === 'tls' ? domain : '';
  }
  if (['trojan', 'hysteria', 'hy2', 'hysteria2'].includes(normalized)) {
    return domain;
  }
  return '';
}

// 入口传输路径默认值：gRPC 用服务名 xrayc；WS/XHTTP 用 /xrayc；不走路径的传输（tcp/udp/xudp）留空。
export function defaultEntryTransportPath(modes: string[]): string {
  const usesPath = modes.some((mode) => ['ws', 'xhttp', 'grpc'].includes(mode));
  if (!usesPath) {
    return '';
  }
  return modes.length === 1 && modes[0] === 'grpc' ? 'xrayc' : '/xrayc';
}

// 入口 Host 头默认值：仅 WS/XHTTP 需要；CF 橙云用 CDN Hostname，否则回退 SNI/证书域名。
export function defaultEntryTransportHost(
  modes: string[],
  options: { cdnHostname: string; addressDomain: string },
): string {
  const usesHost = modes.some((mode) => ['ws', 'xhttp'].includes(mode));
  if (!usesHost) {
    return '';
  }
  return (options.cdnHostname.trim() || options.addressDomain.trim());
}

function allowedNetworkModesForProtocolSecurity(
  protocol: string,
  security: VlessEntrySecurityMode,
) {
  if (['hysteria', 'hy2', 'hysteria2'].includes(protocol)) {
    return ['udp'];
  }
  if (protocol === 'shadowsocks') {
    return ['tcp', 'udp'];
  }
  if (protocol === 'trojan') {
    return ['tcp', 'udp', 'xhttp', 'ws', 'grpc'];
  }
  if (protocol === 'vless') {
    return security === 'reality'
      ? ['tcp', 'xhttp', 'grpc']
      : ['tcp', 'udp', 'xhttp', 'xudp', 'ws', 'grpc'];
  }
  return ['tcp'];
}

function normalizeNetworkModeValue(mode: string) {
  const normalized = mode.trim().toLowerCase();
  if (normalized === 'hysteria' || normalized === 'hy2' || normalized === 'hysteria2') {
    return 'udp';
  }
  if (normalized === 'websocket') {
    return 'ws';
  }
  return normalized;
}

export function primaryTransportForNetworkModes(protocol: string, modes: string[]) {
  if (['hysteria', 'hy2', 'hysteria2'].includes(protocol.trim().toLowerCase())) {
    return 'hysteria';
  }
  return modes.includes('xhttp') && !modes.includes('tcp') && !modes.includes('udp') && !modes.includes('xudp')
    ? 'xhttp'
    : 'tcp';
}

export function udpEnabledForNetworkModes(modes: string[]) {
  return modes.includes('udp') || modes.includes('xudp');
}

export function udpPacketEncodingForNetworkModes(modes: string[]) {
  return modes.includes('xudp') ? 'xudp' : '';
}

export function sumNullable(values: Array<number | null | undefined>) {
  const actual = values.filter((value): value is number => typeof value === 'number' && Number.isFinite(value));
  if (actual.length === 0) {
    return null;
  }
  return actual.reduce((sum, value) => sum + value, 0);
}

export function statusType(status: AccessLine['status']) {
  return status === 'enabled' ? 'success' : status === 'degraded' ? 'warning' : 'info';
}

export function poolStatusType(pool: ExitPool) {
  return pool.status === 'healthy' ? 'success' : pool.status === 'degraded' ? 'warning' : 'danger';
}

export function nodeStatusType(node: RelayNodeView) {
  if (node.configDirty) {
    return 'warning';
  }
  return node.lines.length > 0 ? 'success' : 'info';
}

export function nodeStatusLabel(node: RelayNodeView) {
  if (node.configDirty) {
    return '待同步';
  }
  return node.lines.length > 0 ? '已有运行入口' : '未创建运行入口';
}

export function nodeHealthType(node: RelayNodeView) {
  if (node.healthStatus === 'healthy' || node.healthStatus === 'syncing') {
    return 'success';
  }
  if (node.healthStatus === 'stale' || node.healthStatus === 'degraded') {
    return 'warning';
  }
  if (node.healthStatus === 'offline' && node.lastHeartbeatAt) {
    return 'danger';
  }
  return 'info';
}

export function nodeHealthLabel(node: RelayNodeView) {
  if (!node.lastHeartbeatAt) {
    return '未上线';
  }
  if (node.healthStatus === 'healthy' || node.healthStatus === 'syncing') {
    return '节点正常';
  }
  if (node.healthStatus === 'stale') {
    return '心跳延迟';
  }
  if (node.healthStatus === 'offline') {
    return '节点离线';
  }
  if (node.healthStatus === 'degraded') {
    return '节点异常';
  }
  return '状态未知';
}

export function nodeHealthTitle(node: RelayNodeView) {
  return node.healthReason || '节点状态来自 access-agent 真实心跳和配置应用结果';
}

export function nodeTlsStatusType(node: RelayNodeView) {
  const status = node.tlsCertificates[0]?.status || 'unknown';
  if (status === 'valid') {
    return 'success';
  }
  if (status === 'expiring') {
    return 'warning';
  }
  if (status === 'expired' || status === 'missing' || status === 'invalid') {
    return 'danger';
  }
  return 'info';
}

export function nodeTlsStatusLabel(node: RelayNodeView) {
  const certificate = node.tlsCertificates[0];
  if (!certificate) {
    return 'SSL 未上报';
  }
  if (certificate.status === 'valid') {
    return 'SSL 正常';
  }
  if (certificate.status === 'expiring') {
    return 'SSL 临期';
  }
  if (certificate.status === 'expired') {
    return 'SSL 已过期';
  }
  if (certificate.status === 'missing') {
    return 'SSL 缺失';
  }
  if (certificate.status === 'invalid') {
    return 'SSL 异常';
  }
  return 'SSL 未知';
}

export function nodeTlsSummary(node: RelayNodeView) {
  const certificate = node.tlsCertificates[0];
  if (!certificate) {
    return 'SSL：未上报';
  }
  return `SSL：${certificate.domain || '未知域名'}`;
}

export function nodeTlsRemainingLabel(node: RelayNodeView) {
  const remaining = node.tlsCertificates[0]?.daysRemaining;
  if (remaining === null || remaining === undefined) {
    return node.tlsRenewMessage || '未获取到期时间';
  }
  if (remaining < 0) {
    return '已过期';
  }
  return `${remaining} 天后到期`;
}

export function normalizedTransport(protocol: string, transport: string) {
  const options = transportOptionsFor(protocol);
  if (options.some((option) => option.value === transport)) {
    return transport;
  }
  return options[0]?.value ?? 'tcp';
}

export function looksLikeDnsName(value: string) {
  return /^[a-z0-9.-]+$/i.test(value) && value.includes('.') && !/^\d{1,3}(\.\d{1,3}){3}$/.test(value);
}

export function splitTextList(value: string) {
  return value
    .split(/[\n,，\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function reportedNumber(value: number | null | undefined, fallback = '未上报') {
  return typeof value === 'number' && Number.isFinite(value) ? new Intl.NumberFormat('zh-CN').format(value) : fallback;
}

export function latencyValue(value: number | null | undefined) {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return '-';
  }
  return value <= 0 ? '< 1 ms' : `${new Intl.NumberFormat('zh-CN').format(value)} ms`;
}

export function outboundProtocolLabel(outboundType: ExitEndpointOutboundType) {
  const labels: Record<ExitEndpointOutboundType, string> = {
    direct: 'Direct',
    socks: 'SOCKS5',
    http: 'HTTP',
    vless: 'VLESS',
    trojan: 'Trojan',
    shadowsocks: 'Shadowsocks',
    hysteria: 'HY2',
  };

  return labels[outboundType] ?? outboundType;
}
