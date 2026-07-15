// 用途：覆盖入口管理连接方式切换的前端 smoke 场景——IP 直连下 VLESS 安全模式 Reality/普通双选（无 TLS），
// 再切「CF 直连（橙云）」并提交 CDN 与 WS 字段。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员选 CF 直连连接方式创建 WebSocket 入口时提交 CDN 与 WS 字段', async ({ page }) => {
  await mockAdminSession(page);

  const createEntryPayloads: Array<Record<string, unknown>> = [];

  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [
            {
              id: 'node-cf-1',
              name: 'Cloudflare 接入节点',
              public_host: '203.0.113.10',
              public_port: 443,
              config_dirty: false,
              // 连接方式分段按钮按节点域名 kind 派生：有 direct 域名才出现「域名直连」、
              // 有 cf 域名才出现「CF 直连」。给节点配 direct + cf 各一个，CF 档位才可选。
              // 字段用 snake_case，前端 normalizeNodeDomain 兼容 snake/camel。
              domains: [
                {
                  id: 'domain-direct-1',
                  domain: 'direct.example.test',
                  kind: 'direct',
                  is_primary: true,
                  cert_status: 'valid',
                },
                {
                  id: 'domain-cf-1',
                  domain: 'cf-entry.example.test',
                  kind: 'cf',
                  is_primary: true,
                  cert_status: 'valid',
                },
              ],
            },
          ],
          access_lines: [],
          exit_pools: [],
          line_groups: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/access-entries', async (route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: { entries: [] } }),
      });
      return;
    }
    if (route.request().method() === 'POST') {
      createEntryPayloads.push(JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>);
      await route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: { id: `entry-cf-${createEntryPayloads.length}` } }),
      });
      return;
    }
    await route.fallback();
  });
  await page.route('**/api/admin/access-entry-exit-bindings', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { bindings: [] } }),
    });
  });
  await page.route('**/api/admin/access-entries/*/exit-bindings', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { id: 'binding-cf-ws' } }),
    });
  });
  await page.route('**/api/admin/exit-endpoints', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: [] }),
    });
  });

  await page.goto('/admin/access-entries');
  await expect(page.getByRole('heading', { name: '入口管理' })).toBeVisible();
  await page.getByRole('button', { name: '创建入口', exact: true }).click();

  const dialog = page.getByRole('dialog', { name: '创建入口' });
  await expect(dialog).toBeVisible();
  await dialog.locator('.el-form-item').filter({ hasText: '入口名称' }).locator('input').fill('Cloudflare WS 入口');

  // 选接入节点（唯一一个，含 direct + cf 域名）。
  await dialog.locator('.el-form-item').filter({ hasText: '接入节点' }).locator('.el-select').click();
  await page.locator('.el-select-dropdown__item').filter({ hasText: 'Cloudflare 接入节点' }).click();

  // 默认 IP 直连：协议只有 VLESS / Shadowsocks，没有 Trojan/HY2。
  const connectionModeItem = dialog.locator('.el-form-item').filter({ hasText: '连接方式' });
  await expect(connectionModeItem.locator('.el-radio-button.is-active')).toContainText('IP 直连');
  const protocolItem = dialog.locator('.el-form-item').filter({ hasText: '入口/伪装协议' });
  await protocolItem.locator('.el-select').click();
  await expect(page.getByRole('option', { name: 'VLESS', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'Shadowsocks', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'Trojan', exact: true })).toHaveCount(0);
  // IP 直连 VLESS 安全模式：Reality（抗封伪装）与「普通 VLESS」（裸连免证书，解锁 UDP/XUDP/WS）两选；
  // IP 无证书故不提供 TLS。普通模式是在 IP 直连上拿到 UDP/XUDP 的唯一正路（Reality 仅 TCP/XHTTP/gRPC）。
  await page.getByRole('option', { name: 'VLESS', exact: true }).click();
  const securityItem = dialog.locator('.el-form-item').filter({ hasText: '安全模式' });
  await securityItem.locator('.el-select').click();
  await expect(page.getByRole('option', { name: 'Reality', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: '普通 VLESS', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'TLS', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');

  // SNI 合并（2026-06-28）：前端不再有独立「Server Name」字段——TLS 的 SNI 自动=选用域名、none 无 SNI。
  // IP 直连 + VLESS + Reality（默认安全模式）下，伪装大站走「伪装域名」分类下拉（Reality dest），默认 www.cloudflare.com。
  await expect(
    dialog.locator('.el-form-item').filter({ hasText: 'Server Name' }),
  ).toHaveCount(0);
  // 伪装域名是 el-select（filterable + allow-create），选中值用 toContainText 读（同安全模式/CDN 服务商口径）。
  await expect(
    dialog.locator('.el-form-item').filter({ hasText: '伪装域名' }).locator('.el-select'),
  ).toContainText('www.cloudflare.com');

  // 切到「CF 直连（橙云）」连接方式：分段按钮（el-radio-button），自动开 CDN、协议收敛到 VLESS/Trojan。
  await connectionModeItem.locator('.el-radio-button').filter({ hasText: 'CF 直连' }).click();
  await expect(connectionModeItem.locator('.el-radio-button.is-active')).toContainText('CF 直连');

  // CF 直连协议集：仅 VLESS / Trojan；HY2 / Shadowsocks / AnyTLS 不可选。
  await protocolItem.locator('.el-select').click();
  await expect(page.getByRole('option', { name: 'VLESS', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'Trojan', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'HY2', exact: true })).toHaveCount(0);
  await expect(page.getByRole('option', { name: 'Shadowsocks', exact: true })).toHaveCount(0);
  await expect(page.getByRole('option', { name: 'AnyTLS', exact: true })).toHaveCount(0);
  await page.getByRole('option', { name: 'VLESS', exact: true }).click();

  // CF 直连下安全模式固定 TLS（VLESS 经 CF 回源），el-select 选中值用 toContainText 读。
  await expect(
    dialog.locator('.el-form-item').filter({ hasText: '安全模式' }).locator('.el-select'),
  ).toContainText('TLS');

  // 监听端口被收敛到 CF 支持的 HTTPS 端口（节点 public_port 443 命中，保持 443）。
  await expect(
    dialog.locator('.el-form-item').filter({ hasText: '监听端口' }).locator('.el-select'),
  ).toContainText('443');

  // 网络模式已拆为「传输（可多选）」多选 + 「承载（TCP/UDP）」（仅 RAW 传输出现承载）。
  // CF 只放行 WS / gRPC / XHTTP（无 RAW/TCP/UDP/XUDP，故无承载下拉）；明确归一为仅 WebSocket。
  const networkItem = dialog.locator('.el-form-item').filter({ hasText: '传输（可多选）' });
  await networkItem.locator('.el-select').click();
  await expect(page.getByRole('option', { name: 'WebSocket', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'gRPC', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'XHTTP', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'TCP', exact: true })).toHaveCount(0);
  await expect(page.getByRole('option', { name: 'XUDP', exact: true })).toHaveCount(0);
  // 多选默认可能选中 XHTTP；逐项归一为仅 WebSocket（未选则选上 WS，已选的 gRPC/XHTTP 取消），
  // 确保只剩 WS 一条 => 仅提交一条入口、transport=ws。
  const wsOption = page.getByRole('option', { name: 'WebSocket', exact: true });
  if ((await wsOption.getAttribute('aria-selected')) !== 'true') {
    await wsOption.click();
  }
  for (const optionName of ['gRPC', 'XHTTP']) {
    const option = page.getByRole('option', { name: optionName, exact: true });
    if ((await option.getAttribute('aria-selected')) === 'true') {
      await option.click();
    }
  }
  await expect(networkItem.locator('.el-select')).toContainText('WebSocket');
  await page.keyboard.press('Escape');

  // CF 直连自动出现 CDN 区块，服务商自动填 Cloudflare（无手动「启用 CDN」开关）。
  const cdnProviderItem = dialog.locator('.el-form-item').filter({ hasText: 'CDN 服务商' });
  await expect(cdnProviderItem).toBeVisible();
  await expect(cdnProviderItem.locator('.el-select')).toContainText('Cloudflare');

  // 填 WS 路径与 CDN Hostname。
  await dialog.locator('.el-form-item').filter({ hasText: '路径' }).locator('input').fill('/vless-ws');
  await dialog.locator('.el-form-item').filter({ hasText: 'CDN Hostname' }).locator('input').fill('cdn-entry.example.test');

  await dialog.getByRole('button', { name: '保存', exact: true }).click();

  await expect(dialog).toBeHidden();
  // CF + 单网络模式（WS）=> 仅提交一条入口 payload。
  expect(createEntryPayloads).toHaveLength(1);
  expect(createEntryPayloads[0]).toMatchObject({
    protocol: 'vless',
    transport: 'ws',
    security: 'tls',
    listen_port: 443,
    cdn_enabled: true,
    cdn_provider: 'cloudflare',
    cdn_hostname: 'cdn-entry.example.test',
    ws_path: '/vless-ws',
    // CF 直连选中节点 cf 域名，node_domain_id 带出该域名 id。
    node_domain_id: 'domain-cf-1',
  });
});
