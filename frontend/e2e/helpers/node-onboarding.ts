// 新增节点的三条真实 UI 路径；共用精灵入口，保留原有 API/字段断言。
import { expect, type Page } from '@playwright/test';

export async function openNodeOnboarding(page: Page, method: 'automatic' | 'manual' | 'existing') {
  await page.getByRole('button', { name: '新增中转节点', exact: true }).click();
  const title = { automatic: '一键安装 Agent', manual: '手动安装 Agent', existing: '接入已有 Agent' }[method];
  await page.getByRole('dialog', { name: '新增中转节点', exact: true }).getByRole('button', { name: title, exact: true }).click();
  const dialog = page.getByRole('dialog', { name: title, exact: true });
  await expect(dialog).toBeVisible();
  if (method === 'automatic') await dialog.getByRole('button', { name: '高级安装选项', exact: true }).click();
  return dialog;
}

export async function expectAdminWorkspaces(page: Page) {
  const navigation = page.getByRole('complementary', { name: '工作区导航' });
  for (const name of ['总览', '线路', '用户', '套餐与运营', '节点', '监控与日志', '系统设置']) {
    await expect(navigation.getByRole('link', { name, exact: true })).toBeVisible();
  }
}
