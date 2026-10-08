// 用途：覆盖管理员用户管理页的设备/IP 观察抽屉。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员查看用户设备管理中的订阅拉取和节点使用 IP', async ({ page }) => {
  await mockAdminSession(page);
  let devicesLoaded = false;

  await page.route('**/api/admin/plans', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: [] }),
    });
  });
  await page.route(/.*\/api\/admin\/users(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [{
            id: 1001,
            account: 'device-user@example.test',
            email: 'device-user@example.test',
            name: 'Device User',
            role: 'user',
            status: 'active',
            plan_name: '基础套餐',
            subscription_status: 'active',
            subscription_expires_at: '2026-06-30T00:00:00Z',
            traffic_gb: { used: 1, total: 10 },
            created_at: '2026-06-01T00:00:00Z',
          }],
          total: 1,
          page: 1,
          page_size: 20,
        },
      }),
    });
  });
  await page.route('**/api/admin/users/1001/devices**', async (route) => {
    devicesLoaded = true;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [
            {
              client_ip: '203.0.113.8',
              client_ip_hash: 'sha256:ddbea5471056690e5b1dcfe0c39ffca2f91ee81710048d066e2d53c7d012e14e',
              source: 'both',
              first_seen_at: '2026-06-11T00:01:00Z',
              last_seen_at: '2026-06-11T00:03:00Z',
              subscription_pull_count: 2,
              node_use_count: 3,
              last_access_line: {
                id: 'line-1',
                name: '入口-A',
                protocol: 'vless',
                transport: 'tcp',
              },
              last_access_node: {
                id: 'node-1',
                name: '中转-A',
                public_host: 'edge-a.example.test',
              },
              last_session_status: 'online',
              last_active_connection_count: 2,
            },
          ],
          total: 1,
          page: 1,
          page_size: 20,
        },
      }),
    });
  });

  await page.goto('/admin/users');
  await expect(page.getByRole('heading', { name: '用户管理' })).toBeVisible();
  await page.locator('tr', { hasText: 'device-user@example.test' }).getByRole('button', { name: '更多用户操作' }).click();
  await page.getByRole('menuitem', { name: '访问 IP' }).click();

  await expect.poll(() => devicesLoaded).toBe(true);
  const drawer = page.getByRole('dialog', { name: /设备管理：device-user@example.test/ });
  await expect(drawer.getByText('203.0.113.8')).toBeVisible();
  await expect(drawer.getByText('订阅拉取 + 节点使用')).toBeVisible();
  await expect(drawer.getByText('入口-A / vless / tcp')).toBeVisible();
  await expect(drawer.getByText('中转-A / edge-a.example.test')).toBeVisible();
  await expect(drawer.getByText('online / 连接 2')).toBeVisible();
});
