/*
 * 用途：流 A2 真实 UI —— 在「套餐管理→编辑分组」把测试分组授权给测试套餐。
 * 本文件不修改业务实现，只用真实点击驱动前端，并捕获真实 PUT 报文与响应。
 * 全程脱敏：只打印分组/套餐名与计数，不打印 token/密码/出口真实信息。
 */
import { expect, test } from '@playwright/test';

const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';
const PLAN_NAME = process.env.PHASEA2_PLAN_NAME || 'e2e-realui-20260624-143515-plan';
const GROUP_NAME = process.env.PHASEA2_GROUP_NAME || 'e2e-realui-20260624-143515-group';

test('phaseA2: 套餐授权分组（真实 UI 点击 + 捕获真实报文）', async ({ page }) => {
  // 1) 真实管理员登录
  await page.goto('/login');
  await page.getByPlaceholder('admin 或 user@example.com').fill(ADMIN_ACCOUNT);
  await page.getByPlaceholder('请输入密码').fill(ADMIN_PASSWORD);
  const loginResp = page.waitForResponse(
    (r) => new URL(r.url()).pathname === '/api/auth/login',
  );
  await page.getByRole('button', { name: '进入控制台' }).click();
  const login = await loginResp;
  expect(login.status(), 'login status').toBeGreaterThanOrEqual(200);
  expect(login.status(), 'login status').toBeLessThan(300);

  // 2) 进入套餐管理页
  await page.goto('/admin/plans');
  await expect(page.getByText(PLAN_NAME).first()).toBeVisible({ timeout: 15000 });

  // 3) 定位到测试套餐这一行，点「编辑分组」
  const planRow = page.locator('tr', { hasText: PLAN_NAME }).first();
  await planRow.getByRole('button', { name: '编辑分组' }).click();

  const dialog = page.getByRole('dialog', { name: new RegExp('配置套餐分组') });
  await expect(dialog).toBeVisible();

  // 4) 在「可用分组」多选里选我们的测试分组（真实点击下拉）
  // 注意：同名分组有多个，UI 选项 label 形如「名称（N 个绑定节点）」，
  // 用绑定节点数消歧，选真正含 entry→exit 绑定的那个分组。
  const select = dialog.locator('.authorization-form__select').first();
  await select.click();
  await page.waitForTimeout(500);
  // 调试：打印当前下拉所有可见选项文本（脱敏，只有分组名+计数）
  const allOptions = await page.locator('.el-select-dropdown__item').allInnerTexts();
  console.log(`[phaseA2] dropdown options: ${JSON.stringify(allOptions)}`);
  const optionMatcher = process.env.PHASEA2_GROUP_OPTION_TEXT || GROUP_NAME;
  // el-select 下拉是 teleport 到 body 的；按 label 文本定位 option
  const option = page.locator('.el-select-dropdown__item', { hasText: optionMatcher }).first();
  await expect(option).toBeVisible({ timeout: 8000 });
  await option.click();
  // 关闭下拉
  await page.keyboard.press('Escape');

  // 5) 捕获真实 PUT /api/admin/plans/{planId}/line-groups 报文
  const putResp = page.waitForResponse(
    (r) => /\/api\/admin\/plans\/[^/]+\/line-groups$/.test(new URL(r.url()).pathname)
      && r.request().method() === 'PUT',
  );
  await dialog.getByRole('button', { name: '保存分组' }).click();
  const put = await putResp;

  // 脱敏打印：路径模板、方法、状态、请求体字段名 + 计数
  const reqBody = put.request().postDataJSON?.() ?? null;
  const groupsCount = Array.isArray(reqBody?.line_groups) ? reqBody.line_groups.length : -1;
  const reqFields = reqBody ? Object.keys(reqBody).sort().join(',') : '(none)';
  let respJson: unknown = null;
  try { respJson = await put.json(); } catch { respJson = null; }
  const respShape = respJson && typeof respJson === 'object'
    ? Object.keys(respJson as Record<string, unknown>).sort().join(',')
    : '(non-json)';

  console.log(`[phaseA2] PUT method=${put.request().method()} pathTemplate=/api/admin/plans/{planId}/line-groups status=${put.status()}`);
  console.log(`[phaseA2] request bodyFields=[${reqFields}] line_groups.length=${groupsCount}`);
  console.log(`[phaseA2] response shape=[${respShape}]`);

  expect(put.status(), 'PUT line-groups status').toBeGreaterThanOrEqual(200);
  expect(put.status(), 'PUT line-groups status').toBeLessThan(300);
  expect(groupsCount, 'authorized groups in request').toBeGreaterThan(0);
});
