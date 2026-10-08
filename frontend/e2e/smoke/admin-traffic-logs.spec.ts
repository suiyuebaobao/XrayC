// 用途：覆盖管理员后台独立流量日志页面和用户级明细展示。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员后台流量日志页按用户展示明细', async ({ page }) => {
  await mockAdminSession(page);
  let trafficLogsLoaded = false;

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
          items: [
            {
              id: 1001,
              account: 'legacy@example.test',
              email: 'legacy@example.test',
              name: 'Legacy User',
              role: 'user',
              status: 'active',
              plan_id: 'plan-standard',
              plan_name: '标准套餐',
              subscription_status: 'active',
              subscription_expires_at: '2026-06-01T00:00:00Z',
              traffic_gb: { used: 1, total: 10 },
              created_at: '2026-05-01T00:00:00Z',
            },
          ],
          total: 1,
          page: 1,
          page_size: 20,
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
              traffic_source: 'access_line',
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
              session_status: 'online',
              access_line: {
                id: 'line-entry-a',
                name: '入口-A',
                protocol: 'vless',
                transport: 'xudp',
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
                outbound_type: 'hy2',
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

  await page.goto('/admin/users');
  await page.getByRole('link', { name: '流量明细', exact: true }).click();
  await expect(page.getByRole('heading', { name: '用户流量日志' })).toBeVisible();
  await expect(page.getByText('legacy@example.test').first()).toBeVisible();
  await expect.poll(() => trafficLogsLoaded).toBe(true);
  for (const text of ['203.0.113.24', /入口-A/, /出口线路-US/, 'xray-key-legacy-1001']) {
    const cell = page.getByText(text).first();
    await cell.scrollIntoViewIfNeeded();
    await expect(cell).toBeVisible();
  }
});
