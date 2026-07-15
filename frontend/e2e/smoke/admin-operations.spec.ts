// 用途：覆盖运营中心主动探测和缺失运行态展示的 smoke 场景。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('运营中心主动探测按钮可触发且运行态缺失值不显示假 0', async ({ page }) => {
  await mockAdminSession(page);
  let probePayload: Record<string, unknown> | null = null;

  await page.route('**/api/admin/access-operations/summary', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          active_users: 2,
          active_access_lines: 1,
          healthy_exit_pools: 1,
          monthly_billed_traffic_gb: 0,
          config_dirty_nodes: 0,
          online_users: null,
          active_connections: null,
          unique_client_ips: null,
          runtime_metric_status: 'fresh',
          runtime_metric_line_count: 1,
          runtime_metric_expected_line_count: 1,
          access_node_count: 1,
          access_line_count: 1,
          exit_pool_count: 1,
          ledger_count: 0,
          generated_at: '2026-05-19T00:00:00Z',
          latest_metric_at: '2026-05-19T00:00:00Z',
          alert_summary: {
            danger_count: 1,
            warning_count: 0,
            info_count: 0,
            total_count: 1,
            generated_at: '2026-05-19T00:00:00Z',
          },
          alerts: [
            {
              id: 'access_node_offline:node-ops-1',
              kind: 'access_node_offline',
              severity: 'danger',
              resource_type: 'access_node',
              resource_id: 'node-ops-1',
              resource_name: '香港接入 01',
              title: '中转节点 香港接入 01 离线',
              message: '节点心跳超过 5 分钟未刷新',
              detected_at: '2026-05-19T00:00:00Z',
            },
          ],
          recent_events: [],
          settings: {
            probe_policy: {
              exit_auto_failover_enabled: true,
              exit_failure_threshold: 3,
              exit_recovery_threshold: 2,
              exit_window_minutes: 10,
            },
          },
        },
      }),
    });
  });
  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [
            {
              id: 'node-ops-1',
              name: '香港接入 01',
              public_host: 'access.example.test',
              config_dirty: false,
            },
          ],
          access_lines: [
            {
              id: 'line-ops-1',
              uuid: 'line-ops-1',
              name: '缺失运行态线路',
              region: 'HK',
              access_node: '香港接入 01',
              access_node_id: 'node-ops-1',
              line_group_name: '香港节点',
              listen_host: 'access.example.test',
              listen_port: 443,
              inbound_protocol: 'vless',
              status: 'enabled',
              exit_pool_name: '默认出口池',
              exit_pool_id: 'pool-ops-1',
              online_users: null,
              active_connections: null,
              unique_client_ips: null,
              uplink_rate_bps: null,
              downlink_rate_bps: null,
              latency_ms: null,
              probe_status: '',
            },
          ],
          exit_pools: [
            {
              id: 'pool-ops-1',
              uuid: 'pool-ops-1',
              name: '默认出口池',
              region: 'HK',
              status: 'healthy',
              healthy_members: 1,
              total_members: 1,
              strategy: 'priority',
              active_assignments: 0,
              members: [
                {
                  id: 'exit-endpoint-1',
                  resource_name: '香港出口 01',
                  outbound_type: 'socks',
                  host: 'secret-proxy.example.test',
                  port: 1080,
                  healthy: true,
                  status: 'healthy',
                },
              ],
            },
          ],
          line_groups: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/access-operations/probe', async (route) => {
    probePayload = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { queued: true } }),
    });
  });

  await page.goto('/admin/access-operations');
  await expect(page.getByRole('heading', { name: '监控中心' })).toBeVisible();
  await expect(page.locator('.metric-card').filter({ hasText: '在线用户' }).locator('strong')).toHaveText('未上报');
  await expect(page.locator('.metric-card').filter({ hasText: '活跃连接' }).locator('strong')).toHaveText('未上报');
  await expect(page.locator('.metric-card').filter({ hasText: '独立客户端 IP' }).locator('strong')).toHaveText('未上报');
  await expect(page.locator('.metric-card').filter({ hasText: '上行速率' }).locator('strong')).toHaveText('未上报');
  await expect(page.locator('.runtime-card').getByText('缺失运行态线路')).toBeVisible();
  await expect(page.getByText('生产告警')).toBeVisible();
  await expect(page.getByText('中转节点 香港接入 01 离线')).toBeVisible();
  await expect(page.locator('.runtime-card').getByText('延迟未上报')).toBeVisible();
  await expect(page.locator('.runtime-card')).not.toContainText(/(^|\s)0(\s|$)/);

  await expect(page.getByText('香港出口 01')).toBeVisible();
  await expect(page.getByText('secret-proxy.example.test')).toHaveCount(0);
  await page.getByRole('button', { name: '主动探测', exact: true }).click();
  await expect(page.getByText('已登记主动探测请求，等待节点运行组件拉取执行。')).toBeVisible();
  expect(probePayload).toEqual({ exit_endpoint_id: 'exit-endpoint-1' });
});
