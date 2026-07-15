//! 尾巴2:中转节点运维 UI 主面板真实核对(真实节点、真实后端数据)。
//! 覆盖三项「新功能 UI」并守住红线——重启只验二次确认弹窗、点「取消」绝不真重启:
//!   ① 内核状态标签:connmark 可用且不待升级时,不应渲染「内核待升级·需重启」warning 标签。
//!   ② 续期 SSL 证书:节点卡片有「续期 SSL」按钮(文案口径与后端 tls/renew 排队一致)。
//!   ③ 重启节点:有「重启节点」按钮,点开二次确认框核对风险文案(整机重启/停止服务/数分钟)后点「取消」。
//! 安全:不打印弹窗正文(内嵌节点名/地址),只做子串断言;不点「确认重启」,不触发任何真实重启。
import { expect, test } from '@playwright/test';

import { login } from './runtime-api';

const ADMIN_ACCOUNT = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PASSWORD = process.env.E2E_ADMIN_PASSWORD || 'admin123456';

test('尾巴2:节点续签/重启/内核状态 UI 主面板真实核对(不真重启)', async ({ page }) => {
  await login(page, '/admin/login', ADMIN_ACCOUNT, ADMIN_PASSWORD, /\/(overview|dashboard)$/);
  await page.goto('/admin/transit-nodes');

  // 定位中转节点卡片(基线只有一个节点;不按名定位以免引用内嵌真实地址的节点名)。
  const nodeCard = page.locator('.relay-card').first();
  await expect(nodeCard, '中转节点页应渲染节点卡片').toBeVisible({ timeout: 15_000 });

  // ① 内核状态:connmark 可用、不待升级 → 不应出现「内核待升级·需重启」标签。
  const kernelTag = nodeCard.locator('.el-tag', { hasText: '内核待升级·需重启' });
  expect(await kernelTag.count(), '正常内核态不应渲染「内核待升级·需重启」标签').toBe(0);

  // ② 续期 SSL 证书:卡片有「续期 SSL」按钮(无证书时按钮置灰但仍渲染)。
  const renewBtn = nodeCard.locator('.el-button', { hasText: '续期 SSL' }).first();
  await expect(renewBtn, '节点卡片应有「续期 SSL」按钮').toBeVisible();

  // ③ 重启节点:按钮存在且可点。
  const rebootBtn = nodeCard.locator('.el-button', { hasText: '重启节点' }).first();
  await expect(rebootBtn, '节点卡片应有「重启节点」按钮').toBeVisible();
  await expect(rebootBtn).toBeEnabled();

  // ③' 点开二次确认弹窗,核对风险文案后点「取消」——红线:绝不点「确认重启」、不触发真实重启。
  await rebootBtn.click();
  const msgBox = page.locator('.el-message-box');
  await expect(msgBox, '应弹出重启二次确认框').toBeVisible({ timeout: 10_000 });
  await expect(msgBox.locator('.el-message-box__title')).toContainText('重启中转节点');
  await expect(msgBox, '正文应警告整机重启').toContainText('整机重启');
  await expect(msgBox, '正文应警告停止服务').toContainText('停止服务');
  await expect(msgBox, '正文应警告停机数分钟').toContainText('数分钟');
  await expect(msgBox.locator('button', { hasText: '确认重启' }), '应有「确认重启」按钮').toBeVisible();
  await expect(msgBox.locator('button', { hasText: '取消' }), '应有「取消」按钮').toBeVisible();
  await msgBox.locator('button', { hasText: '取消' }).click();
  await expect(msgBox, '点取消后弹窗应关闭、未触发重启').toBeHidden({ timeout: 5_000 });
});
