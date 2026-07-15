// 用途：验证管理员后台使用教程页面可进入且覆盖核心运营流程。
// 本测试只检查页面导航和静态教程内容，不依赖模拟接口。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员可以打开系统使用教程页面', async ({ page }) => {
  await mockAdminSession(page);
  await page.route('**/api/admin/access-operations/summary', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          active_users: 0,
          active_access_lines: 0,
          healthy_exit_pools: 0,
          monthly_billed_traffic_gb: 0,
          config_dirty_nodes: 0,
          generated_at: '2026-06-15T00:00:00Z',
          recent_events: [],
        },
      }),
    });
  });
  await page.goto('/overview');

  await expect(page.locator('.shell__menu').getByText('使用教程')).toBeVisible();

  await page.goto('/admin/tutorial');
  await expect(page.getByRole('heading', { name: '使用教程' })).toBeVisible();
  await expect(page.getByText('从出口管理到用户订阅的完整运营路径')).toBeVisible();
  await expect(page.getByRole('heading', { name: '第一步：准备出口管理' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '第二步：创建入口绑定' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '第三步：配置分组' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '第四步：部署中转节点' })).toBeVisible();
  await expect(page.getByText('由平台自动连接服务器、上传并执行脚本')).toBeVisible();
  await expect(page.getByText('远端执行、拉取容器和登记节点')).toBeVisible();
  await expect(page.getByRole('heading', { name: '第五步：配置套餐授权' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '第六步：交付用户订阅' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '第七步：日常运维检查' })).toBeVisible();
});
