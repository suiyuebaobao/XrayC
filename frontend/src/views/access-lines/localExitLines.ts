// 本文件提供本机出口线路批量创建的纯工具。
// 每条本机出口线路都是出口管理里的 self_hosted endpoint。
// 本机出口服务面向管理员批量创建自建线路，前端用普通字段组装后端配置。
// 网络模式写入 stream_config，后端会再次校验并补齐。
import type { ThirdPartyExitEndpointOutboundType } from '@/services/api';
import type { LocalExitLineForm } from '@/views/access-lines/types';
import { vlessSecurityOptions } from '@/views/exit-pools/exitPoolsPageHelpers';
import type { VlessSecurityMode } from '@/views/exit-pools/exitPoolsPageHelpers';

export const localExitProtocolOptions: Array<{ label: string; value: ThirdPartyExitEndpointOutboundType }> = [
  { label: 'VLESS', value: 'vless' },
  { label: 'HY2', value: 'hysteria' },
  { label: 'Shadowsocks', value: 'shadowsocks' },
  { label: 'Trojan', value: 'trojan' },
  // SOCKS5/HTTP:最简单的本机出口,明文转发、免证书(类型层与编译侧已支持)。
  { label: 'SOCKS5', value: 'socks' },
  { label: 'HTTP', value: 'http' },
];

// 本机出口里「要证书」的协议:Trojan/HY2 直接要证书,VLESS 仅在 security=tls 时要证书。
// 这些协议要靠所属节点的域名直连地址(cert_domain)签/锚定证书,纯 IP/无证书节点不能用。
const CERT_REQUIRED_PROTOCOLS: ReadonlySet<ThirdPartyExitEndpointOutboundType> = new Set([
  'hysteria',
  'trojan',
]);

// 出口侧协议-地址护栏(前端过滤):节点没配域名直连地址时,去掉要证书的协议(Trojan/HY2)。
// 配了 cert_domain 则全放开。和后端 validate_local_exit_cert_domain 同口径。
export function localExitProtocolOptionsFor(certDomainAvailable: boolean) {
  if (certDomainAvailable) {
    return localExitProtocolOptions;
  }
  return localExitProtocolOptions.filter((option) => !CERT_REQUIRED_PROTOCOLS.has(option.value));
}

// VLESS 安全模式过滤:节点无 cert_domain 时去掉 tls(留 reality/none),配了则全放开。
export function localExitVlessSecurityOptionsFor(certDomainAvailable: boolean) {
  if (certDomainAvailable) {
    return vlessSecurityOptions;
  }
  return vlessSecurityOptions.filter((option) => option.value !== 'tls');
}

// 选中的节点域名 kind:direct=域名直连(有证书);cf=CF 直连;''/undefined=免证书(留空)。
export type LocalExitDomainKind = 'direct' | 'cf' | '';

// 本机出口每条线路的「连接方式」:由它驱动域名选择 + 协议选项,比"选域名隐式定协议"直观。
// ip=IP 直连(免证书,不选域名);direct=域名直连(灰云,选直连域名)。
// 本机出口是同机自家出口,走 CF(橙云)无意义,故不提供 CF 直连。
export type LocalExitConnectionMode = 'ip' | 'direct';

// 线路表单(运行期)在 LocalExitLineForm 基础上挂一个连接方式;字段默认值由组件初始化为 'ip'。
// 之所以在此扩展而不改 types.ts:连接方式是 UI 驱动层,提交给后端仍只看 nodeDomainId(留空=免证书)。
export type LocalExitLineFormWithMode = LocalExitLineForm & {
  connectionMode: LocalExitConnectionMode;
};

// 连接方式下拉标签(灰云沿用节点域名页同口径措辞)。
export const localExitConnectionModeLabels: Record<LocalExitConnectionMode, string> = {
  ip: 'IP 直连（免证书）',
  direct: '域名直连（灰云）',
};

// 连接方式 → 选中域名 kind:ip 不选域名(留空=免证书),direct 对应同名 kind。
// 协议/安全/网络过滤照旧按 kind 走,只是 kind 来源从"找选中域名"换成"连接方式"。
// 本机出口无 cf 连接方式,故不会产生 'cf' kind。
export function localExitConnectionModeToDomainKind(mode: LocalExitConnectionMode): LocalExitDomainKind {
  if (mode === 'direct') {
    return 'direct';
  }
  return '';
}

// CF 直连下只放 ws/grpc/xhttp + tls 的协议(VLESS/Trojan);direct 全放;留空只放免证书。
const CF_PROTOCOLS: ReadonlySet<ThirdPartyExitEndpointOutboundType> = new Set(['vless', 'trojan']);

// 按「选中节点域名 kind」过滤本机出口协议(与后端按选中域名 kind 判定护栏同口径):
// - direct(有证书域名):全放开(含 Trojan/HY2/VLESS-TLS)。
// - cf(CF 域名):只放 VLESS/Trojan(走 ws/grpc/xhttp + tls)。
// - 留空(免证书):去掉要证书的协议(Trojan/HY2),只留 Reality/SS/socks/http。
export function localExitProtocolOptionsForDomainKind(kind: LocalExitDomainKind) {
  if (kind === 'cf') {
    return localExitProtocolOptions.filter((option) => CF_PROTOCOLS.has(option.value));
  }
  if (kind === 'direct') {
    // 域名直连本机出口 = Trojan/HY2/VLESS-TLS(CLAUDE §4);SS/SOCKS5/HTTP 属免证书、走 IP 直连档,不在此。
    return localExitProtocolOptions.filter(
      (option) => option.value === 'vless' || CERT_REQUIRED_PROTOCOLS.has(option.value),
    );
  }
  return localExitProtocolOptions.filter((option) => !CERT_REQUIRED_PROTOCOLS.has(option.value));
}

// 按「选中节点域名 kind」过滤 VLESS 安全模式(CLAUDE.md §4:本机出口域名直连=VLESS-TLS):
// - cf:只 tls(CF↔节点回源走 TLS)。
// - direct(域名直连):只 tls——用证书做 TLS;Reality 用 decoy SNI 不碰证书属 IP 直连、普通 none 不用证书,均不在此。
// - 留空(免证书/IP 直连):去掉 tls(留 reality/none)。
export function localExitVlessSecurityOptionsForDomainKind(kind: LocalExitDomainKind) {
  if (kind === 'cf' || kind === 'direct') {
    return vlessSecurityOptions.filter((option) => option.value === 'tls');
  }
  // IP 直连(免证书):本机出口 VLESS 放 Reality(默认,仅 TCP)+ 普通 none(裸连免证书,开 TCP+UDP/XUDP);
  // 与开发方案 §47「非 Reality VLESS 本机出口才允许 XHTTP/XUDP」、§237「本机出口口径不变」一致,不去掉 none。
  return vlessSecurityOptions.filter((option) => option.value !== 'tls');
}

// 本机出口传输选项（streamSettings.network，可多选，每个传输 = 一条线路）。
// SS/SOCKS/HTTP 无传输层只有 RAW；VLESS Reality 仅 RAW；VLESS 非 reality / Trojan 放 RAW + XHTTP；HY2 固定 UDP。
export function localExitTransportOptionsFor(
  protocol: string,
  security: VlessSecurityMode = 'reality',
): Array<{ label: string; value: string }> {
  const normalized = protocol.trim().toLowerCase();
  if (normalized === 'hysteria') {
    return [{ label: 'HY2（UDP）', value: 'udp' }];
  }
  if (normalized === 'vless') {
    return security === 'reality'
      ? [{ label: 'RAW（TCP）', value: 'tcp' }]
      : [{ label: 'RAW（TCP）', value: 'tcp' }, { label: 'XHTTP', value: 'xhttp' }];
  }
  if (normalized === 'trojan') {
    return [{ label: 'RAW（TCP）', value: 'tcp' }, { label: 'XHTTP', value: 'xhttp' }];
  }
  // shadowsocks / socks / http：无传输层，只有 RAW。
  return [{ label: 'RAW（TCP）', value: 'tcp' }];
}

// 本机出口承载选项（L4 单选）。后端 validate_local_exit_network_mode 仅 SS/SOCKS 接受合并 tcp,udp；
// 其它协议（含 HTTP/Trojan/VLESS）承载为单值；VLESS 非 reality 另含 xudp，Reality 仅 tcp，HY2 仅 udp。
export function localExitCarriageOptionsFor(
  protocol: string,
  security: VlessSecurityMode = 'reality',
): Array<{ label: string; value: string }> {
  const normalized = protocol.trim().toLowerCase();
  if (normalized === 'hysteria') {
    return [{ label: 'UDP', value: 'udp' }];
  }
  if (normalized === 'shadowsocks' || normalized === 'socks') {
    return [
      { label: 'TCP', value: 'tcp' },
      { label: 'UDP', value: 'udp' },
      { label: 'TCP + UDP', value: 'tcp,udp' },
    ];
  }
  if (normalized === 'http') {
    // HTTP 代理纯 TCP,无 UDP。
    return [{ label: 'TCP', value: 'tcp' }];
  }
  if (normalized === 'trojan') {
    // Trojan 一条 outbound 同收 TCP+UDP(UDP 走协议),无独立 UDP-only;落库 network_mode='tcp'。
    return [{ label: 'TCP + UDP', value: 'tcp' }];
  }
  if (normalized === 'vless') {
    // VLESS 一条 outbound 同收 TCP+UDP,XUDP 是 UDP 封装档(不单列);
    // 落库 network_mode tcp / xudp,后端编出 network:tcp(+packetEncoding xudp),不发会被后端拒的合并串。
    return security === 'reality'
      ? [{ label: '仅 TCP', value: 'tcp' }]
      : [
          { label: 'TCP + UDP', value: 'tcp' },
          { label: 'TCP + UDP + XUDP', value: 'xudp' },
        ];
  }
  return [{ label: 'TCP', value: 'tcp' }];
}

// 归一传输集合：去掉不在当前协议×安全允许集里的项，空则回落第一个允许项。
export function normalizeLocalExitTransports(
  protocol: string,
  security: VlessSecurityMode,
  transports: string[],
): string[] {
  const options = localExitTransportOptionsFor(protocol, security);
  const allowed = new Set(options.map((option) => option.value));
  const normalized = transports
    .map((transport) => transport.trim().toLowerCase())
    .filter((transport, index, values) => allowed.has(transport) && values.indexOf(transport) === index);
  return normalized.length > 0 ? normalized : [options[0]?.value ?? 'tcp'];
}

// 归一承载到当前协议×安全允许的合并/单值；不在选项里则回落第一项。
export function normalizeLocalExitCarriage(
  protocol: string,
  security: VlessSecurityMode,
  carriage: string,
): string {
  const options = localExitCarriageOptionsFor(protocol, security);
  const allowed = new Set(options.map((option) => option.value));
  const value = carriage.trim().toLowerCase();
  return allowed.has(value) ? value : (options[0]?.value ?? 'tcp');
}

export const localExitShadowsocksMethodOptions = [
  { label: '2022-blake3-aes-128-gcm', value: '2022-blake3-aes-128-gcm' },
  { label: '2022-blake3-aes-256-gcm', value: '2022-blake3-aes-256-gcm' },
  { label: 'aes-128-gcm', value: 'aes-128-gcm' },
  { label: 'aes-256-gcm', value: 'aes-256-gcm' },
  { label: 'chacha20-ietf-poly1305', value: 'chacha20-ietf-poly1305' },
];

const DEFAULT_REALITY_SERVER_NAME = 'www.cloudflare.com';
const DEFAULT_SHADOWSOCKS_METHOD = '2022-blake3-aes-128-gcm';

export function defaultLocalExitLineDetails(
  protocol: ThirdPartyExitEndpointOutboundType,
  host = '',
): Pick<LocalExitLineForm,
  | 'vlessSecurity'
  | 'uuid'
  | 'username'
  | 'password'
  | 'method'
  | 'serverName'
  | 'tlsCertificateFile'
  | 'tlsKeyFile'
> {
  const serverName = defaultServerName(protocol, host, protocol === 'vless' ? 'reality' : 'tls');
  return {
    vlessSecurity: protocol === 'vless' ? 'reality' : 'tls',
    uuid: randomUuid(),
    username: '',
    password: defaultPasswordForProtocol(protocol),
    method: protocol === 'shadowsocks' ? DEFAULT_SHADOWSOCKS_METHOD : '',
    serverName,
    tlsCertificateFile: serverName ? defaultTlsCertificateFile(serverName) : '',
    tlsKeyFile: serverName ? defaultTlsKeyFile(serverName) : '',
  };
}

export function buildLocalExitOutboundConfig(line: LocalExitLineForm, fallbackHost = '') {
  const host = line.host.trim() || fallbackHost.trim();
  const serverName =
    line.serverName.trim() || defaultServerName(line.outboundType, host, line.vlessSecurity);
  const password = line.password.trim() || defaultPasswordForProtocol(line.outboundType);

  if (line.outboundType === 'vless') {
    const config: Record<string, unknown> = {
      uuid: line.uuid.trim() || randomUuid(),
      security: line.vlessSecurity,
    };

    if (line.vlessSecurity === 'reality') {
      const realityServerName = serverName || DEFAULT_REALITY_SERVER_NAME;
      config.server_name = realityServerName;
      config.dest = `${realityServerName}:443`;
      config.flow = '';
      config.fingerprint = 'chrome';
    } else if (line.vlessSecurity === 'tls') {
      // TLS（域名直连）SNI=证书域名（所选域名），绝不回退 Reality 的 cloudflare decoy。
      const tlsServerName = serverName || (isIpAddress(host) ? '' : host);
      config.server_name = tlsServerName;
      config.flow = '';
      config.fingerprint = 'chrome';
    }

    return config;
  }

  if (line.outboundType === 'hysteria') {
    return withTlsFiles({
      password,
      server_name: requireGeneratedText(serverName, 'HY2 SNI/证书域名'),
    }, line, serverName);
  }

  if (line.outboundType === 'trojan') {
    return withTlsFiles({
      password,
      security: 'tls',
      server_name: requireGeneratedText(serverName, 'Trojan SNI/证书域名'),
    }, line, serverName);
  }

  if (line.outboundType === 'shadowsocks') {
    return {
      method: line.method.trim() || DEFAULT_SHADOWSOCKS_METHOD,
      password: password || randomBase64Key(16),
    };
  }

  return {
    username: line.username.trim(),
    password,
  };
}

export function defaultStreamConfigForNetworkMode(mode: string) {
  const network = mode === 'xudp' ? 'tcp' : mode;
  const config: Record<string, string> = {
    network_mode: mode,
    network,
  };
  if (mode === 'xudp') {
    config.udp_packet_encoding = 'xudp';
  }
  return JSON.stringify(config, null, 2);
}

export function parseJsonObject(value: string, label: string) {
  const trimmed = value.trim();
  if (!trimmed) {
    return {};
  }
  const parsed = JSON.parse(trimmed) as unknown;
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error(`${label} 必须是 JSON 对象`);
  }
  return parsed as Record<string, unknown>;
}

export function defaultServerName(
  protocol: ThirdPartyExitEndpointOutboundType,
  host = '',
  vlessSecurity: VlessSecurityMode = 'reality',
) {
  const normalizedHost = host.trim();
  const domainOrEmpty = !normalizedHost || isIpAddress(normalizedHost) ? '' : normalizedHost;
  if (protocol === 'vless') {
    // Reality 用大站 decoy SNI 伪装（IP 直连，不碰真证书）；TLS 用证书域名=所选域名；普通 none 无需 SNI。
    if (vlessSecurity === 'reality') {
      return DEFAULT_REALITY_SERVER_NAME;
    }
    return vlessSecurity === 'tls' ? domainOrEmpty : '';
  }
  return domainOrEmpty;
}

export function defaultTlsCertificateFile(serverName: string) {
  return `/etc/letsencrypt/live/${serverName}/fullchain.pem`;
}

export function defaultTlsKeyFile(serverName: string) {
  return `/etc/letsencrypt/live/${serverName}/privkey.pem`;
}

function defaultPasswordForProtocol(protocol: ThirdPartyExitEndpointOutboundType) {
  if (protocol === 'shadowsocks') {
    return randomBase64Key(16);
  }
  if (protocol === 'hysteria') {
    return randomCredential('hy2');
  }
  if (protocol === 'trojan') {
    return randomCredential('trojan');
  }
  if (protocol === 'socks' || protocol === 'http') {
    return randomCredential('pass');
  }
  return '';
}

function withTlsFiles(config: Record<string, unknown>, line: LocalExitLineForm, serverName: string) {
  const certificateFile = line.tlsCertificateFile.trim() || defaultTlsCertificateFile(serverName);
  const keyFile = line.tlsKeyFile.trim() || defaultTlsKeyFile(serverName);
  return {
    ...config,
    certificate_file: certificateFile,
    key_file: keyFile,
  };
}

function requireGeneratedText(value: string, label: string) {
  const trimmed = value.trim();
  if (!trimmed) {
    throw new Error(`${label} 不能为空。`);
  }
  return trimmed;
}

function isIpAddress(value: string) {
  return /^\d{1,3}(?:\.\d{1,3}){3}$/.test(value) || value.includes(':');
}

function randomUuid() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const value = Math.floor(Math.random() * 16);
    const digit = char === 'x' ? value : (value & 0x3) | 0x8;
    return digit.toString(16);
  });
}

function randomCredential(prefix: string) {
  const random = randomUuid().replace(/-/g, '');
  return `${prefix}-${random.slice(0, 24)}`;
}

function randomBase64Key(length: number) {
  const bytes = new Uint8Array(length);
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    crypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary);
}
