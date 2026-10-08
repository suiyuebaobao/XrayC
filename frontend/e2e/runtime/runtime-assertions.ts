/*
 * 用途：封装 runtime no-mock e2e 的页面断言和订阅 YAML 校验。
 * spec 文件只保留用户态与管理态页面流程，降低单文件行数。
 */
import { expect, type Page, test } from '@playwright/test';

import {
  formatGb,
  formatPrice,
  multiplierLabel,
  type ControlPlane,
  type ExitEndpointInfo,
  type ExitPool,
  type OperationsSummary,
  type PlanInfo,
  type SubscriptionInfo,
} from './runtime-data';
import { type JsonRecord, numberValue, stringValue } from './runtime-values';

export async function expectVisibleText(page: Page, text: string | RegExp) {
  await expect(page.getByText(text).first()).toBeVisible();
}

export async function expectMetricCard(page: Page, label: string, value: string | RegExp) {
  const card = page.locator('.metric-card, .overview-metrics > article').filter({ hasText: label }).first();
  await expect(card).toBeVisible();
  await expect(card.locator('strong').filter({ hasText: value }).first()).toBeVisible();
}

export async function assertNoFakeZeroRuntimeCards(page: Page, summary: OperationsSummary) {
  if (summary.runtimeMetricStatus !== 'no_data' && summary.runtimeMetricStatus !== 'unknown') {
    return;
  }

  const expected = summary.runtimeMetricStatus === 'no_data' ? '未上报' : '未知';
  await expectMetricCard(page, '在线用户', expected);
  await expectMetricCard(page, '活跃连接', expected);
  await expectMetricCardNotValue(page, '在线用户', '0');
  await expectMetricCardNotValue(page, '活跃连接', '0');
}

export function assertSubscriptionLines(page: Page, subscription: SubscriptionInfo) {
  if (subscription.visibleLines.length === 0) {
    return Promise.all([
      expectVisibleText(page, '名称'),
      expectVisibleText(page, '地区'),
      expectVisibleText(page, '客户端连接地址'),
      expectVisibleText(page, '协议'),
    ]);
  }

  const line = subscription.visibleLines[0];
  return Promise.all([
    line.name ? expectVisibleText(page, line.name) : Promise.resolve(),
    line.region ? expectVisibleText(page, line.region) : Promise.resolve(),
    line.server && line.port ? expectVisibleText(page, `${line.server}:${line.port}`) : Promise.resolve(),
    line.protocol ? expectVisibleText(page, line.protocol) : Promise.resolve(),
  ]);
}

export async function assertSubscriptionYamlDownload(page: Page, subscription: SubscriptionInfo) {
  if (!subscription.token || subscription.visibleLines.length === 0) {
    if (process.env.XRAYC_REAL_RELEASE === '1' || process.env.E2E_REQUIRE_NO_SKIP === '1') {
      throw new Error('real no-mock subscription YAML download requires a token and at least one visible access line');
    }
    test.info().annotations.push({
      type: 'skip-yaml-download',
      description: 'real backend subscription has no token or visible access_lines',
    });
    return;
  }

  const response = await page.request.get(`/sub/${encodeURIComponent(subscription.token)}`);
  expect(response.status(), 'subscription YAML download status').toBeGreaterThanOrEqual(200);
  expect(response.status(), 'subscription YAML download status').toBeLessThan(300);

  const yaml = await response.text();
  assertNoSensitiveSubscriptionYamlLeak(yaml);

  const proxies = parseSubscriptionYamlProxies(yaml);
  expect(proxies.length, 'subscription YAML proxies').toBeGreaterThan(0);

  const allowedServers = new Set(
    subscription.visibleLines
      .filter((line) => line.server && line.port > 0)
      .map((line) => `${line.server}:${line.port}`),
  );
  expect(allowedServers.size, 'subscription access_lines with server:port').toBeGreaterThan(0);
  expect(
    proxies.length,
    'subscription YAML proxy count must not exceed visible access_lines',
  ).toBeLessThanOrEqual(subscription.visibleLines.length);

  for (const proxy of proxies) {
    expect(proxy.server, 'subscription YAML proxy server').toBeTruthy();
    expect(proxy.port, 'subscription YAML proxy port').toBeGreaterThan(0);
    expect(proxy.type.toLowerCase(), 'subscription YAML raw proxy protocol').not.toMatch(/^(socks|socks5|http)$/);
    expect(
      allowedServers.has(`${proxy.server}:${proxy.port}`),
      `subscription YAML proxy endpoint must come from access_lines: ${proxy.server}:${proxy.port}`,
    ).toBeTruthy();
  }
}

export function assertOperationsRuntimeLines(page: Page, controlPlane: ControlPlane) {
  if (controlPlane.accessLines.length === 0) {
    return expect(page.getByText('暂无中转入口 / 未上报')).toBeVisible();
  }

  const line = controlPlane.accessLines[0];
  return Promise.all([
    expectVisibleText(page, line.name || line.uuid || '未命名线路'),
    line.accessNode ? expectVisibleText(page, line.accessNode) : Promise.resolve(),
    line.listenHost && line.listenPort ? expectVisibleText(page, `${line.listenHost}:${line.listenPort}`) : Promise.resolve(),
  ]);
}

export function assertAccessRoutingRows(page: Page, controlPlane: ControlPlane) {
  if (controlPlane.accessLines.length === 0) {
    return Promise.all([
      expectVisibleText(page, '运行入口数'),
      expectVisibleText(page, '客户端连接地址'),
      expectVisibleText(page, '该中转节点还没有运行入口'),
      expectVisibleText(page, '出口线路'),
    ]);
  }

  const line = controlPlane.accessLines[0];
  const boundLineName = line.exitEndpointName || line.lineGroupName;
  return Promise.all([
    line.accessNode ? expectVisibleText(page, line.accessNode) : Promise.resolve(),
    line.listenHost && line.listenPort ? expectVisibleText(page, `${line.listenHost}:${line.listenPort}`) : Promise.resolve(),
    boundLineName ? expectVisibleText(page, boundLineName) : Promise.resolve(),
    expectVisibleText(page, line.status),
  ]);
}

export function assertExitPoolRows(page: Page, pools: ExitPool[]) {
  if (pools.length === 0) {
    return Promise.all([
      expectVisibleText(page, '分组'),
      expectVisibleText(page, '可用线路'),
      expectVisibleText(page, '分配策略'),
      expectVisibleText(page, '出口映射'),
    ]);
  }

  const pool = pools[0];
  return Promise.all([
    pool.name ? expectVisibleText(page, pool.name) : Promise.resolve(),
    pool.region ? expectVisibleText(page, pool.region) : Promise.resolve(),
    expectVisibleText(page, `${pool.healthyMembers} / ${pool.totalMembers}`),
    pool.strategy ? expectVisibleText(page, exitPoolStrategyLabel(pool.strategy)) : Promise.resolve(),
    expectVisibleText(page, String(pool.activeAssignments)),
    expectVisibleText(page, pool.status),
  ]);
}

export async function assertLinePoolEndpointRows(page: Page, endpoints: ExitEndpointInfo[]) {
  const table = page.locator('.el-table').first();
  await expect(table).toContainText('协议');
  await expect(table).toContainText('地址');
  await expect(table).toContainText('账号');
  await expect(table).toContainText('密码');
  await expect(table).toContainText('配置 JSON');

  if (endpoints.length === 0) {
    return;
  }

  const endpoint = endpoints.find((item) => item.name || item.resourceName || item.host || item.port > 0) ?? endpoints[0];
  const displayName = endpoint.name || endpoint.resourceName;
  if (displayName) {
    await expect(table).toContainText(displayName);
  }
  if (endpoint.outboundType) {
    await expect(table).toContainText(exitEndpointProtocolLabel(endpoint.outboundType));
  }
  if (endpoint.host) {
    await expect(table).toContainText(endpoint.host);
  }
  if (endpoint.port > 0) {
    await expect(table).toContainText(String(endpoint.port));
  }

  const account = endpointConfigText(endpoint.outboundConfig, ['username', 'user', 'account', 'uuid', 'id', 'auth']);
  if (account) {
    await expect(table).toContainText(account);
  }

  const secret = endpointConfigText(endpoint.outboundConfig, [
    'password',
    'pass',
    'server_password',
    'public_key',
    'publicKey',
    'reality_public_key',
    'short_id',
    'shortId',
    'reality_short_id',
    'method',
    'cipher',
    'key',
  ]);
  if (secret) {
    await expect(table).toContainText(secret);
  }

  const configKey = Object.keys(endpoint.outboundConfig).find((key) => endpoint.outboundConfig[key] !== undefined);
  if (configKey) {
    await expect(table).toContainText(`"${configKey}"`);
  }
}

export async function assertAdminPlanRows(page: Page, plans: PlanInfo[]) {
  const table = page.locator('.el-table').first();
  await expect(table).toContainText('套餐');
  await expect(table).toContainText('流量额度');
  await expect(table).toContainText('价格 / 周期');
  await expect(table).toContainText('授权分组');

  if (plans.length === 0) {
    return;
  }

  await expect(table.locator('tbody tr').first()).toBeVisible();
  let visiblePlan: PlanInfo | undefined;
  for (const plan of plans) {
    if (!plan.name) {
      continue;
    }
    if (await table.getByText(plan.name, { exact: false }).first().count() > 0) {
      visiblePlan = plan;
      break;
    }
  }
  expect(visiblePlan, 'one API plan should be visible in the admin plan table').toBeTruthy();
  if (!visiblePlan) {
    return;
  }
  await expect(table).toContainText(visiblePlan.name);
  await expect(table).toContainText(formatGb(visiblePlan.trafficLimitGb));
  await expect(table).toContainText(multiplierLabel(visiblePlan.billingMultiplier));
  await expect(table).toContainText(formatPrice(visiblePlan));
  await expect(table).toContainText(`${visiblePlan.durationDays} 天`);
}

async function expectMetricCardNotValue(page: Page, label: string, value: string) {
  const card = page.locator('.metric-card, .overview-metrics > article').filter({ hasText: label }).first();
  await expect(card).toBeVisible();
  await expect(card.locator('strong')).not.toHaveText(value);
}

function assertNoSensitiveSubscriptionYamlLeak(yaml: string) {
  const forbiddenPatterns = [
    {
      name: 'raw proxy URL',
      pattern: /\b(?:(?:vless|trojan|ss|ssr|hysteria2|hy2|socks|socks4|socks4a|socks5|socks5h):\/\/[^\s'"]+|https?:\/\/[^\/\s'"]+:[0-9]+(?:\/|\b))/i,
    },
    {
      name: 'control-plane or exit endpoint field',
      pattern: /\b(?:outbound_proxy_url|proxy_url|agent_token|private[ _-]?key|secret_key|exit_endpoint|exitEndpoint)\b\s*[:=]/i,
    },
    {
      name: 'third-party credential placeholder',
      pattern: /\b(?:third[_ -]?party|upstream)[_\s-]*(?:proxy|host|port|user|username|password|secret|token|credential)s?\b\s*[:=]/i,
    },
    {
      name: 'template credential placeholder',
      pattern: /(?:\$\{[^}]*?(?:proxy|host|user|username|password|secret|token|credential)[^}]*\}|<[^>]*?(?:proxy|host|user|username|password|secret|token|credential)[^>]*>)/i,
    },
  ];

  for (const { name, pattern } of forbiddenPatterns) {
    expect(yaml, `subscription YAML must not contain ${name}`).not.toMatch(pattern);
  }
  expect(yaml, 'subscription YAML must not contain proxy or group icon fields').not.toMatch(/(?:^|\n)\s*icon\s*:/i);
}

type SubscriptionYamlProxy = {
  type: string;
  server: string;
  port: number;
};

function parseSubscriptionYamlProxies(yaml: string): SubscriptionYamlProxy[] {
  const proxies: SubscriptionYamlProxy[] = [];
  let inProxies = false;
  let current: JsonRecord | null = null;

  const pushCurrent = () => {
    if (!current) {
      return;
    }
    const server = stringValue(current.server).trim();
    const port = numberValue(current.port);
    if (server || port > 0 || current.type) {
      proxies.push({
        type: stringValue(current.type),
        server,
        port,
      });
    }
    current = null;
  };

  for (const line of yaml.split(/\r?\n/)) {
    if (inProxies && isTopLevelYamlMappingKey(line) && line.trim() !== 'proxies:') {
      pushCurrent();
      inProxies = false;
    }
    if (line === 'proxies:') {
      inProxies = true;
      continue;
    }
    if (!inProxies) {
      continue;
    }

    const itemMatch = line.match(/^-\s*(.*)$/);
    if (itemMatch) {
      pushCurrent();
      current = {};
      assignYamlScalar(current, itemMatch[1]);
      continue;
    }

    if (current) {
      assignYamlScalar(current, line.trim());
    }
  }
  pushCurrent();

  return proxies;
}

function isTopLevelYamlMappingKey(line: string) {
  return /^[A-Za-z0-9_-]+:\s*(?:$|.)/.test(line);
}

function assignYamlScalar(target: JsonRecord, source: string) {
  const trimmed = source.trim();
  if (trimmed.startsWith('{') && trimmed.endsWith('}')) {
    assignYamlFlowMapping(target, trimmed);
    return;
  }
  const match = source.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
  if (!match) {
    return;
  }
  target[match[1]] = unquoteYamlScalar(match[2]);
}

function assignYamlFlowMapping(target: JsonRecord, source: string) {
  for (const key of ['type', 'server', 'port']) {
    const match = source.match(new RegExp(`(?:^|,)\\s*${key}\\s*:\\s*("[^"]*"|'[^']*'|[^,}]+)`));
    if (match) {
      target[key] = unquoteYamlScalar(match[1]);
    }
  }
}

function unquoteYamlScalar(value: string) {
  const trimmed = value.trim();
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"'))
    || (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

function exitPoolStrategyLabel(strategy: string) {
  if (strategy === 'priority') {
    return '优先级';
  }
  return strategy;
}

function endpointConfigText(config: JsonRecord, keys: string[]) {
  for (const key of keys) {
    const value = config[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
    if (typeof value === 'number' || typeof value === 'boolean') {
      return String(value);
    }
  }
  return '';
}

function exitEndpointProtocolLabel(outboundType: string) {
  const normalized = outboundType.trim().toLowerCase();
  if (normalized === 'socks' || normalized === 'socks5') {
    return 'SOCKS5';
  }
  if (normalized === 'http') {
    return 'HTTP';
  }
  if (normalized === 'vless') {
    return 'VLESS';
  }
  if (normalized === 'trojan') {
    return 'Trojan';
  }
  if (normalized === 'shadowsocks') {
    return 'Shadowsocks';
  }
  if (normalized === 'hysteria' || normalized === 'hysteria2' || normalized === 'hy2') {
    return 'HY2';
  }
  if (normalized === 'direct') {
    return 'Direct';
  }
  return outboundType;
}
