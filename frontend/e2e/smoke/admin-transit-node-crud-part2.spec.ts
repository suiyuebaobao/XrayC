// 用途：中转节点 CRUD smoke 的第二部分——编辑节点、启用本机出口服务并批量删除。
// 仅为控制单文件行数（≤550）从 admin-transit-node-crud.spec.ts 原样拆出，用例内容不变。
// 本用例使用 Playwright 路由 mock，只验证前端动作和 API 契约。
// 真实链路仍由脚本和远端服务器 E2E 覆盖。

import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员可以编辑中转节点、启用本机出口服务并批量删除', async ({ page }) => {
  await mockAdminSession(page);

  const node = {
    id: 'node-relay-1',
    name: '香港中转 01',
    public_host: 'hk-relay.example.test',
    ssh_host: 'ssh-relay-a.example.test',
    // 编辑保存要求节点至少有一个对外直连地址（IP/域名/CF），否则前端校验拦截不关闭弹窗；
    // 这里给一个 IP 直连地址满足校验，编辑流程才能正常保存关闭。
    ip_direct_address: '192.0.2.20',
    // 节点带一个直连域名（灰云）：本机出口服务里 HY2/Trojan/VLESS-TLS 需要把线路连接方式切到
    // 「域名直连」并选中该域名才能用（与后端按选中域名 kind 判定护栏同口径）；域名与编辑后的
    // 客户连接地址一致，证书路径据此默认推导。
    domains: [
      { id: 'domain-direct-1', domain: 'customer-relay.example.test', kind: 'direct', is_primary: true },
    ],
    // 节点带域名就要签证书，编辑保存校验要求 ACME 邮箱非空，否则弹窗不关闭。
    acme_email: 'ops@example.test',
    status: 'healthy',
    config_dirty: false,
  };
  const state = {
    nodes: [node],
    lines: [] as Array<Record<string, unknown>>,
  };
  let updatePayload: Record<string, unknown> | null = null;
  let localExitPayload: Record<string, unknown> | null = null;
  let batchDeletePayload: Record<string, unknown> | null = null;

  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: state.nodes,
          access_lines: state.lines,
          exit_pools: [],
          line_groups: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/deployment-tasks', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { items: [] } }),
    });
  });
  await page.route('**/api/admin/exit-endpoints', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: [] }),
    });
  });
  await page.route('**/api/admin/access-nodes/node-relay-1', async (route) => {
    if (route.request().method() === 'PUT') {
      updatePayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
      node.name = String(updatePayload.name ?? node.name);
      node.public_host = String(updatePayload.public_host ?? node.public_host);
      node.ssh_host = String(updatePayload.ssh_host ?? node.ssh_host);
      await route.fulfill({ contentType: 'application/json', body: JSON.stringify({ success: true }) });
      return;
    }
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          deleted_node_ids: ['node-relay-1'],
          deleted_node_count: 1,
          deleted_access_line_count: 1,
          deleted_local_resource_count: 1,
          deleted_local_pool_count: 1,
        },
      }),
    });
  });
  await page.route('**/api/admin/access-nodes/node-relay-1/local-exit-lines', async (route) => {
    localExitPayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_node_id: 'node-relay-1',
          access_node_name: node.name,
          created_count: 4,
          created_lines: [
            { exit_resource_id: 'res-local-1', exit_endpoint_id: 'ep-local-1', outbound_type: 'vless', network_mode: 'tcp' },
          ],
        },
      }),
    });
  });
  await page.route('**/api/admin/access-nodes/batch-delete', async (route) => {
    batchDeletePayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
    state.nodes = [];
    state.lines = [];
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          deleted_node_ids: ['node-relay-1'],
          deleted_node_count: 1,
          deleted_access_line_count: 1,
          deleted_local_resource_count: 1,
          deleted_local_pool_count: 1,
        },
      }),
    });
  });

  await page.goto('/admin/transit-nodes');
  await page.getByRole('button', { name: '编辑' }).click();
  const editDialog = page.getByRole('dialog', { name: '编辑中转节点' });
  await editDialog.locator('.el-form-item').filter({ hasText: '节点名称' }).locator('input').fill('香港中转主节点');
  // 三地址自动推导：弹窗已无独立「客户连接地址」字段，public_host 由 IP直连>域名直连>CF 推导。
  // 本节点 ip_direct_address=192.0.2.20，故 public_host 推导为该 IP（不再手填客户连接地址）。
  await expect(editDialog.locator('.el-form-item').filter({ hasText: '客户连接地址' })).toHaveCount(0);
  await editDialog.locator('.el-form-item').filter({ hasText: 'SSH IP' }).locator('input').fill('ssh-relay-b.example.test');
  await editDialog.getByRole('button', { name: '保存' }).click();
  await expect(editDialog).toBeHidden();
  expect(updatePayload).toMatchObject({
    name: '香港中转主节点',
    public_host: '192.0.2.20',
    ssh_host: 'ssh-relay-b.example.test',
  });

  await page.getByRole('button', { name: '本机出口服务' }).click();
  const localExitDialog = page.getByRole('dialog', { name: '本机出口服务' });
  await expect(localExitDialog).toBeVisible();
  await expect(localExitDialog.getByText(/仅批量添加本机创建的出口线路到出口管理/)).toBeVisible();

  // 默认预填两条「免证书」线路：VLESS(Reality) + Shadowsocks，均为 IP 直连（不需要节点域名）。
  // HY2/Trojan 要证书，必须把线路连接方式切到「域名直连」并选中节点直连域名才能使用。
  // 这里再补两行，分别切到「域名直连」后选 HY2 / Trojan，凑齐 VLESS/HY2/Shadowsocks/Trojan 四协议。
  const lineRows = localExitDialog.locator('.line-row');
  await expect(lineRows).toHaveCount(2);
  await localExitDialog.getByRole('button', { name: '添加线路' }).click();
  await localExitDialog.getByRole('button', { name: '添加线路' }).click();
  await expect(lineRows).toHaveCount(4);

  // 第 3 行（索引 2）：切到「域名直连（灰云）」（自动选中主直连域名），协议选 HY2。
  await lineRows.nth(2).locator('.el-radio-button').filter({ hasText: '域名直连' }).click();
  await lineRows.nth(2).locator('.el-form-item').filter({ hasText: '线路协议' }).locator('.el-select').click();
  await page.getByRole('option', { name: 'HY2', exact: true }).click();
  // 第 4 行（索引 3）：切到「域名直连（灰云）」，协议选 Trojan。
  await lineRows.nth(3).locator('.el-radio-button').filter({ hasText: '域名直连' }).click();
  const trojanProtocolSelect = lineRows.nth(3).locator('.el-form-item').filter({ hasText: '线路协议' }).locator('.el-select');
  await trojanProtocolSelect.click();
  const trojanDropdownId = await trojanProtocolSelect.getByRole('combobox').getAttribute('aria-controls');
  // 鎖定本列的選單，避免前一列選單的離場動畫造成重複選項。
  await page.locator(`[id="${trojanDropdownId}"]`).getByRole('option', { name: 'Trojan', exact: true }).click();

  // 四种协议标签都应可见；本机出口不展示 SOCKS5/HTTP（需在 IP 直连下另选），也没有原始 JSON 字段。
  await expect(localExitDialog.getByText('VLESS', { exact: true }).first()).toBeVisible();
  await expect(localExitDialog.getByText('HY2', { exact: true }).first()).toBeVisible();
  await expect(localExitDialog.getByText('Shadowsocks', { exact: true }).first()).toBeVisible();
  await expect(localExitDialog.getByText('Trojan', { exact: true }).first()).toBeVisible();
  await expect(localExitDialog.getByText('连接配置 JSON')).toHaveCount(0);
  await expect(localExitDialog.getByText('Stream 配置 JSON')).toHaveCount(0);
  await expect(localExitDialog.getByText('探测配置 JSON')).toHaveCount(0);

  // 地址字段改名「本机出口服务地址」；默认按连接方式带出：IP 直连两行取节点 ip_direct_address(192.0.2.20)，
  // 后补的域名直连两行(HY2/Trojan)取所选直连域名(customer-relay.example.test)。
  const hostFields = localExitDialog.locator('.el-form-item').filter({ hasText: '本机出口服务地址' }).locator('input');
  await expect(hostFields).toHaveCount(4);
  await expect(hostFields.nth(0)).toHaveValue('192.0.2.20');
  await expect(hostFields.nth(1)).toHaveValue('192.0.2.20');
  await expect(hostFields.nth(2)).toHaveValue('customer-relay.example.test');
  await expect(hostFields.nth(3)).toHaveValue('customer-relay.example.test');

  // 网络模式已拆为「传输（可多选）」+「承载（TCP/UDP）」（仅选中 RAW 传输才出现承载）。
  // VLESS 行（索引 0）：改地址；Reality 传输只放 RAW（TCP）、承载只「仅 TCP」（UDP/XUDP 全锁、无 XHTTP）。
  await hostFields.nth(0).fill('local-vless.example.test');
  const row0Transport = lineRows.nth(0).locator('.el-form-item').filter({ hasText: '传输（可多选）' }).locator('.el-select');
  await row0Transport.click();
  await expect(page.getByRole('option', { name: 'RAW（TCP）', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'XHTTP', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  const row0Carriage = lineRows.nth(0).locator('.el-form-item').filter({ hasText: '承载（TCP/UDP）' }).locator('.el-select');
  await row0Carriage.click();
  await expect(page.getByRole('option', { name: '仅 TCP', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'TCP + UDP', exact: true })).toHaveCount(0);
  await expect(page.getByRole('option', { name: 'TCP + UDP + XUDP', exact: true })).toHaveCount(0);
  await page.keyboard.press('Escape');
  // 切到「普通 VLESS」(security=none)：承载解锁 TCP + UDP / TCP + UDP + XUDP（IP 直连上拿 UDP/XUDP 的正路）。
  // 该行是 IP 直连（免证书），安全模式只放开 普通 VLESS / Reality；TLS 需要域名直连，故不出现。
  await lineRows.nth(0).locator('.el-form-item').filter({ hasText: 'VLESS 安全模式' }).locator('.el-select').click();
  await expect(page.getByRole('option', { name: '普通 VLESS', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'Reality', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'TLS', exact: true })).toHaveCount(0);
  await page.getByRole('option', { name: '普通 VLESS', exact: true }).click();
  await row0Carriage.click();
  await expect(page.getByRole('option', { name: 'TCP + UDP', exact: true })).toBeVisible();
  await expect(page.getByRole('option', { name: 'TCP + UDP + XUDP', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');

  const localExitResponse = page.waitForResponse((response) =>
    response.url().includes('/local-exit-lines') && response.request().method() === 'POST',
  );
  await localExitDialog.getByRole('button', { name: '添加到出口管理' }).click();
  await localExitResponse;
  // 提交按线路顺序展开：默认两行 VLESS/Shadowsocks 在前，后补的 HY2/Trojan 在后。
  const linesPayload = Array.isArray(localExitPayload?.lines) ? localExitPayload.lines : [];
  expect(linesPayload).toHaveLength(4);
  expect(linesPayload[0]).toMatchObject({
    outbound_type: 'vless',
    network_mode: 'tcp',
    host: 'local-vless.example.test',
  });
  expect(linesPayload[0].outbound_config).toMatchObject({
    security: 'none',
  });
  expect(linesPayload[0].stream_config).toMatchObject({
    network_mode: 'tcp',
    network: 'tcp',
  });
  expect(linesPayload[0].probe_config).toMatchObject({});
  // SS 行默认 IP 直连，host 取节点 ip_direct_address(192.0.2.20)；HY2/Trojan 切域名直连后才取直连域名。
  expect(linesPayload[1]).toMatchObject({ outbound_type: 'shadowsocks', network_mode: 'tcp', host: '192.0.2.20' });
  expect(String(linesPayload[1].outbound_config?.method)).toBe('2022-blake3-aes-128-gcm');
  expect(linesPayload[2]).toMatchObject({ outbound_type: 'hysteria', network_mode: 'udp', host: 'customer-relay.example.test' });
  expect(linesPayload[2].outbound_config).toMatchObject({
    server_name: 'customer-relay.example.test',
    certificate_file: '/etc/letsencrypt/live/customer-relay.example.test/fullchain.pem',
    key_file: '/etc/letsencrypt/live/customer-relay.example.test/privkey.pem',
  });
  expect(linesPayload[3]).toMatchObject({ outbound_type: 'trojan', network_mode: 'tcp', host: 'customer-relay.example.test' });
  expect(linesPayload[3].outbound_config).toMatchObject({
    security: 'tls',
    server_name: 'customer-relay.example.test',
    certificate_file: '/etc/letsencrypt/live/customer-relay.example.test/fullchain.pem',
    key_file: '/etc/letsencrypt/live/customer-relay.example.test/privkey.pem',
  });
  await expect(localExitDialog).toBeHidden();
  await expect(page.getByText('hk-relay.example.test:1443')).toHaveCount(0);

  await page.getByTestId('relay-node-select').click();
  const batchDeleteButton = page.getByRole('button', { name: '批量删除节点' });
  await expect(batchDeleteButton).toBeEnabled();
  await batchDeleteButton.click();
  const deleteDialog = page.getByRole('dialog', { name: '删除中转节点' });
  await expect(deleteDialog).toBeVisible();
  await Promise.all([
    page.waitForRequest((request) =>
      request.url().includes('/api/admin/access-nodes/batch-delete') && request.method() === 'POST',
    ),
    deleteDialog.getByRole('button', { name: '确认' }).click(),
  ]);
  expect(batchDeletePayload).toMatchObject({ access_node_ids: ['node-relay-1'] });
  await expect(page.getByText('还没有中转节点')).toBeVisible();
});
