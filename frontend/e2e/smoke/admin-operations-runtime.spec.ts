// 用途：覆盖管理员运营中心运行态卡片和真实指标缺失/新鲜/过期状态。
import { expect, type Route, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员运营中心可区分未上报、实时和旧快照状态', async ({ page }) => {
  test.setTimeout(60_000);

  await mockAdminSession(page);
  let runtimeMode: 'fresh' | 'stale' | 'no_data' = 'no_data';
  const collectedAt = '2026-05-19T00:00:00Z';
  const summaryRoute = async (route: Route) => {
    const hasRuntime = runtimeMode !== 'no_data';
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
          online_users: hasRuntime ? 5 : null,
          active_connections: hasRuntime ? 7 : null,
          unique_client_ips: hasRuntime ? 3 : null,
          runtime_metric_status: runtimeMode,
          runtime_metric_line_count: hasRuntime ? 1 : 0,
          runtime_metric_expected_line_count: 1,
          access_node_count: 1,
          access_line_count: 1,
          exit_pool_count: 1,
          ledger_count: 0,
          generated_at: collectedAt,
          latest_metric_at: hasRuntime ? collectedAt : null,
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
  };
  const routingRoute = async (route: Route) => {
    const hasRuntime = runtimeMode !== 'no_data';
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [{ id: 'node-ops-1', name: '香港接入 01', public_host: 'access.example.test', config_dirty: false }],
          access_lines: [
            {
              id: 'line-ops-1',
              uuid: 'line-ops-1',
              name: '香港运营线',
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
              ...(hasRuntime
                ? {
                    online_users: 5,
                    active_connections: 7,
                    unique_client_ips: 3,
                    uplink_rate_bps: 500,
                    downlink_rate_bps: 800,
                    metric_collected_at: collectedAt,
                    latency_ms: runtimeMode === 'stale' ? 120 : 32,
                    probe_status: runtimeMode === 'stale' ? 'unhealthy' : 'healthy',
                    last_probe_at: collectedAt,
                    probe_error_summary: runtimeMode === 'stale' ? 'TCP 拨测失败' : '',
                  }
                : {}),
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
              members: [],
            },
          ],
          line_groups: [
            {
              id: 'demo-line-group',
              name: '香港节点',
              country_code: 'HK',
              exit_endpoint_ids: ['exit-ops-1'],
            },
          ],
        },
      }),
    });
  };
  await page.route('**/api/admin/access-operations/summary', summaryRoute);
  await page.route('**/api/admin/access-operations/ledger-ranking?*', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          source: 'usage_ledgers+usage_daily_rollups',
          limit: 10,
          generated_at: collectedAt,
          totals: {
            access_line_count: 1,
            ledger_count: 0,
            delta_uplink: 0,
            delta_downlink: 0,
            real_bytes: 0,
            billed_uplink: 0,
            billed_downlink: 0,
            billed_bytes: 0,
          },
          items: [],
        },
      }),
    });
  });
  await page.route('**/api/admin/access-routing', routingRoute);

  runtimeMode = 'no_data';
  await page.goto('/admin/access-operations');
  await expect(page.getByRole('heading', { name: '监控中心' })).toBeVisible();
  await expect(page.getByText('中转运营态')).toBeVisible();
  await expect(page.getByText('独立客户端 IP')).toBeVisible();
  await expect(page.getByText('中转入口运行态')).toBeVisible();
  await expect(page.getByText('上行速率').first()).toBeVisible();
  await expect(page.getByText('下行速率').first()).toBeVisible();
  await expect(page.getByText('待同步节点数')).toBeVisible();
  await expect(page.getByText('探测故障转移策略')).toBeVisible();
  await expect(page.getByText('连续失败阈值')).toBeVisible();
  await expect(page.getByText('连续恢复阈值')).toBeVisible();
  await expect(page.getByText('不是 0，而是未上报')).toBeVisible();
  await expect(page.getByText('no_data · 未上报')).toBeVisible();
  await expect(page.locator('.metric-card').filter({ hasText: '在线用户' }).locator('strong')).toHaveText('未上报');
  await expect(page.locator('.metric-card').filter({ hasText: '活跃连接' }).locator('strong')).toHaveText('未上报');
  await expect(page.locator('.metric-card').filter({ hasText: '上行速率' }).locator('strong')).toHaveText('未上报');

  runtimeMode = 'fresh';
  await page.goto('/admin/access-operations');
  await expect(page.getByText('fresh · 实时')).toBeVisible();
  await expect(page.locator('.metric-card').filter({ hasText: '在线用户' }).locator('strong')).toHaveText('5');
  await expect(page.locator('.metric-card').filter({ hasText: '活跃连接' }).locator('strong')).toHaveText('7');
  await expect(page.locator('.metric-card').filter({ hasText: '独立客户端 IP' }).locator('strong')).toHaveText('3');
  await expect(page.locator('.metric-card').filter({ hasText: '上行速率' }).locator('strong')).toHaveText('500 bps');
  await expect(page.locator('.metric-card').filter({ hasText: '下行速率' }).locator('strong')).toHaveText('800 bps');
  await expect(page.locator('.runtime-card').getByText('健康')).toBeVisible();
  await expect(page.locator('.runtime-card').getByText('32 ms')).toBeVisible();

  runtimeMode = 'stale';
  await page.goto('/admin/access-operations');
  await expect(page.locator('.runtime-card').getByText('stale · 旧快照')).toBeVisible();
  await expect(page.locator('.runtime-card').getByText('异常')).toBeVisible();
  await expect(page.locator('.runtime-card').getByText('TCP 拨测失败')).toBeVisible();
});
