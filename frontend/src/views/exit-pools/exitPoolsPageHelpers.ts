/**
 * 用途：沉淀出口池页面的选项、文案和协议配置校验逻辑。
 * 该文件只包含出口管理和本机出口复用的纯 helper。
 */
import type {
  ExitEndpointOutboundType,
  ExitEndpointSummary,
  ExitPool,
  ExitPoolMemberStatus,
  ThirdPartyExitEndpointOutboundType,
} from '@/services/api';

export type ExitPoolStrategy = 'priority';
export type VlessSecurityMode = 'none' | 'reality' | 'tls';

export const poolStrategyOptions: Array<{ label: string; value: ExitPoolStrategy; description: string }> = [
  {
    label: '固定顺序',
    value: 'priority',
    description: '按优先级和启用状态选择可用线路。',
  },
];

export const upstreamOutboundOptions: Array<{ label: string; value: ThirdPartyExitEndpointOutboundType }> = [
  { label: 'SOCKS5', value: 'socks' },
  { label: 'HTTP', value: 'http' },
  { label: 'VLESS', value: 'vless' },
  { label: 'Trojan', value: 'trojan' },
  { label: 'Shadowsocks', value: 'shadowsocks' },
  { label: 'Hysteria2', value: 'hysteria' },
];

export const vlessSecurityOptions: Array<{ label: string; value: VlessSecurityMode }> = [
  { label: '普通 VLESS', value: 'none' },
  { label: 'Reality', value: 'reality' },
  { label: 'TLS', value: 'tls' },
];

export function isHongKongRegionCode(regionCode: string) {
  const normalized = regionCode.trim().toUpperCase();
  return ['HK', 'HKG', 'HONGKONG', 'HONG_KONG', '香港'].includes(normalized);
}

export function outboundOptionsForRegion(regionCode: string) {
  const hk = isHongKongRegionCode(regionCode);
  return upstreamOutboundOptions.map((option) => ({
    ...option,
    disabled: hk && option.value === 'hysteria',
  }));
}

export function ensureNoHongKongHysteria(outboundType: string, regionCode: string) {
  if (outboundType === 'hysteria' && isHongKongRegionCode(regionCode)) {
    throw new Error('香港线路不支持 HY2，请改用 VLESS、Trojan 或 Shadowsocks。');
  }
}

export function statusType(status: ExitPool['status']) {
  if (status === 'healthy') {
    return 'success';
  }

  return status === 'degraded' ? 'warning' : 'danger';
}

export function memberStatusType(member: ExitEndpointSummary) {
  if (!member.statusKnown) {
    return 'info';
  }

  if (member.status === 'healthy') {
    return 'success';
  }

  if (member.status === 'degraded' || member.status === 'draining') {
    return 'warning';
  }

  return member.status === 'unknown' ? 'info' : 'danger';
}

export function memberStatusLabel(member: ExitEndpointSummary) {
  if (!member.statusKnown) {
    return member.healthy ? '健康（状态未回显）' : '异常（状态未回显）';
  }

  const labels: Record<ExitPoolMemberStatus, string> = {
    healthy: '健康',
    degraded: '降级',
    draining: '排空',
    offline: '离线',
    unknown: '未知',
  };

  return labels[member.status];
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

export function poolStrategyLabel(strategy: string) {
  return poolStrategyOption(strategy)?.label ?? strategy;
}

export function poolStrategyDescription(strategy: string) {
  return poolStrategyOption(strategy)?.description ?? '后端返回的自定义策略。';
}

export function defaultOutboundConfig(outboundType: ExitEndpointOutboundType) {
  if (outboundType === 'socks' || outboundType === 'http') {
    return '{\n  "username": "",\n  "password": ""\n}';
  }
  if (outboundType === 'vless') {
    return '{\n  "uuid": "",\n  "security": "reality",\n  "server_name": "",\n  "public_key": "",\n  "short_id": "",\n  "flow": "",\n  "fingerprint": "chrome"\n}';
  }
  if (outboundType === 'trojan' || outboundType === 'hysteria') {
    return '{\n  "password": "",\n  "server_name": ""\n}';
  }
  if (outboundType === 'shadowsocks') {
    return '{\n  "method": "2022-blake3-aes-128-gcm",\n  "password": ""\n}';
  }

  return '{}';
}

export function vlessSecurityFromConfigText(value: string): VlessSecurityMode {
  try {
    return vlessSecurity(parseJsonObject(value));
  } catch {
    return 'reality';
  }
}

export function setVlessOutboundSecurity(value: string, security: VlessSecurityMode) {
  const config = parseJsonObjectOrDefault(value, 'vless');
  config.security = security;
  ensureConfigKey(config, 'uuid', '');

  if (security === 'reality') {
    ensureConfigKey(config, 'server_name', '');
    ensureConfigKey(config, 'flow', '');
    ensureConfigKey(config, 'fingerprint', 'chrome');
    ensureConfigKey(config, 'public_key', '');
    ensureConfigKey(config, 'short_id', '');
  } else if (security === 'tls') {
    ensureConfigKey(config, 'server_name', '');
    ensureConfigKey(config, 'flow', '');
    ensureConfigKey(config, 'fingerprint', 'chrome');
    delete config.dest;
    delete config.reality_dest;
    delete config.realityDest;
    delete config.public_key;
    delete config.publicKey;
    delete config.reality_public_key;
    delete config.private_key;
    delete config.privateKey;
    delete config.reality_private_key;
    delete config.realityPrivateKey;
    delete config.short_id;
    delete config.shortId;
    delete config.reality_short_id;
    delete config.realityShortId;
  } else {
    delete config.server_name;
    delete config.serverName;
    delete config.servername;
    delete config.sni;
    delete config.dest;
    delete config.reality_dest;
    delete config.realityDest;
    delete config.flow;
    delete config.fingerprint;
    delete config.public_key;
    delete config.publicKey;
    delete config.reality_public_key;
    delete config.private_key;
    delete config.privateKey;
    delete config.reality_private_key;
    delete config.realityPrivateKey;
    delete config.short_id;
    delete config.shortId;
    delete config.reality_short_id;
    delete config.realityShortId;
  }

  return JSON.stringify(config, null, 2);
}

export function parseJsonObject(value: string) {
  const trimmed = value.trim();
  if (!trimmed) {
    return {};
  }
  const parsed = JSON.parse(trimmed) as unknown;
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('出口配置必须是 JSON 对象');
  }
  return parsed as Record<string, unknown>;
}

export function protocolConfigHint(outboundType: ExitEndpointOutboundType) {
  const hints: Record<ExitEndpointOutboundType, string> = {
    direct: '历史兼容出口仅用于保留旧数据，连接方式配置必须为 {}，不可作为当前新增出口。',
    socks: 'SOCKS 必填地址和端口；认证可为空，如填写用户名必须同时填写密码。',
    http: 'HTTP 必填地址和端口；认证可为空，如填写用户名必须同时填写密码。',
    vless: 'VLESS 支持普通、TLS、Reality 三种安全模式；普通模式只需 uuid；TLS 建议填写 server_name/SNI；Reality 需要 uuid、server_name、public_key，short_id 可为空。',
    trojan:
      'Trojan 线路默认按 TLS/SNI 配置，必填 password；用户入站必须有有效证书。security=none 仅可作为旧配置兼容输入，不能作为发布通过证据。',
    shadowsocks: 'Shadowsocks 必填 method 和 password。',
    hysteria: 'Hysteria2 必填 password 或 auth，可选 server_name。',
  };

  return hints[outboundType];
}

export function validateProtocolConfig(outboundType: ExitEndpointOutboundType, config: Record<string, unknown>) {
  if (outboundType === 'direct') {
    if (Object.keys(config).length > 0) {
      throw new Error('历史兼容出口连接方式配置必须为 {}。');
    }
    return;
  }

  if (outboundType === 'socks' || outboundType === 'http') {
    const username = configText(config, ['username', 'user']);
    const password = configText(config, ['password', 'pass']);
    if (Boolean(username) !== Boolean(password)) {
      throw new Error(`${outboundType.toUpperCase()} 认证用户名和密码必须同时填写或同时为空。`);
    }
    return;
  }

  if (outboundType === 'vless') {
    requireConfigText(config, ['uuid', 'id'], 'VLESS uuid');
    const security = vlessSecurity(config);
    if (security === 'reality') {
      requireConfigText(config, ['server_name', 'servername', 'sni'], 'VLESS server_name');
      requireConfigText(config, ['public_key', 'publicKey', 'reality_public_key'], 'VLESS Reality public_key');
    }
    return;
  }

  if (outboundType === 'trojan') {
    requireConfigText(config, ['password'], 'Trojan password');
    const security = configText(config, ['security'])?.toLowerCase() || 'tls';
    if (security !== 'tls') {
      throw new Error(`Trojan security 仅支持 tls，当前为 ${security}。`);
    }
    requireConfigText(config, ['server_name', 'servername', 'sni'], 'Trojan server_name/SNI');
    return;
  }

  if (outboundType === 'shadowsocks') {
    requireConfigText(config, ['method', 'cipher'], 'Shadowsocks method');
    requireConfigText(config, ['password'], 'Shadowsocks password');
    return;
  }

  if (outboundType === 'hysteria') {
    requireConfigText(config, ['password', 'auth'], 'Hysteria2 password/auth');
  }
}

function poolStrategyOption(strategy: string) {
  return poolStrategyOptions.find((option) => option.value === strategy);
}

function parseJsonObjectOrDefault(value: string, outboundType: ExitEndpointOutboundType) {
  try {
    return parseJsonObject(value);
  } catch {
    return parseJsonObject(defaultOutboundConfig(outboundType));
  }
}

function ensureConfigKey(config: Record<string, unknown>, key: string, defaultValue: string) {
  if (!(key in config)) {
    config[key] = defaultValue;
  }
}

function requireConfigText(config: Record<string, unknown>, keys: string[], label: string) {
  if (!configText(config, keys)) {
    throw new Error(`${label} 不能为空。`);
  }
}

function vlessSecurity(config: Record<string, unknown>): VlessSecurityMode {
  const explicitSecurity = configText(config, ['security']).toLowerCase();
  if (explicitSecurity === 'none' || explicitSecurity === 'tls' || explicitSecurity === 'reality') {
    return explicitSecurity;
  }
  if (explicitSecurity) {
    throw new Error(`VLESS security 仅支持 none、tls 或 reality，当前为 ${explicitSecurity}。`);
  }

  return configText(config, ['public_key', 'publicKey', 'reality_public_key'])
    || configText(config, ['short_id', 'shortId', 'reality_short_id', 'realityShortId'])
    ? 'reality'
    : 'tls';
}

function configText(config: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const value = config[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
}
