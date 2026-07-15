// 用途：保留公开页面和注册流程的基础 smoke 测试，避免入口测试文件超过 500 行。
import { expect, test } from '@playwright/test';

test('公共首页和平台介绍页可访问', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('main h1')).toBeVisible();
  await expect(page.getByText('中转入口订阅服务')).toBeVisible();
  await expect(page.getByText('选择适合你的流量套餐')).toBeVisible();
  await expect(page.getByRole('link', { name: /查看套餐|立即注册/ }).first()).toBeVisible();
  await expect(page.getByText('销售首页配置')).toHaveCount(0);

  await page.goto('/platform');
  await expect(page.getByRole('heading', { name: /出口管理、入口管理、分组、套餐授权统一管理/ })).toBeVisible();
  await expect(page.getByText('中转节点编排')).toBeVisible();
});

test('注册页可访问并展示注册安全字段', async ({ page }) => {
  await page.route('**/api/auth/security', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          require_invite_code: true,
          captcha: {
            register_enabled: false,
            user_login_enabled: false,
            admin_login_enabled: false,
          },
          email_verification: {
            enabled: true,
            allowed_domains: ['example.test'],
            cooldown_seconds: 60,
          },
          login_guard: {
            enabled: true,
            failure_threshold: 5,
            lock_minutes: 5,
          },
        },
      }),
    });
  });

  await page.goto('/register');
  await expect(page.getByRole('heading', { name: '创建账号' })).toBeVisible();
  await expect(page.getByPlaceholder('user@example.com')).toBeVisible();
  await expect(page.getByPlaceholder('至少 8 位')).toBeVisible();
  await expect(page.getByPlaceholder('可填写邀请人提供的邀请码')).toBeVisible();
  await expect(page.getByRole('button', { name: '发送邮箱验证码' })).toBeVisible();
  await expect(page.getByText('当前仅允许以下邮箱域名注册：example.test')).toBeVisible();
  await expect(page.getByRole('button', { name: '注册并领取基础套餐' })).toBeVisible();
  await expect(page.getByRole('link', { name: '返回登录' })).toBeVisible();
});
