// 用途：使用脱敏演示数据生成公开 README 截图，禁止连接生产 API。
import { expect, test, type Page } from '@playwright/test';
import path from 'node:path';

import { mockAdminSession, mockUserSession } from './helpers/smoke';

const outputDir = path.resolve(process.cwd(), '..', 'screenshots');

test.use({ viewport: { width: 1440, height: 960 }, colorScheme: 'light' });

test.beforeEach(async ({ page }) => {
  // 所有未显式设置的 API 也使用示例响应，截图不能读取真实业务资料。
  await page.route('**/api/**', (route) => route.fulfill({ json: { success: true, data: {} } }));
});

async function save(page: Page, name: string) {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({
    path: path.join(outputDir, name),
    fullPage: true,
    animations: 'disabled',
  });
}

test('生成销售首页和平台能力截图', async ({ page }) => {
  await page.route('**/api/sales-landing', async (route) => {
    await route.fulfill({ json: { success: true, data: {} } });
  });

  await page.goto('/');
  await expect(page.getByRole('heading', { level: 1 })).toContainText('稳定高速');
  await save(page, '01-sales-landing.png');

  await page.goto('/platform');
  await expect(page.getByRole('heading', { level: 1 })).toContainText('出口管理');
  await save(page, '02-platform.png');
});

test('生成管理员概览截图', async ({ page }) => {
  await mockAdminSession(page);
  await page.route('**/api/admin/access-operations/summary', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          active_users: 128,
          active_access_lines: 18,
          healthy_exit_pools: 16,
          monthly_billed_traffic_gb: 2860,
          config_dirty_nodes: 1,
          access_node_count: 8,
          recent_events: [
            { id: 1, level: 'info', title: '演示节点配置已同步', time: '刚刚' },
            { id: 2, level: 'warning', title: '一台演示节点等待同步', time: '5 分钟前' },
            { id: 3, level: 'info', title: '数据库备份已完成', time: '30 分钟前' },
          ],
        },
      }),
    });
  });

  await page.goto('/overview');
  await expect(page.getByRole('heading', { name: '总览', exact: true })).toBeVisible();
  await expect(page.getByText('128')).toBeVisible();
  await save(page, '03-admin-overview.png');
});

test('生成用户订阅截图', async ({ page }) => {
  await mockUserSession(page);
  await page.route('**/api/user/subscription', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          token: 'demo-token',
          plan_name: '标准演示套餐',
          expires_at: '2030-12-31',
          traffic: { used: 24.6, total: 100 },
          subscription_url: 'https://panel.example.test/sub/demo-token',
          access_lines: [
            {
              id: 'demo-hk',
              name: '香港高速',
              line_group_id: 'demo-asia',
              line_group_name: '亚洲节点',
              region: 'HK',
              listen_host: 'hk-access.example.test',
              listen_port: 443,
              inbound_protocol: 'vless',
              status: 'enabled',
            },
            {
              id: 'demo-sg',
              name: '新加坡优选',
              line_group_id: 'demo-asia',
              line_group_name: '亚洲节点',
              region: 'SG',
              listen_host: 'sg-access.example.test',
              listen_port: 8443,
              inbound_protocol: 'trojan',
              status: 'enabled',
            },
            {
              id: 'demo-us',
              name: '美国西部',
              line_group_id: 'demo-global',
              line_group_name: '全球节点',
              region: 'US',
              listen_host: 'us-access.example.test',
              listen_port: 2053,
              inbound_protocol: 'hysteria2',
              status: 'enabled',
            },
          ],
        },
      }),
    });
  });
  await page.route('**/api/user/usage', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          real_bytes: 26414048870,
          billed_bytes: 28600998502,
          real_gb: 24.6,
          billed_gb: 26.64,
        },
      }),
    });
  });

  const subscriptionResponse = page.waitForResponse((response) => (
    new URL(response.url()).pathname === '/api/user/subscription'
  ));
  await page.goto('/subscription');
  await subscriptionResponse;
  await expect(page.getByRole('heading', { name: '我的服务', exact: true })).toBeVisible();
  await expect(page.getByText('标准演示套餐')).toBeVisible();
  await expect(page.getByText('香港高速')).toBeVisible();
  await save(page, '04-user-subscription.png');
});
