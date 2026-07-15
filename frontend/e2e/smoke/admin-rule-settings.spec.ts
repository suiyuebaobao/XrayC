// 用途：覆盖管理员在独立规则设置页维护通用规则和规则库的 smoke 场景。
import { expect, test } from '@playwright/test';

import { mockAdminSession } from '../helpers/smoke';

test('管理员在规则设置页维护通用规则和规则库', async ({ page }) => {
  await mockAdminSession(page);

  const settings = {
    mixed_port: 7890,
    allow_lan: false,
    mode: 'rule',
    log_level: 'info',
    profile_name: 'XrayC Smoke',
    update_interval_hours: 24,
    default_rules: ['MATCH,PROXY'],
    auto_test_enabled: false,
    auto_test_name: '自动选择',
    auto_test_url: 'http://cp.cloudflare.com/generate_204',
    auto_test_interval_seconds: 86400,
    block_unhealthy_lines: false,
  };
  const ruleSets = [
    {
      id: 'rule-ai',
      name: 'AI 规则',
      description: '已有规则',
      enabled: true,
      rules: ['DOMAIN-SUFFIX,openai.com,PROXY'],
      binding_count: 1,
      created_at: '2026-06-11T00:00:00Z',
      updated_at: '2026-06-11T00:00:00Z',
    },
  ];
  let submittedCommon: Record<string, unknown> | null = null;
  let submittedRuleSet: Record<string, unknown> | null = null;

  await page.route('**/api/admin/subscription-settings', async (route) => {
    if (route.request().method() === 'PUT') {
      submittedCommon = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: submittedCommon }),
      });
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: settings }),
    });
  });
  await page.route('**/api/admin/subscription-rule-sets', async (route) => {
    if (route.request().method() === 'POST') {
      submittedRuleSet = JSON.parse(route.request().postData() ?? '{}') as Record<string, unknown>;
      ruleSets.push({
        id: 'rule-created',
        name: String(submittedRuleSet.name),
        description: String(submittedRuleSet.description ?? ''),
        enabled: Boolean(submittedRuleSet.enabled ?? true),
        rules: Array.isArray(submittedRuleSet.rules) ? submittedRuleSet.rules.map(String) : [],
        binding_count: 0,
        created_at: '2026-06-11T00:00:00Z',
        updated_at: '2026-06-11T00:00:00Z',
      });
      await route.fulfill({
        status: 201,
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data: { id: 'rule-created' } }),
      });
      return;
    }

    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { rule_sets: ruleSets } }),
    });
  });

  await page.goto('/admin/rule-settings');
  await expect(page.getByRole('heading', { name: '规则设置' })).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('规则设置')).toBeVisible();

  const commonCard = page.locator('[data-testid="common-rules-section"]');
  await expect(commonCard.getByText('通用规则', { exact: true })).toBeVisible();
  await commonCard.getByTestId('rule-target').locator('input').fill('baidu.com');
  await commonCard.getByTestId('rule-action').click();
  await page.getByRole('option', { name: '直连' }).click();
  await commonCard.getByRole('button', { name: '添加规则' }).click();
  await expect(commonCard.getByText('DOMAIN-SUFFIX,baidu.com,DIRECT')).toBeVisible();

  const commonSave = page.waitForResponse(
    (response) => response.url().includes('/api/admin/subscription-settings')
      && response.request().method() === 'PUT',
  );
  await page.getByRole('button', { name: '保存通用规则' }).click();
  await commonSave;
  expect(submittedCommon?.default_rules).toContain('DOMAIN-SUFFIX,baidu.com,DIRECT');

  const ruleLibrary = page.locator('[data-testid="rule-library-section"]');
  await expect(ruleLibrary.getByText('规则库', { exact: true })).toBeVisible();
  await expect(ruleLibrary.getByText('AI 规则')).toBeVisible();
  await expect(ruleLibrary.getByText('1 个分组')).toBeVisible();

  await ruleLibrary.getByRole('button', { name: '创建规则库' }).click();
  await page.getByLabel('规则库名称').fill('游戏规则');
  await page.getByLabel('备注').fill('游戏域名');
  await page.getByTestId('rule-target').last().locator('input').fill('game.example.com');
  await page.getByTestId('rule-action').last().click();
  await page.getByRole('option', { name: '走绑定分组' }).click();
  await page.getByRole('button', { name: '添加规则' }).last().click();
  await expect(page.getByText('DOMAIN-SUFFIX,game.example.com,PROXY')).toBeVisible();

  const ruleSetSave = page.waitForResponse(
    (response) => response.url().includes('/api/admin/subscription-rule-sets')
      && response.request().method() === 'POST',
  );
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await ruleSetSave;
  expect(submittedRuleSet?.name).toBe('游戏规则');
  expect(submittedRuleSet?.rules).toContain('DOMAIN-SUFFIX,game.example.com,PROXY');
  await expect(ruleLibrary.getByText('游戏规则')).toBeVisible();
});
