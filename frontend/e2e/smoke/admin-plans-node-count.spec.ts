// 用途：覆盖套餐授权页按单层分组统计绑定节点数量。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('套餐授权分组直接显示分组内绑定节点数量', async ({ page }) => {
  await mockAdminSession(page);

  const lineGroups = [
    {
      id: 'group-default',
      name: 'GPT 分组',
      enabled: true,
      sort_weight: 1,
      binding_node_ids: ['binding-a-1', 'binding-a-2', 'binding-b'],
    },
  ];

  await page.route('**/api/admin/plans', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: [{
          id: 'plan-default',
          name: '基础套餐',
          is_default: true,
          enabled: true,
          traffic_limit_bytes: 10737418240,
          billing_multiplier: 1,
          price_cents: 0,
          currency: 'USDT',
          duration_days: 30,
          sort_weight: 1,
          line_groups: [{ line_group_id: 'group-default', billing_multiplier: 1 }],
        }],
      }),
    });
  });
  await page.route('**/api/admin/access-routing', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_nodes: [],
          access_lines: [],
          exit_pools: [],
          line_groups: lineGroups,
        },
      }),
    });
  });

  await page.goto('/admin/plans');
  await page.getByRole('row', { name: /基础套餐/ }).locator('.el-table__expand-icon').click();
  await expect(page.getByText('绑定节点：3 条')).toBeVisible();
  await expect(page.getByText('按订阅口径折叠')).toHaveCount(0);

  await page.getByRole('button', { name: '编辑分组', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: /配置套餐分组/ });
  await expect(dialog).toContainText('GPT 分组（3 个绑定节点）');
  await expect(dialog).toContainText('3 个绑定节点');
});
