// 用途：覆盖管理员用户管理页编辑保护和用户级流量日志入口。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员用户页可编辑用户并查看该用户流量日志', async ({ page }) => {
  await mockAdminSession(page);
  let adminPlansLoaded = false;
  let userDetailLoaded = false;
  let trafficLogsLoaded = false;
  const users = [
    {
      id: 1001,
      account: 'legacy@example.test',
      email: 'legacy@example.test',
      name: 'Legacy User',
      role: 'user',
      status: 'active',
      plan_id: 'plan-legacy',
      plan_name: '旧套餐',
      subscription_status: 'active',
      subscription_expires_at: '2026-06-01T00:00:00Z',
      traffic_gb: { used: 1, total: 10 },
      created_at: '2026-05-01T00:00:00Z',
    },
  ];
  await page.route('**/api/admin/plans', async (route) => {
    adminPlansLoaded = true;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: 'plan-standard',
            name: '标准套餐',
            is_default: false,
            enabled: true,
            traffic_limit_bytes: 107374182400,
            billing_multiplier: 1,
            price_cents: 990,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 10,
            line_groups: [],
          },
          {
            id: 'plan-legacy',
            name: '旧套餐',
            is_default: false,
            enabled: false,
            traffic_limit_bytes: 10737418240,
            billing_multiplier: 1,
            price_cents: 0,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 1,
            line_groups: [],
          },
        ],
      }),
    });
  });
  await page.route(/.*\/api\/admin\/users(?:\?.*)?$/, async (route) => {
    if (route.request().method() === 'POST') {
      const payload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
      const id = 1001 + users.length;
      users.push({
        id,
        account: String(payload.email),
        email: String(payload.email),
        name: String(payload.email),
        role: payload.is_admin ? 'admin' : 'user',
        status: payload.disabled ? 'disabled' : 'active',
        plan_id: String(payload.plan_id ?? ''),
        plan_name: payload.plan_id ? '标准套餐' : '基础套餐',
        subscription_status: 'active',
        subscription_expires_at: '2026-06-24T00:00:00Z',
        traffic_gb: { used: 0, total: 100 },
        created_at: '2026-05-25T00:00:00Z',
      });
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: users[users.length - 1] }),
      });
      return;
    }
    const url = new URL(route.request().url());
    const keyword = (url.searchParams.get('keyword') || '').toLowerCase();
    const status = url.searchParams.get('status') || '';
    const role = url.searchParams.get('role') || '';
    const page = Number(url.searchParams.get('page') || '1');
    const pageSize = Number(url.searchParams.get('page_size') || '20');
    const filtered = users.filter((user) => {
      const searchable = `${user.email} ${user.name} ${user.plan_name}`.toLowerCase();
      return (!keyword || searchable.includes(keyword))
        && (!status || user.status === status)
        && (!role || user.role === role);
    });
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: filtered.slice((page - 1) * pageSize, page * pageSize),
          total: filtered.length,
          page,
          page_size: pageSize,
        },
      }),
    });
  });
  await page.route('**/api/admin/users/batch-delete', async (route) => {
    const payload = JSON.parse(route.request().postData() ?? '{}') as { user_ids?: number[] };
    const ids = new Set(payload.user_ids ?? []);
    const deleted = users.filter((user) => ids.has(Number(user.id)));
    for (const user of deleted) {
      users.splice(users.findIndex((item) => item.id === user.id), 1);
    }
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          deleted_count: deleted.length,
          items: deleted.map((user) => ({
            deleted: true,
            id: user.id,
            email: user.email,
            deleted_usage_ledger_count: 0,
            deleted_snapshot_count: 0,
          })),
        },
      }),
    });
  });
  await page.route('**/api/admin/users/1001/traffic-logs**', async (route) => {
    trafficLogsLoaded = true;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [
            {
              id: 'traffic-log-1',
              user_id: 1001,
              xray_user_key: 'xray-key-legacy-1001',
              traffic_source: 'xray',
              delta_uplink: 2048,
              delta_downlink: 4096,
              delta_total: 6144,
              billed_uplink: 2048,
              billed_downlink: 4096,
              billed_bytes: 6144,
              billing_multiplier: 1,
              real_gb: 0.000006,
              billed_gb: 0.000006,
              collected_at: '2026-05-24T08:01:00Z',
              recorded_at: '2026-05-24T08:02:00Z',
              client_ip: '203.0.113.24',
              client_ip_hash: 'ip-hash-legacy',
              ip_observed_at: '2026-05-24T08:01:00Z',
              active_connection_count: 1,
              session_status: 'active',
              access_line: {
                id: 'line-entry-a',
                name: '入口-A',
                protocol: 'vless',
                transport: 'tcp',
                listen_host: '0.0.0.0',
                listen_port: 443,
              },
              access_node: {
                id: 'node-entry-a',
                name: '入口节点-A',
                public_host: 'edge-a.example.test',
              },
              exit_endpoint: {
                id: 'exit-us-a',
                name: '出口线路-US',
                outbound_type: 'direct',
                host: '198.51.100.8',
                port: 443,
                resource_name: '美国出口资源',
              },
            },
          ],
          total: 1,
          page: 1,
          page_size: 20,
        },
      }),
    });
  });
  await page.route('**/api/admin/users/1001', async (route) => {
    if (route.request().method() === 'PATCH') {
      const payload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
      const target = users.find((user) => user.id === 1001);
      if (target) {
        target.email = String(payload.email ?? target.email);
        target.account = target.email;
        target.name = target.email;
        target.status = payload.disabled ? 'disabled' : 'active';
        target.role = payload.is_admin ? 'admin' : 'user';
      }
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: target }),
      });
      return;
    }
    userDetailLoaded = true;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          id: 1001,
          account: 'legacy@example.test',
          email: 'legacy@example.test',
          role: 'user',
          status: 'active',
          plan_id: 'plan-legacy',
          plan_name: '旧套餐',
        },
      }),
    });
  });
  await page.route(/.*\/api\/admin\/users\/100[23]$/, async (route) => {
    const userId = Number(new URL(route.request().url()).pathname.split('/').pop());
    if (route.request().method() === 'DELETE') {
      const deletedUser = users.find((user) => user.id === userId);
      users.splice(users.findIndex((user) => user.id === userId), 1);
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: {
            deleted: true,
            id: userId,
            email: deletedUser?.email,
            deleted_usage_ledger_count: 0,
            deleted_snapshot_count: 0,
          },
        }),
      });
      return;
    }
    await route.fulfill({ status: 405 });
  });

  await page.goto('/admin/users');
  await expect(page.getByRole('heading', { name: '用户管理' })).toBeVisible();
  await expect(page.getByText('legacy@example.test').first()).toBeVisible();
  await expect(page.locator('.metric-card').filter({ hasText: '用户总数' }).locator('strong')).toHaveText('1');
  await expect.poll(() => adminPlansLoaded).toBe(true);

  await page.getByRole('button', { name: '新增用户' }).click();
  const createDialog = page.getByRole('dialog', { name: '新增用户' });
  await createDialog.getByPlaceholder('user@example.com').fill('created-by-admin@example.test');
  await createDialog.getByPlaceholder('至少 8 位').fill('created123');
  const createRequest = page.waitForRequest((request) => (
    new URL(request.url()).pathname === '/api/admin/users' && request.method() === 'POST'
  ));
  await createDialog.getByRole('button', { name: '保存' }).click();
  const createPayload = JSON.parse((await createRequest).postData() ?? '{}') as Record<string, unknown>;
  expect(createPayload).toMatchObject({
    email: 'created-by-admin@example.test',
    password: 'created123',
    disabled: false,
    is_admin: false,
  });
  await expect(createDialog).toBeHidden();
  await expect(page.getByText('created-by-admin@example.test').first()).toBeVisible();
  await expect(page.locator('.metric-card').filter({ hasText: '用户总数' }).locator('strong')).toHaveText('2');

  const searchRequest = page.waitForRequest((request) => {
    const url = new URL(request.url());
    return url.pathname === '/api/admin/users'
      && request.method() === 'GET'
      && url.searchParams.get('keyword') === 'created-by-admin';
  });
  await page.getByPlaceholder('邮箱、名称、套餐').fill('created-by-admin');
  await page.getByRole('button', { name: '查询' }).click();
  await searchRequest;
  await expect(page.getByText('created-by-admin@example.test').first()).toBeVisible();
  await expect(page.getByText('legacy@example.test')).toHaveCount(0);
  // 「重置」非精确会同时命中行内「重置密码 / 重置订阅 token」按钮，用 exact 锁定搜索区的清空筛选按钮。
  await page.getByRole('button', { name: '重置', exact: true }).click();
  await expect(page.getByText('legacy@example.test').first()).toBeVisible();

  await page.getByRole('button', { name: '新增用户' }).click();
  const singleDeleteDialog = page.getByRole('dialog', { name: '新增用户' });
  await singleDeleteDialog.getByPlaceholder('user@example.com').fill('single-delete@example.test');
  await singleDeleteDialog.getByPlaceholder('至少 8 位').fill('single123');
  await singleDeleteDialog.getByRole('button', { name: '保存' }).click();
  await expect(page.getByText('single-delete@example.test').first()).toBeVisible();
  const singleDeleteRequest = page.waitForRequest((request) => (
    new URL(request.url()).pathname === '/api/admin/users/1003' && request.method() === 'DELETE'
  ));
  await page.locator('tr', { hasText: 'single-delete@example.test' }).getByRole('button', { name: '更多用户操作' }).click();
  await page.getByRole('menuitem', { name: '删除用户', exact: true }).click();
  await page.getByRole('button', { name: '删除', exact: true }).last().click();
  await singleDeleteRequest;
  await expect(page.getByText('single-delete@example.test')).toHaveCount(0);

  await page.locator('tr', { hasText: 'legacy@example.test' }).getByRole('button', { name: '编辑用户' }).click();
  await expect.poll(() => userDetailLoaded).toBe(true);
  const dialog = page.getByRole('dialog', { name: /编辑用户/ });
  await expect(dialog.getByText('当前套餐 旧套餐 不在可选启用套餐中')).toBeVisible();
  await expect(dialog.getByText('未选择新套餐时，保存不会提交 plan_id，也不会变更用户当前套餐。')).toBeVisible();

  const updateRequest = page.waitForRequest((request) => (
    new URL(request.url()).pathname === '/api/admin/users/1001' && request.method() === 'PATCH'
  ));
  // 当前设计：编辑弹窗邮箱框只读（邮箱=账号身份/验证码锚点，创建后不可改），不再尝试改邮箱。
  // 仅验证只读并直接保存；PATCH payload 仍带原邮箱值，但不含 plan_id（未另选套餐）。
  await expect(dialog.getByPlaceholder('user@example.com')).toBeDisabled();
  await dialog.getByRole('button', { name: '保存' }).click();
  const payload = JSON.parse((await updateRequest).postData() ?? '{}') as Record<string, unknown>;
  expect(payload).not.toHaveProperty('plan_id');
  expect(payload).toMatchObject({
    email: 'legacy@example.test',
    disabled: false,
    is_admin: false,
  });
  await expect(page.getByText('legacy@example.test').first()).toBeVisible();

  await page.locator('tr', { hasText: 'legacy@example.test' }).getByRole('button', { name: '更多用户操作' }).click();
  await page.getByRole('menuitem', { name: '流量日志', exact: true }).click();
  await expect.poll(() => trafficLogsLoaded).toBe(true);
  const drawer = page.getByRole('dialog', { name: /用户流量日志：legacy@example.test/ });
  // 表格列可横向滚动，逐个滚动到字段后再验证用户级日志内容。
  for (const text of ['203.0.113.24', /入口-A/, /出口线路-US/, 'xray-key-legacy-1001']) {
    const cell = drawer.getByText(text).first();
    await cell.scrollIntoViewIfNeeded();
    await expect(cell).toBeVisible();
  }
  await page.keyboard.press('Escape');
  await expect(drawer).toBeHidden();

  const deleteRequest = page.waitForRequest((request) => (
    new URL(request.url()).pathname === '/api/admin/users/batch-delete' && request.method() === 'POST'
  ));
  await page.locator('tr', { hasText: 'created-by-admin@example.test' }).locator('.el-checkbox__inner').click();
  await page.getByRole('button', { name: '批量删除' }).click();
  await page.getByRole('button', { name: '批量删除', exact: true }).last().click();
  await deleteRequest;
  await expect(page.getByText('created-by-admin@example.test')).toHaveCount(0);
  await expect(page.locator('.metric-card').filter({ hasText: '用户总数' }).locator('strong')).toHaveText('1');
});
