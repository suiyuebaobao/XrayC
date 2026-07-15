// 本文件负责出口管理粘贴内容的自动识别。
// 解析器只做字段提取和置信度提示，不直接保存数据库。
// 支持 Clash/mihomo YAML、常见 URI、Xray outbound JSON 和半结构化文本。
// 不确定的字段会保留为空并返回警告，由管理员人工确认。
// 后续扩展新协议时，应先加解析测试再扩展这里的映射规则。

import type { ExitEndpointConfig, ThirdPartyExitEndpointOutboundType } from '@/services/api';

export type ParsedThirdPartyLine = {
  resourceName: string;
  regionCode: string;
  providerName: string;
  outboundType: ThirdPartyExitEndpointOutboundType;
  host: string;
  port: number;
  outboundConfig: ExitEndpointConfig;
  streamConfig: ExitEndpointConfig;
  probeConfig: ExitEndpointConfig;
  confidence: 'high' | 'medium' | 'low';
  warnings: string[];
};

export type ParseThirdPartyLinesResult = {
  lines: ParsedThirdPartyLine[];
  errors: string[];
};

type LooseRecord = Record<string, unknown>;

const uriPattern = /\b(vless|trojan|ss|hysteria2|hy2|socks5?|http|https):\/\/[^\s'"<>]+/gi;

export function parseThirdPartyLines(raw: string): ParseThirdPartyLinesResult {
  const text = raw.trim();
  if (!text) {
    return { lines: [], errors: [] };
  }

  const parsers = [parseJsonInput, parseYamlInput, parseUriInput, parsePlainTextInput];
  for (const parser of parsers) {
    const lines = parser(text);
    if (lines.length > 0) {
      return { lines, errors: [] };
    }
  }

  return { lines: [], errors: ['未识别到可导入的线路，请检查格式或手动填写。'] };
}

function parseJsonInput(text: string) {
  try {
    const value = JSON.parse(text) as unknown;
    const records = Array.isArray(value)
      ? value
      : Array.isArray(record(value).outbounds)
        ? record(value).outbounds as unknown[]
        : [value];
    return records.map(recordToLine).filter(isParsedLine);
  } catch {
    return [];
  }
}

function parseYamlInput(text: string) {
  if (!/^\s*(proxies\s*:|-\s*name\s*:|name\s*:)/m.test(text)) {
    return [];
  }
  const blocks: LooseRecord[] = [];
  let current: LooseRecord | null = null;
  let nestedKey = '';
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed === 'proxies:') {
      continue;
    }
    const listItem = trimmed.match(/^-\s*(.*)$/);
    if (listItem) {
      if (current) {
        blocks.push(current);
      }
      current = {};
      nestedKey = '';
      parseYamlPair(current, listItem[1], nestedKey);
      continue;
    }
    if (!current) {
      current = {};
    }
    const keyOnly = trimmed.match(/^([A-Za-z0-9_-]+)\s*:\s*$/);
    if (keyOnly) {
      nestedKey = keyOnly[1];
      current[nestedKey] = record(current[nestedKey]);
      continue;
    }
    parseYamlPair(current, trimmed, nestedKey);
  }
  if (current) {
    blocks.push(current);
  }
  return blocks.map(recordToLine).filter(isParsedLine);
}

function parseYamlPair(target: LooseRecord, text: string, nestedKey: string) {
  const pair = text.match(/^([A-Za-z0-9_-]+)\s*:\s*(.*)$/);
  if (!pair) {
    return;
  }
  const key = pair[1];
  const value = parseScalar(pair[2]);
  if (nestedKey && target[nestedKey] && typeof target[nestedKey] === 'object') {
    (target[nestedKey] as LooseRecord)[key] = value;
    return;
  }
  target[key] = value;
}

function parseUriInput(text: string) {
  const uris = [...text.matchAll(uriPattern)].map((match) => match[0]);
  return uris.map(uriToLine).filter(isParsedLine);
}

function uriToLine(uri: string): ParsedThirdPartyLine | null {
  try {
    const url = new URL(uri);
    const protocol = normalizeType(url.protocol.replace(':', ''));
    if (!protocol) {
      return null;
    }
    const params = url.searchParams;
    const name = decodeURIComponent(url.hash.replace(/^#/, '')) || url.hostname;
    const username = decodeURIComponent(url.username || '');
    const password = decodeURIComponent(url.password || '');
    const base: LooseRecord = {
      name,
      type: protocol,
      server: url.hostname,
      port: url.port,
      uuid: protocol === 'vless' ? username : '',
      username: protocol === 'socks' || protocol === 'http' ? username : '',
      password: protocol === 'vless' ? '' : (password || username),
      method: protocol === 'shadowsocks' ? username.split(':')[0] : '',
      network: params.get('type') || params.get('network') || params.get('net') || '',
      security: params.get('security') || '',
      servername: params.get('sni') || params.get('servername') || params.get('peer') || '',
      flow: params.get('flow') || '',
      'client-fingerprint': params.get('fp') || params.get('fingerprint') || '',
      'reality-opts': {
        'public-key': params.get('pbk') || params.get('public-key') || '',
        'short-id': params.get('sid') || params.get('short-id') || '',
      },
    };
    return recordToLine(base);
  } catch {
    return null;
  }
}

function parsePlainTextInput(text: string) {
  const protocol = normalizeType(firstMatch(text, /\b(vless|trojan|shadowsocks|ss|hy2|hysteria2|socks5?|http)\b/i));
  const host = firstMatch(text, /\b(?:server|host|address|addr)\s*[:=]\s*([A-Za-z0-9.-]+)/i)
    || firstMatch(text, /\b((?:\d{1,3}\.){3}\d{1,3}|[A-Za-z0-9][A-Za-z0-9.-]+\.[A-Za-z]{2,})\b/);
  const port = Number(firstMatch(text, /\bport\s*[:=]\s*(\d{1,5})\b/i) || firstMatch(text, /^\s*(\d{2,5})\s*$/m) || 0);
  if (!protocol || !host || !port) {
    return [];
  }
  const line = recordToLine({
    name: firstMatch(text, /\bname\s*[:=]\s*["']?([^"'\n]+)/i) || host,
    type: protocol,
    server: host,
    port,
    uuid: firstMatch(text, /\b(?:uuid|id)\s*[:=]\s*([0-9a-f-]{32,36})/i),
    password: firstMatch(text, /\b(?:password|pass|passwd|auth)\s*[:=]\s*([^\s]+)/i),
    username: firstMatch(text, /\b(?:username|user|account)\s*[:=]\s*([^\s]+)/i),
    network: firstMatch(text, /\b(?:network|net|transport)\s*[:=]\s*([^\s]+)/i),
    tls: firstMatch(text, /\btls\s*[:=]\s*([^\s]+)/i),
    servername: firstMatch(text, /\b(?:sni|servername|server_name)\s*[:=]\s*([^\s]+)/i),
    security: firstMatch(text, /\b(?:security|tls)\s*[:=]\s*([^\s]+)/i),
    flow: firstMatch(text, /\bflow\s*[:=]\s*([^\s]+)/i),
    'client-fingerprint': firstMatch(text, /\b(?:fp|fingerprint)\s*[:=]\s*([^\s]+)/i),
    'reality-opts': {
      'public-key': firstMatch(text, /\b(?:pbk|public-key|public_key)\s*[:=]\s*([^\s]+)/i),
      'short-id': firstMatch(text, /\b(?:sid|short-id|short_id)\s*[:=]\s*([^\s]*)/i),
    },
  });
  return line ? [line] : [];
}

function recordToLine(value: unknown): ParsedThirdPartyLine | null {
  const data = record(value);
  const outboundType = normalizeType(textValue(data.type ?? data.protocol ?? data.outbound_type));
  const host = textValue(data.server ?? data.host ?? data.address ?? data.addr);
  const port = Number(data.port || 0);
  if (!outboundType || !host || !port) {
    return null;
  }
  const name = textValue(data.name ?? data.remarks ?? data.ps) || `${host}:${port}`;
  const nestedReality = record(data['reality-opts'] ?? data.reality_opts ?? data.reality);
  const network = textValue(data.network ?? data.net ?? data.transport ?? data.type);
  const serverName = textValue(data.servername ?? data.server_name ?? data.sni ?? data.peer);
  const publicKey = textValue(nestedReality['public-key'] ?? nestedReality.public_key ?? data.public_key ?? data.pbk);
  const shortId = textValue(nestedReality['short-id'] ?? nestedReality.short_id ?? data.short_id ?? data.sid);
  const security = normalizeSecurity(data, publicKey);
  const outboundConfig = configForType(outboundType, data, { serverName, publicKey, shortId, security });
  const warnings = warningsForLine(outboundType, outboundConfig);
  return {
    resourceName: name,
    regionCode: guessRegion(name),
    providerName: '',
    outboundType,
    host,
    port,
    outboundConfig,
    streamConfig: network ? { network } : {},
    probeConfig: {},
    confidence: warnings.length === 0 ? 'high' : 'medium',
    warnings,
  };
}

function configForType(
  outboundType: ThirdPartyExitEndpointOutboundType,
  data: LooseRecord,
  reality: { serverName: string; publicKey: string; shortId: string; security: string },
) {
  if (outboundType === 'vless') {
    return compact({
      uuid: textValue(data.uuid ?? data.id),
      security: reality.security,
      server_name: reality.serverName,
      flow: textValue(data.flow),
      fingerprint: textValue(data['client-fingerprint'] ?? data.fingerprint ?? data.fp),
      public_key: reality.publicKey,
      short_id: reality.shortId,
    });
  }
  if (outboundType === 'trojan' || outboundType === 'hysteria') {
    return compact({ password: textValue(data.password ?? data.pass ?? data.auth), server_name: reality.serverName });
  }
  if (outboundType === 'shadowsocks') {
    return compact({ method: textValue(data.cipher ?? data.method), password: textValue(data.password ?? data.pass) });
  }
  return compact({
    username: textValue(data.username ?? data.user ?? data.account),
    password: textValue(data.password ?? data.pass ?? data.auth),
  });
}

function warningsForLine(outboundType: ThirdPartyExitEndpointOutboundType, config: ExitEndpointConfig) {
  const warnings: string[] = [];
  if (outboundType === 'vless' && !textValue(config.uuid)) {
    warnings.push('VLESS 缺少 UUID');
  }
  if (outboundType === 'vless' && textValue(config.security) === 'reality' && !textValue(config.public_key)) {
    warnings.push('Reality 缺少 public_key');
  }
  if ((outboundType === 'trojan' || outboundType === 'hysteria' || outboundType === 'shadowsocks') && !textValue(config.password)) {
    warnings.push('缺少密码或认证字段');
  }
  return warnings;
}

function normalizeType(value: unknown): ThirdPartyExitEndpointOutboundType | '' {
  const normalized = textValue(value).toLowerCase();
  if (normalized === 'socks' || normalized === 'socks5') {
    return 'socks';
  }
  if (normalized === 'http' || normalized === 'https') {
    return 'http';
  }
  if (normalized === 'ss' || normalized === 'shadowsocks') {
    return 'shadowsocks';
  }
  if (normalized === 'hy2' || normalized === 'hysteria2' || normalized === 'hysteria') {
    return 'hysteria';
  }
  if (normalized === 'vless' || normalized === 'trojan') {
    return normalized;
  }
  return '';
}

function normalizeSecurity(data: LooseRecord, publicKey: string) {
  const security = textValue(data.security).toLowerCase();
  if (security === 'none' || security === 'reality' || security === 'tls') {
    return security;
  }
  if (publicKey) {
    return 'reality';
  }
  return data.tls === true || textValue(data.tls) === 'true' ? 'tls' : '';
}

function parseScalar(value: string) {
  const trimmed = value.trim();
  if (trimmed === 'true') {
    return true;
  }
  if (trimmed === 'false') {
    return false;
  }
  if (/^\d+$/.test(trimmed)) {
    return Number(trimmed);
  }
  return cleanImportedScalar(trimmed);
}

function compact(value: LooseRecord) {
  return Object.fromEntries(Object.entries(value).filter(([, item]) => item !== undefined && item !== null)) as ExitEndpointConfig;
}

function record(value: unknown): LooseRecord {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as LooseRecord : {};
}

function textValue(value: unknown) {
  return typeof value === 'string'
    ? cleanImportedScalar(value)
    : value === undefined || value === null
      ? ''
      : cleanImportedScalar(String(value));
}

function firstMatch(text: string, pattern: RegExp) {
  return cleanImportedScalar(text.match(pattern)?.[1] ?? '');
}

function cleanImportedScalar(value: string) {
  return value
    .trim()
    .replace(/^['"`]|['"`]$/g, '')
    .replace(/[;,]+$/g, '')
    .trim();
}

function guessRegion(name: string) {
  const value = name.toLowerCase();
  if (/(hk|香港|hong\s*kong)/i.test(value)) return 'HK';
  if (/(us|美国|united\s*states|america)/i.test(value)) return 'US';
  if (/(sg|新加坡|singapore)/i.test(value)) return 'SG';
  if (/(jp|日本|japan)/i.test(value)) return 'JP';
  if (/(tw|台湾|taiwan)/i.test(value)) return 'TW';
  return '';
}

function isParsedLine(value: ParsedThirdPartyLine | null): value is ParsedThirdPartyLine {
  return Boolean(value);
}
