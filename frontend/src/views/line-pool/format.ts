// 本文件集中维护出口管理列表的显示格式化逻辑。
// 它只处理协议标签、账号凭据展示和 JSON 展示。
// 页面组件不在这里发请求，也不修改表单状态。
// 管理后台允许展示真实线路配置，普通用户侧仍不能复用这些函数。
// 后续新增协议时，应同步更新协议标签和凭据提取规则。

import type { ExitEndpointConfig, ExitEndpointSummary } from '@/services/api';

const outboundTypeLabels: Record<string, string> = {
  direct: 'Direct',
  socks: 'SOCKS5',
  http: 'HTTP',
  vless: 'VLESS',
  trojan: 'Trojan',
  shadowsocks: 'Shadowsocks',
  hysteria: 'HY2',
};

export function formatJson(value: ExitEndpointConfig) {
  return JSON.stringify(value && typeof value === 'object' ? value : {}, null, 2);
}

export function protocolLabel(outboundType: string) {
  return outboundTypeLabels[outboundType] ?? outboundType;
}

export function lineAccount(line: ExitEndpointSummary) {
  const config = configRecord(line.outboundConfig);
  if (line.outboundType === 'vless') {
    return configValue(config, ['uuid', 'id']) || '-';
  }
  if (line.outboundType === 'hysteria') {
    const auth = configValue(config, ['auth']);
    return auth ? `auth: ${auth}` : 'HY2 使用 password/auth，无账号/UUID';
  }

  return configValue(config, ['username', 'user', 'account']) || '-';
}

export function lineSecret(line: ExitEndpointSummary) {
  const config = configRecord(line.outboundConfig);
  if (line.outboundType === 'vless') {
    return configPairs(config, [
      ['public_key', ['public_key', 'publicKey', 'reality_public_key']],
      ['short_id', ['short_id', 'shortId', 'reality_short_id', 'realityShortId']],
    ]) || '-';
  }
  if (line.outboundType === 'hysteria') {
    return configPairs(config, [
      ['password/auth', ['password', 'auth']],
      ['server_name', ['server_name', 'servername', 'sni']],
    ]) || '-';
  }

  return configValue(config, ['password', 'pass', 'auth', 'key', 'private_key', 'privateKey', 'public_key', 'publicKey'])
    || '-';
}

function configRecord(value: ExitEndpointConfig) {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function configValue(config: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const value = config[key];
    if (value === undefined || value === null || value === '') {
      continue;
    }
    return typeof value === 'string' ? value : JSON.stringify(value);
  }
  return '';
}

function configPairs(config: Record<string, unknown>, pairs: Array<[string, string[]]>) {
  return pairs
    .map(([label, keys]) => {
      const value = configValue(config, keys);
      return value ? `${label}: ${value}` : '';
    })
    .filter(Boolean)
    .join('\n');
}
