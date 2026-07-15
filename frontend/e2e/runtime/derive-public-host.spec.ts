/*
 * 用途:验证「公网地址 / 客户连接地址」冗余字段已从两个建节点弹窗移除,
 * 且提交时 public_host 由三个对外直连地址按 IP直连>域名直连>CF域名 自动推导。
 * 真实管理员登录 + 真实 UI 点击 + 拦截真实创建请求体断言;
 * 用占位鉴权码提交,后端必拒(parse_access_node_auth_code),不创建任何节点、零副作用。
 * 不 mock 任何响应,只观测表单真实发出的请求。
 */
import { expect, test, type Page } from '@playwright/test';

const ADMIN = process.env.E2E_ADMIN_ACCOUNT || 'admin';
const ADMIN_PW = process.env.E2E_ADMIN_PASSWORD || 'admin123456';

async function loginAsAdmin(page: Page) {
  await page.goto('/login');
  await page.getByPlaceholder('admin 或 user@example.com').fill(ADMIN);
  await page.getByPlaceholder('请输入密码').fill(ADMIN_PW);
  const resp = page.waitForResponse(
    (r) => {
      try {
        return new URL(r.url()).pathname === '/api/auth/login' && r.request().method() === 'POST';
      } catch {
        return false;
      }
    },
    { timeout: 15_000 },
  );
  await page.getByRole('button', { name: '进入控制台' }).click();
  expect((await resp).status()).toBeLessThan(300);
  await expect(page).toHaveURL(/\/(?:dashboard|overview)$/);
}

test('一键安装与新增节点弹窗去除公网地址,public_host 由直连地址推导', async ({ page }) => {
  await loginAsAdmin(page);
  await page.goto('/admin/transit-nodes');

  // —— 一键安装 Agent 弹窗:不应再有「公网地址」输入,三地址仍在 ——
  await page.getByRole('button', { name: '一键安装 Agent' }).first().click();
  const oc = page.getByRole('dialog', { name: '一键安装 Agent' });
  await expect(oc).toBeVisible();
  await expect(oc.getByText('公网地址', { exact: true })).toHaveCount(0);
  await expect(oc.getByText('IP 直连地址')).toBeVisible();
  await expect(oc.getByText('CF 直连地址')).toBeVisible();
  await oc.getByRole('button', { name: '取消' }).click();
  await expect(oc).toBeHidden();

  // —— 新增中转节点弹窗:不应再有「客户连接地址」输入 ——
  await page.getByRole('button', { name: '新增中转节点' }).first().click();
  const dlg = page.getByRole('dialog', { name: '新增中转节点' });
  await expect(dlg).toBeVisible();
  await expect(dlg.getByText('客户连接地址', { exact: true })).toHaveCount(0);
  await expect(dlg.getByText('IP 直连地址')).toBeVisible();

  // 只填 名称 + IP 直连地址 + 占位鉴权码,提交并拦截真实创建请求体。
  await dlg.getByPlaceholder('例如：香港中转 01').fill('e2e-derive-nohost');
  await dlg.getByPlaceholder(/公网 IP，例如 203\.0\.113\.10/).fill('203.0.113.77');
  await dlg.getByPlaceholder('服务器安装 agent 成功后输出的节点鉴权码').fill('placeholder-token-not-real');

  const createReq = page.waitForRequest(
    (r) => {
      try {
        return new URL(r.url()).pathname === '/api/admin/access-nodes' && r.method() === 'POST';
      } catch {
        return false;
      }
    },
    { timeout: 10_000 },
  );
  await dlg.getByRole('button', { name: '保存' }).click();
  const req = await createReq;
  const body = JSON.parse(req.postData() || '{}');
  // 核心断言:public_host 自动等于唯一填写的 IP 直连地址(没有再单独填公网地址)。
  expect(body.public_host, 'public_host 应由 IP 直连地址推导').toBe('203.0.113.77');
  expect(body.ip_direct_address).toBe('203.0.113.77');
});
