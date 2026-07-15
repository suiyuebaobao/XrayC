/*
 * 用途：验证 no-mock 运行时页面直接使用真实后端数据。
 * 测试流程保留在本文件，通用工具拆到 e2e/runtime 目录。
 */
import { expect, type Page, test } from '@playwright/test';

import {
  API,
  collectApiStatuses,
  gotoAndWaitForApi,
  gotoAndWaitForApis,
  login,
  requiredAdminApiPaths,
  requiredUserApiPaths,
  responseData,
  runtimeAuth,
  skipOrFailMissingCredentials,
  waitForApiResponse,
} from './runtime/runtime-api';
import {
  formatKnownNumber,
  formatPrice,
  formatRuntimeMetricNumber,
  formatRuntimeNumber,
  formatUserPlanTraffic,
  normalizeControlPlane,
  normalizeExitEndpoints,
  normalizeOperationsSummary,
  normalizePlans,
  normalizeSubscription,
} from './runtime/runtime-data';
import {
  assertAccessRoutingRows,
  assertAdminPlanRows,
  assertLinePoolEndpointRows,
  assertNoFakeZeroRuntimeCards,
  assertOperationsRuntimeLines,
  assertSubscriptionLines,
  assertSubscriptionYamlDownload,
  expectMetricCard,
  expectVisibleText,
} from './runtime/runtime-assertions';

test('公共注册页 no-mock 使用真实认证安全配置', async ({ page }) => {
  const landingResponse = await gotoAndWaitForApi(page, '/', API.salesLanding);
  await responseData(landingResponse);
  await expect(page.getByText('XrayC').first()).toBeVisible();
  await expect(page.getByText('Plans').first()).toBeVisible();
  await expect(page.getByText('FAQ').first()).toBeVisible();
  await expect(page.locator('.marketing-hero__actions').getByRole('button').first()).toBeVisible();

  await page.goto('/platform');
  await expect(page.getByRole('heading', { name: '出口管理、入口管理、分组、套餐授权统一管理。' })).toBeVisible();
  await expect(page.getByLabel('平台能力')).toBeVisible();
  await expectVisibleText(page, '中转节点编排');

  const securityResponse = await gotoAndWaitForApi(page, '/register', API.authSecurity);
  await responseData(securityResponse);
  await expect(page.getByRole('heading', { name: '创建账号' })).toBeVisible();
  await expect(page.getByPlaceholder('user@example.com')).toBeVisible();
  await expect(page.getByPlaceholder('至少 8 位')).toBeVisible();
  await expect(page.getByPlaceholder('可填写邀请人提供的邀请码')).toBeVisible();
  await expect(page.getByRole('button', { name: '注册并领取基础套餐' })).toBeVisible();
  await expect(page.getByRole('link', { name: '返回登录' })).toBeVisible();
});

test('普通用户 no-mock 运行时页面使用真实后端数据', async ({ page }) => {
  skipOrFailMissingCredentials(
    Boolean(runtimeAuth.userAccount && runtimeAuth.userPassword),
    'user no-mock smoke requires E2E_USER_ACCOUNT/E2E_USER_PASSWORD or SMOKE_LOGIN_ACCOUNT/SMOKE_LOGIN_PASSWORD',
  );

  const api = collectApiStatuses(page, requiredUserApiPaths);

  await login(page, '/login', runtimeAuth.userAccount, runtimeAuth.userPassword, /\/dashboard$/);
  api.assert2xx(API.login);

  const dashboardSubscriptionResponse = await gotoAndWaitForApi(page, '/dashboard', API.userSubscription);
  const dashboardSubscription = normalizeSubscription(await responseData(dashboardSubscriptionResponse));
  await expect(page.getByRole('heading', { name: '首页' })).toBeVisible();
  await expect(page.getByText('当前套餐')).toBeVisible();
  await expect(page.getByText('剩余流量')).toBeVisible();
  await expect(page.getByText('可用节点', { exact: true })).toBeVisible();
  await expectVisibleText(page, dashboardSubscription.planName || '未开通');

  const subscriptionResponse = await gotoAndWaitForApi(page, '/subscription', API.userSubscription);
  const subscription = normalizeSubscription(await responseData(subscriptionResponse));
  await expect(page.getByRole('heading', { name: '订阅' })).toBeVisible();
  await expect(page.getByText('订阅链接', { exact: true })).toBeVisible();
  await expectVisibleText(page, subscription.planName);
  if (subscription.expiresAt) {
    await expectVisibleText(page, `到期时间：${subscription.expiresAt}`);
  }
  await expectVisibleText(
    page,
    `${subscription.trafficGb.used} / ${subscription.trafficGb.total} GB`,
  );
  if (subscription.token) {
    await expect(page.locator('.el-card').filter({ hasText: '订阅链接' }).locator('input')).not.toHaveValue('');
  }
  await expect(page.getByText('可用节点', { exact: true })).toBeVisible();
  await assertSubscriptionLines(page, subscription);
  await assertSubscriptionYamlDownload(page, subscription);

  const plansResponse = await gotoAndWaitForApi(page, '/plans', API.userPlans);
  const plans = normalizePlans(await responseData(plansResponse)).filter((plan) => plan.enabled && !plan.isDeleted);
  await expect(page.getByRole('heading', { name: '套餐' })).toBeVisible();
  if (plans.length === 0) {
    await expect(page.getByText('暂无可购买套餐')).toBeVisible();
  } else {
    const firstPlan = plans[0];
    await expectVisibleText(page, firstPlan.name);
    await expectVisibleText(page, formatPrice(firstPlan));
    await expectVisibleText(page, `${firstPlan.durationDays} 天有效期`);
    await expectVisibleText(page, formatUserPlanTraffic(firstPlan));
    await expectVisibleText(page, '可用节点由套餐授权分组控制');
    // 旧的「支付暂停」按钮已移除，现套餐页每个套餐卡片提供「购买」按钮发起真实下单（与 smoke/user.spec.ts 同步）。
    await expect(page.getByRole('button', { name: '购买' }).first()).toBeVisible();
  }

  await gotoAndWaitForApi(page, '/orders', API.userOrders);
  await expect(page.getByRole('heading', { name: '我的订单' })).toBeVisible();
  await expectMetricCard(page, '订单总数', api.statuses(API.userOrders).length > 0 ? /\d+/ : '0');
  await expectVisibleText(page, '支付功能暂停');
  await expectVisibleText(page, '订单记录');
  await expectVisibleText(page, '订单号');

  await page.goto('/redeem');
  await expect(page.getByRole('heading', { name: '兑换码' })).toBeVisible();
  await expectVisibleText(page, '使用兑换码');
  await expect(page.getByPlaceholder('请输入兑换码')).toBeVisible();
  await expect(page.getByRole('button', { name: '提交兑换' })).toBeVisible();

  await gotoAndWaitForApi(page, '/invite-codes', API.userInviteCodes);
  await expect(page.getByRole('heading', { name: '邀请码' })).toBeVisible();
  await expectVisibleText(page, '自助生成');
  await expectVisibleText(page, '生成额度');
  await expectVisibleText(page, '剩余额度');
  await expectVisibleText(page, '我的邀请码');

  for (const path of requiredUserApiPaths) {
    api.assert2xx(path);
  }
});

test('管理员 no-mock 运行时页面使用真实后端数据', async ({ page }) => {
  test.setTimeout(60_000);

  skipOrFailMissingCredentials(
    Boolean(runtimeAuth.adminAccount && runtimeAuth.adminPassword),
    'admin no-mock smoke requires E2E_ADMIN_ACCOUNT/E2E_ADMIN_PASSWORD',
  );

  const api = collectApiStatuses(page, requiredAdminApiPaths);

  await login(page, '/admin/login', runtimeAuth.adminAccount, runtimeAuth.adminPassword, /\/overview$/);
  api.assert2xx(API.login);

  const overviewResponse = await gotoAndWaitForApi(page, '/overview', API.operationsSummary);
  await responseData(overviewResponse);
  await expect(page.getByRole('heading', { name: '概览' })).toBeVisible();
  await expectMetricCard(page, '活跃用户', /\d+/);
  await expectMetricCard(page, '启用线路', /\d+/);
  await expectMetricCard(page, '可用线路', /\d+/);
  await expectMetricCard(page, '本月扣费流量', /\d+(?:\.\d+)? GB/);
  await expectVisibleText(page, '中转节点运行态');
  await expectVisibleText(page, '待同步节点');

  const operationsSummaryPromise = waitForApiResponse(page, API.operationsSummary);
  const operationsRoutingPromise = waitForApiResponse(page, API.accessRouting);
  await page.goto('/admin/access-operations');
  const operationsSummary = normalizeOperationsSummary(await responseData(await operationsSummaryPromise));
  const operationsControlPlane = normalizeControlPlane(await responseData(await operationsRoutingPromise));
  // 监控中心合并页：默认「运行总览」tab 即原运营中心内容。
  await expect(page.getByRole('heading', { name: '监控中心' })).toBeVisible();
  await expectVisibleText(page, '中转运营态');
  await expectVisibleText(page, `运行态 ${operationsSummary.runtimeMetricStatus}`);
  await expectVisibleText(page, `中转节点 ${formatKnownNumber(operationsSummary.accessNodeCount)}`);
  await expectVisibleText(page, `线路 ${formatKnownNumber(operationsSummary.accessLineCount)}`);
  await expectVisibleText(page, '分组');
  await expectVisibleText(page, '账本');
  await expectMetricCard(page, '活跃用户', formatKnownNumber(operationsSummary.activeUsers));
  await expectMetricCard(page, '在线用户', formatRuntimeNumber(operationsSummary));
  await expectMetricCard(
    page,
    '活跃连接',
    formatRuntimeMetricNumber(operationsSummary.activeConnections, operationsSummary.runtimeMetricStatus),
  );
  await assertNoFakeZeroRuntimeCards(page, operationsSummary);
  await expectVisibleText(page, '中转入口运行态');
  await expectVisibleText(page, '独立客户端 IP');
  await expectVisibleText(page, '探测故障转移策略');
  await expectVisibleText(page, '启用自动故障转移');
  await expectVisibleText(page, '连续失败阈值');
  await assertOperationsRuntimeLines(page, operationsControlPlane);

  const healthSummaryPromise = waitForApiResponse(page, API.operationsSummary);
  const healthRoutingPromise = waitForApiResponse(page, API.accessRouting);
  // 健康检查已并入监控中心：旧路径重定向到监控中心，再切到「健康检查」tab 验证原内容。
  await page.goto('/admin/health-check');
  const healthSummary = normalizeOperationsSummary(await responseData(await healthSummaryPromise));
  await healthRoutingPromise;
  await expect(page.getByRole('heading', { name: '监控中心' })).toBeVisible();
  await page.getByRole('tab', { name: '健康检查' }).click();
  await expectHealthCard(page, '中转节点', /\d+\/\d+/);
  await expectHealthCard(page, '线路', /\d+\/\d+/);
  await expectHealthCard(page, '运行数据', /.+/);
  await expectHealthCard(page, '探针异常', /\d+/);
  await expectVisibleText(page, `线路状态 ${formatKnownNumber(healthSummary.exitProbeStateCount)} 条`);
  await expect(page.getByRole('tab', { name: '中转节点健康' })).toBeVisible();
  await expect(page.getByRole('tab', { name: '出口线路健康' })).toBeVisible();
  await expect(page.getByRole('tab', { name: '分组健康' })).toBeVisible();
  await expectHealthTrafficPanel(page, 'node', '中转节点健康');
  await page.getByRole('tab', { name: '出口线路健康' }).click();
  await expectHealthTrafficPanel(page, 'exit', '出口线路健康');
  await page.getByRole('tab', { name: '分组健康' }).click();
  await expectHealthTrafficPanel(page, 'group', '分组健康');
  await expectVisibleText(page, '高级健康详情');

  const accessRoutingResponse = await gotoAndWaitForApi(page, '/admin/transit-nodes', API.accessRouting);
  const accessRouting = normalizeControlPlane(await responseData(accessRoutingResponse));
  await expect(page.getByRole('heading', { name: '中转节点' })).toBeVisible();
  await expectVisibleText(page, '新增中转节点');
  await expectVisibleText(page, '运行入口');
  await expectVisibleText(page, '活跃连接');
  await expect(page.locator('.shell__menu').getByText('中转节点')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('入口管理')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('出口管理')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('分组', { exact: true })).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('套餐授权')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('用户管理')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('订单')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('兑换码')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('认证安全')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('审计日志')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('订阅设置')).toBeVisible();
  await expect(page.locator('.shell__menu').getByText('销售页配置')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Agent 安装说明', exact: true }).first()).toBeVisible();
  await expect(page.getByRole('button', { name: '一键部署', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '部署/重装' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '运行数据' })).toHaveCount(0);
  await expect(page.getByText('入站协议')).toHaveCount(0);
  await assertAccessRoutingRows(page, accessRouting);

  const [, linePoolEndpointsResponse] = await gotoAndWaitForApis(page, '/admin/line-pool', [API.exitResources, API.exitEndpoints]);
  const linePoolEndpoints = normalizeExitEndpoints(await responseData(linePoolEndpointsResponse));
  await expect(page.getByRole('heading', { name: '出口管理' })).toBeVisible();
  await expectVisibleText(page, '出口管理（统一列表）');
  await expectVisibleText(page, '所有来源的线路都在这里维护。');
  await expect(page.getByRole('button', { name: '添加线路', exact: true })).toBeVisible();
  await assertLinePoolEndpointRows(page, linePoolEndpoints);

  const lineGroupsRoutingPromise = waitForApiResponse(page, API.accessRouting);
  await page.goto('/admin/line-groups');
  const lineGroupsControlPlane = normalizeControlPlane(await responseData(await lineGroupsRoutingPromise));
  await expect(page.getByRole('heading', { name: '分组' })).toBeVisible();
  await expectVisibleText(page, '创建分组');
  if (lineGroupsControlPlane.lineGroups.length > 0) {
    await expectVisibleText(page, '分组内绑定节点');
    await expectVisibleText(page, '可选绑定节点');
    await expectVisibleText(page, '绑定节点');
    await expectVisibleText(page, '启用');
  }

  const adminPlansResponsePromise = waitForApiResponse(page, API.adminPlans);
  const adminPlansRoutingPromise = waitForApiResponse(page, API.accessRouting);
  await page.goto('/admin/plans');
  const adminPlans = normalizePlans(await responseData(await adminPlansResponsePromise));
  await adminPlansRoutingPromise;
  await expect(page.getByRole('heading', { name: '套餐授权' })).toBeVisible();
  await expectMetricCard(page, '套餐数量', String(adminPlans.length));
  await expectMetricCard(page, '基础套餐', adminPlans.find((plan) => plan.isDefault)?.name ?? '未配置');
  await expectVisibleText(page, '套餐列表');
  await assertAdminPlanRows(page, adminPlans);

  await gotoAndWaitForApis(page, '/admin/users', [API.adminUsers, API.adminPlans]);
  await expect(page.getByRole('heading', { name: '用户管理' })).toBeVisible();
  await expectVisibleText(page, '用户总数');
  await expectVisibleText(page, '用户列表');
  await expectVisibleText(page, '订阅状态');
  await runAdminUserCrudNoMock(page);

  await gotoAndWaitForApi(page, '/admin/orders', API.adminOrders);
  await expect(page.getByRole('heading', { name: '订单管理' })).toBeVisible();
  await expectVisibleText(page, '订单列表');
  await expectVisibleText(page, '订单号');
  await expectVisibleText(page, '支付金额');

  await gotoAndWaitForApis(page, '/admin/redeem-codes', [API.adminRedeemCodes, API.userPlans]);
  await expect(page.getByRole('heading', { name: '兑换码管理' })).toBeVisible();
  await expectVisibleText(page, '兑换码列表');
  await expectVisibleText(page, '批量生成');
  await expectVisibleText(page, '兑换码总数');

  await gotoAndWaitForApi(page, '/admin/audit-logs', API.adminAuditLogs);
  await expect(page.getByRole('heading', { name: '审计日志' })).toBeVisible();
  await expectVisibleText(page, '管理员审计日志');
  await expectVisibleText(page, '请求摘要');

  await gotoAndWaitForApi(page, '/admin/subscription-settings', API.adminSubscriptionSettings);
  await expect(page.getByRole('heading', { name: '订阅设置' })).toBeVisible();
  await expectVisibleText(page, '基础配置');
  await expectVisibleText(page, '线路过滤');
  await expectVisibleText(page, '自动测速');

  await gotoAndWaitForApi(page, '/admin/auth-security', API.adminAuthSecurity);
  await expect(page.getByRole('heading', { name: '认证安全' })).toBeVisible();
  await expectVisibleText(page, '安全策略');
  await expectVisibleText(page, '注册算术验证码');
  await expectVisibleText(page, '邀请码注册');
  await expectVisibleText(page, '登录失败锁定');
  await expectVisibleText(page, 'SMTP 发信配置');

  await gotoAndWaitForApi(page, '/admin/sales-landing', API.adminSalesLanding);
  await expect(page.getByRole('heading', { name: '销售首页配置' })).toBeVisible();
  await expectVisibleText(page, '首屏文案');
  await expectVisibleText(page, '结构化区块 JSON');
  await expectVisibleText(page, '预览');

  for (const path of requiredAdminApiPaths) {
    api.assert2xx(path);
  }
});

async function runAdminUserCrudNoMock(page: Page) {
  const createdIds: string[] = [];
  const suffix = `${Date.now()}-${Math.floor(Math.random() * 100000)}`;
  const email = `e2e-user-${suffix}@example.test`;

  try {
    await page.getByRole('button', { name: '新增用户' }).click();
    const createDialog = page.getByRole('dialog', { name: '新增用户' });
    await createDialog.getByPlaceholder('user@example.com').fill(email);
    await createDialog.getByPlaceholder('至少 8 位').fill('e2eUser123');
    const createResponsePromise = waitForApiResponse(page, API.adminUsers, { method: 'POST' });
    await createDialog.getByRole('button', { name: '保存' }).click();
    const created = await responseData(await createResponsePromise);
    const createdId = jsonString(created, 'id');
    if (createdId) {
      createdIds.push(createdId);
    }
    await expect(createDialog).toBeHidden();

    await searchAdminUser(page, email.toUpperCase());
    await expect(page.getByText(email).first()).toBeVisible();

    const userRow = page.locator('tr', { hasText: email }).first();
    const detailResponsePromise = page.waitForResponse((response) => (
      response.request().method() === 'GET' && new URL(response.url()).pathname === `/api/admin/users/${createdId}`
    ));
    await userRow.getByRole('button', { name: '编辑用户' }).click();
    await detailResponsePromise;

    const editDialog = page.getByRole('dialog', { name: /编辑用户/ });
    // 当前设计：编辑弹窗邮箱只读（邮箱=账号身份/验证码锚点，创建后不可改），不再改邮箱。
    // 仅验证只读并直接保存触发 PATCH；后续按原 email 检索/删除。
    await expect(editDialog.getByPlaceholder('user@example.com')).toBeDisabled();
    const updateResponsePromise = page.waitForResponse((response) => (
      response.request().method() === 'PATCH' && new URL(response.url()).pathname === `/api/admin/users/${createdId}`
    ));
    await editDialog.getByRole('button', { name: '保存' }).click();
    await updateResponsePromise;
    await expect(editDialog).toBeHidden();

    await searchAdminUser(page, email);
    await expect(page.getByText(email).first()).toBeVisible();

    const deleteResponsePromise = page.waitForResponse((response) => (
      response.request().method() === 'DELETE' && new URL(response.url()).pathname === `/api/admin/users/${createdId}`
    ));
    await page.locator('tr', { hasText: email }).getByRole('button', { name: '删除' }).click();
    await page.getByRole('button', { name: '删除', exact: true }).last().click();
    await deleteResponsePromise;
    createdIds.splice(createdIds.indexOf(createdId), 1);
    // 硬删后断言用户「行」消失即可（搜索框仍保留已输入的邮箱文本，getByText 全页面匹配会误命中筛选框，故锁定表格行）。
    await expect(page.locator('tr', { hasText: email })).toHaveCount(0);
  } finally {
    for (const userId of createdIds) {
      await adminDeleteUserByApi(page, userId);
    }
    // 「重置」非精确会同时命中行内「重置密码 / 重置订阅 token」按钮，用 exact 锁定搜索区清空筛选按钮。
    await page.getByRole('button', { name: '重置', exact: true }).click().catch(() => undefined);
  }
}

async function searchAdminUser(page: Page, keyword: string) {
  const responsePromise = waitForApiResponse(page, API.adminUsers);
  await page.getByPlaceholder('邮箱、名称、套餐').fill(keyword);
  await page.getByRole('button', { name: '查询' }).click();
  await responsePromise;
}

async function adminDeleteUserByApi(page: Page, userId: string) {
  const token = await page.evaluate(() => {
    const raw = window.localStorage.getItem('xrayc.session');
    if (!raw) {
      return '';
    }
    const parsed = JSON.parse(raw) as { accessToken?: string; access_token?: string; token?: string };
    return parsed.accessToken || parsed.access_token || parsed.token || '';
  });
  if (!token) {
    return;
  }
  await page.request.delete(`/api/admin/users/${encodeURIComponent(userId)}`, {
    headers: {
      Authorization: `Bearer ${token}`,
      'X-XrayC-CSRF': '1',
    },
  }).catch(() => undefined);
}

function jsonString(value: unknown, key: string) {
  return value && typeof value === 'object' && key in value
    ? String((value as Record<string, unknown>)[key] ?? '')
    : '';
}

async function expectHealthCard(page: Page, label: string, value: string | RegExp) {
  const card = page.locator('.health-card').filter({ hasText: label }).first();
  await expect(card).toBeVisible();
  await expect(card.locator('strong').filter({ hasText: value }).first()).toBeVisible();
}

async function expectHealthTrafficPanel(page: Page, kind: string, title: string) {
  // 监控中心「流量」tab 与「健康检查」tab 都用 HealthTrafficPanel，kind=node 的 testid 同名会 strict 撞 2 个。
  // 健康检查 tab 的面板包在 .health-board 卡片里，约束到它即可消歧（流量 tab 用 .monitor-traffic-card）。
  const panel = page.locator('.health-board').getByTestId(`health-${kind}-traffic`);
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(title);
  for (const label of ['今日', '本周', '本月', '峰值', '低谷', '总流量', '当月总流量']) {
    await expect(panel.getByText(label, { exact: true }).first()).toBeVisible();
  }
}
