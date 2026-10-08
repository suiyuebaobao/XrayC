// 用途：覆盖普通用户登录后查看套餐、订阅、订单与兑换入口的 smoke 流程。
import { expect, test } from '@playwright/test';

import { mockUserSession } from '../helpers/smoke';

test('普通用户可以登录并查看订阅信息', async ({ page }) => {
  await mockUserSession(page);
  await page.route('**/api/user/subscription', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          token: 'demo-token',
          plan_name: '基础套餐',
          expires_at: '2036-05-16T00:00:00Z',
          traffic: { used: 0, total: 10 },
          subscription_url: '/sub/demo-token',
          access_lines: [
            {
              id: 'line-smoke-1',
              uuid: 'line-smoke-1',
              name: '香港高速',
              line_group_id: 'group-smoke-hk',
              line_group_name: '香港分组',
              line_group_icon: 'legacy-group-icon',
              icon: 'legacy-line-icon',
              region: 'HK',
              listen_host: 'access.example.test',
              listen_port: 443,
              inbound_protocol: 'vless',
              status: 'enabled',
              // 负向样本：即使异常响应夹带上游字段，用户订阅页也不能展示。
              exit_pool_name: '默认出口池',
              exit_endpoint_host: 'third-party-exit.example.test',
              upstream_proxy_url: 'socks5://example-user:example-password@third-party-exit.example.test:1080',
              agent_token: 'secret-agent-token',
            },
          ],
        },
      }),
    });
  });
  await page.route('**/api/plans', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [
          {
            id: '00000000-0000-0000-0000-000000000001',
            name: '基础套餐',
            is_default: true,
            enabled: true,
            traffic_limit_bytes: 10737418240,
            billing_multiplier: 1,
            price_cents: 0,
            currency: 'USDT',
            duration_days: 30,
            sort_weight: 1,
            line_groups: [{ line_group_id: 'group-smoke-hk', billing_multiplier: 1 }],
          },
          {
            id: '00000000-0000-0000-0000-000000000002',
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
        ],
      }),
    });
  });

  await page.route('**/api/orders', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({
          success: true,
          data: {
            id: 'order-smoke-created',
            order_no: 'ORD-SMOKE-CREATED',
            user_email: 'demo@example.test',
            plan_name: '标准套餐',
            amount_cents: 990,
            amount: 9.9,
            currency: 'USDT',
            status: 'pending',
            payment_address: 'xrayc-smoke-created-payment-address',
            expires_at: '2026-05-19T01:30:00Z',
            duration_days: 30,
            created_at: '2026-05-19T00:30:00Z',
            paid_at: null,
          },
        }),
      });
      return;
    }

    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          items: [
            {
              id: 'order-smoke-1',
              order_no: 'ORD-SMOKE-001',
              user_email: 'demo@example.test',
              plan_name: '标准套餐',
              amount_cents: 990,
              amount: 9.9,
              currency: 'USDT',
              status: 'pending',
              payment_address: 'xrayc-smoke-payment-address',
              expires_at: '2026-05-19T01:00:00Z',
              duration_days: 30,
              created_at: '2026-05-19T00:00:00Z',
              paid_at: null,
            },
          ],
        },
      }),
    });
  });
  await page.route('**/api/payment/channels', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          enabled: true,
          channels: [{ channel: 'alipay_qr', label: '支付宝扫码' }],
        },
      }),
    });
  });
  await page.route(/.*\/api\/user\/invite-codes(?:\?.*)?$/, async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          enabled: true,
          max_count: 3,
          generated_count: 1,
          remaining_count: 2,
          items: [
            {
              code: 'INVITE-SMOKE-001',
              is_used: false,
              used_by_email: '',
              used_at: '',
              created_at: '2026-06-03T00:00:00Z',
            },
          ],
        },
      }),
    });
  });

  await page.goto('/dashboard');

  await expect(page).toHaveURL(/\/dashboard$/);
  await expect(page.getByRole('heading', { name: '我的服务' })).toBeVisible();
  await expect(page.getByRole('complementary').getByRole('link', { name: '我的服务', exact: true })).toBeVisible();
  await expect(page.getByRole('complementary').getByRole('link', { name: '套餐', exact: true })).toBeVisible();

  await page.goto('/plans');
  await expect(page.getByRole('heading', { name: '套餐' })).toBeVisible();
  await expect(page.getByText('基础套餐').first()).toBeVisible();
  // 旧的「支付暂停」按钮已移除，现套餐页每个套餐卡片提供「购买」按钮发起真实下单。
  await expect(page.getByRole('button', { name: '购买' }).first()).toBeVisible();

  await page.getByRole('complementary').getByRole('link', { name: '我的服务', exact: true }).click();
  await expect(page).toHaveURL(/\/dashboard$/);
  await expect(page.getByRole('heading', { name: '我的服务' })).toBeVisible();
  await expect(page.getByText('基础套餐')).toBeVisible();
  await expect(page.locator('.el-card').filter({ hasText: '订阅链接' }).locator('input')).toHaveValue(/\/sub\/demo-token$/);
  await expect(page.getByText('可用节点', { exact: true })).toBeVisible();
  await expect(page.locator('strong', { hasText: '香港高速' }).first()).toBeVisible();
  await expect(page.getByText('香港分组')).toBeVisible();
  await expect(page.getByText('access.example.test:443')).toBeVisible();
  await expect(page.getByText('默认出口池')).toHaveCount(0);
  await expect(page.getByText('third-party-exit.example.test')).toHaveCount(0);
  await expect(page.getByText('example-user')).toHaveCount(0);
  await expect(page.getByText('example-password')).toHaveCount(0);
  await expect(page.getByText('secret-agent-token')).toHaveCount(0);
  await expect(page.getByText('legacy-line-icon')).toHaveCount(0);
  await expect(page.getByText('legacy-group-icon')).toHaveCount(0);
  await expect(page.getByRole('complementary').getByRole('link', { name: '节点', exact: true })).toHaveCount(0);
  await page.goto('/orders');
  await expect(page.getByRole('heading', { name: '我的订单' })).toBeVisible();
  await expect(page.getByText('支付功能暂停')).toBeVisible();
  await expect(page.getByText('xrayc-smoke-payment-address')).toBeVisible();
  await expect(page.getByText('9.9').first()).toBeVisible();
  await expect(page.getByText('USDT').first()).toBeVisible();
  await expect(page.getByText('2026-05-19T01:00:00Z')).toBeVisible();
  await expect(page.getByRole('button', { name: '复制', exact: true }).first()).toBeVisible();
  await expect(page.getByText('人工处理')).toHaveCount(0);
  await expect(page.getByText('后端接口待接入')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '提交订单' })).toHaveCount(0);
  await expect(page.getByText('本次订单支付信息')).toHaveCount(0);

  await page.goto('/redeem');
  await expect(page.getByRole('heading', { name: '兑换码' })).toBeVisible();
  await expect(page.getByRole('button', { name: '提交兑换' })).toBeVisible();
  await expect(page.getByText('后端接口待接入')).toHaveCount(0);

  await page.goto('/invite-codes');
  await expect(page.getByRole('heading', { name: '邀请码' })).toBeVisible();
  await expect(page.getByText('我的邀请码')).toBeVisible();
  await expect(page.getByText('后端接口待接入')).toHaveCount(0);

  await page.goto('/admin/line-groups');
  await expect(page).toHaveURL(/\/dashboard$/);
});
