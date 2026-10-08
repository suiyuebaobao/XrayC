// 新工作区关键交互：按需诊断、用户入口开关、两个出口入口共用同一写接口与字段范围。
import { expect, test } from '@playwright/test';
import { mockAdminSession, mockUserSession } from '../helpers/smoke';

test('监控默认不拉取诊断明细，展开后才请求', async ({ page }) => {
  await mockAdminSession(page);
  let details = 0; let rankings = 0;
  await page.route('**/api/admin/access-operations/summary', (route) => route.fulfill({ json: { success: true, data: { runtime_metric_status: 'no_data' } } }));
  await page.route('**/api/admin/access-routing', (route) => {
    details += 1;
    return route.fulfill({ json: { success: true, data: { access_nodes: [], access_lines: [], exit_pools: [], line_groups: [] } } });
  });
  await page.route('**/api/admin/access-operations/ledger-ranking**', (route) => {
    rankings += 1;
    return route.fulfill({ json: { success: true, data: { items: [] } } });
  });
  await page.goto('/admin/access-operations');
  await expect(page.getByText('展开诊断明细（10 分钟）', { exact: true })).toBeVisible();
  expect(details).toBe(0); expect(rankings).toBe(0);
  await page.getByText('展开诊断明细（10 分钟）', { exact: true }).click();
  await expect.poll(() => details).toBe(1);
  await expect.poll(() => rankings).toBe(1);
  await expect(page.getByText('中转入口运行态', { exact: true })).toBeVisible();
});

test('用户服务页合并订阅并尊重营运入口开关，旧网址仍可访问', async ({ page }) => {
  await mockUserSession(page);
  await page.route('**/api/sales-landing', (route) => route.fulfill({ json: { success: true, data: { portal_features: { plans: false, orders: false, redeem: false, invites: false, marketing: false } } } }));
  await page.route('**/api/user/subscription', (route) => route.fulfill({ json: { success: true, data: { plan_name: '保留套餐', subscription_url: '/sub/fixture-only', traffic: { used: 1, total: 10 }, access_lines: [] } } }));
  await page.route('**/api/user/usage', (route) => route.fulfill({ json: { success: true, data: { real_bytes: 1024, billed_bytes: 2048 } } }));
  let ordersRequested = false;
  await page.route('**/api/orders', (route) => { ordersRequested = true; return route.fulfill({ json: { success: true, data: [] } }); });
  await page.goto('/dashboard');
  await expect(page.getByRole('heading', { name: '我的服务', exact: true })).toBeVisible();
  await expect(page.getByText('保留套餐', { exact: true })).toBeVisible();
  const menu = page.getByRole('complementary', { name: '工作区导航' });
  await expect(menu.getByRole('link', { name: '我的服务', exact: true })).toBeVisible();
  await expect(menu.getByRole('link', { name: '账户安全', exact: true })).toBeVisible();
  for (const name of ['套餐', '我的订单', '兑换码', '邀请码']) await expect(menu.getByRole('link', { name, exact: true })).toHaveCount(0);
  expect(ordersRequested).toBe(false);
  await page.goto('/subscription');
  await expect(page.getByRole('heading', { name: '我的服务', exact: true })).toBeVisible();
  await expect(page.getByText('订阅链接', { exact: true })).toBeVisible();
});

test('全域出口编辑本机服务时复用节点写接口且不覆盖未修改凭据', async ({ page }) => {
  await mockAdminSession(page);
  const endpoint = { id: 'exit-1', exit_resource_id: 'resource-1', name: '共享出口', resource_name: '共享出口', host: '127.0.0.1', port: 41000, enabled: true, exit_resource_enabled: true, outbound_type: 'socks', outbound_config: { username: 'fixture-user', password: 'test-shared-exit-secret' }, stream_config: {}, probe_config: {} };
  await page.route('**/api/admin/exit-resources', (route) => route.fulfill({ json: { success: true, data: { exit_resources: [{ id: 'resource-1', name: '共享出口', ownership: 'self_hosted', access_node_id: 'node-1', enabled: true }] } } }));
  await page.route('**/api/admin/exit-endpoints', (route) => route.fulfill({ json: { success: true, data: { exit_endpoints: [endpoint] } } }));
  await page.route('**/api/admin/access-routing', (route) => route.fulfill({ json: { success: true, data: { access_nodes: [{ id: 'node-1', name: '测试节点', domains: [] }], access_lines: [], exit_pools: [], line_groups: [] } } }));
  await page.route('**/api/admin/access-nodes/node-1/local-exit-lines', (route) => route.fulfill({ json: { success: true, data: { lines: [{ exit_endpoint_id: 'exit-1', endpoint_name: '共享出口', host: '127.0.0.1', port: 41000, outbound_type: 'socks', enabled: true }] } } }));
  let sent: unknown;
  await page.route('**/api/admin/access-nodes/node-1/local-exit-lines/exit-1', async (route) => {
    sent = route.request().postDataJSON();
    await route.fulfill({ json: { success: true, data: {} } });
  });
  await page.goto('/admin/line-pool');
  await page.getByRole('button', { name: '查看/编辑', exact: true }).first().click();
  const dialog = page.getByRole('dialog', { name: '编辑本机出口线路', exact: true });
  await expect(dialog.getByLabel('端口', { exact: true })).toBeVisible();
  await dialog.getByLabel('端口', { exact: true }).fill('41001');
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog).toBeHidden();
  expect(sent).toEqual({ port: 41001 });
});
