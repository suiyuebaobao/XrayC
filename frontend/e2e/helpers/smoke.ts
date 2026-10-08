// 用途：提供 smoke 测试共享的登录、环境变量、弹窗和管理员会话工具。
import { expect, type Page } from '@playwright/test';

export const smokeUserAccount = envValue('E2E_USER_ACCOUNT', 'SMOKE_LOGIN_ACCOUNT') || 'demo@example.test';
export const smokeUserPassword = envValue('E2E_USER_PASSWORD', 'SMOKE_LOGIN_PASSWORD') || 'demo123456';
export const smokeAdminAccount = envValue('E2E_ADMIN_ACCOUNT', 'ADMIN_LOGIN_ACCOUNT') || 'admin@example.test';
export const smokeAdminPassword = envValue('E2E_ADMIN_PASSWORD', 'ADMIN_LOGIN_PASSWORD') || 'admin123456';
export const requireRealAccessEndpoint = envValue('E2E_REQUIRE_REAL_ACCESS_ENDPOINT') === '1';

export async function login(page: Page, account: string, password: string) {
  await page.goto('/login');
  await page.getByPlaceholder('admin 或 user@example.com').fill(account);
  await page.getByPlaceholder('请输入密码').fill(password);
  const loginResponse = page.waitForResponse(
    (response) => {
      try {
        return new URL(response.url()).pathname === '/api/auth/login'
          && response.request().method() === 'POST';
      } catch {
        return false;
      }
    },
    { timeout: 10_000 },
  );
  await page.getByRole('button', { name: '进入控制台' }).click();
  const response = await loginResponse;
  expect(response.status(), 'login response status').toBeGreaterThanOrEqual(200);
  expect(response.status(), 'login response status').toBeLessThan(300);
  await expect(page).toHaveURL(/\/(?:dashboard|overview)$/);
}

export async function expectDialogOpensAndCloses(page: Page, buttonName: string, dialogName: string | RegExp) {
  await page.getByRole('button', { name: buttonName, exact: true }).first().click();
  const dialog = page.getByRole('dialog', { name: dialogName });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: '取消' }).click();
  await expect(dialog).toBeHidden();
}

export async function mockAdminSession(page: Page) {
  // 節點與入口頁共同載入這兩個集合；各用例可再註冊路由覆寫預設資料。
  // 避免 placeholder JWT 落到真實後端而使 Promise.all 整批載入失敗。
  for (const [path, data] of [
    ['access-entries', { access_entries: [] }],
    ['exit-resources', { exit_resources: [] }],
  ] as const) {
    await page.route(`**/api/admin/${path}`, async (route) => {
      if (route.request().method() !== 'GET') {
        await route.fallback();
        return;
      }
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({ success: true, data }),
      });
    });
  }
  await page.addInitScript(() => {
    window.localStorage.setItem(
      'xrayc.session',
      JSON.stringify({
        accessToken: 'placeholder',
        user: {
          id: 1,
          account: 'admin',
          name: 'admin',
          role: 'admin',
        },
      }),
    );
  });
  await page.route('**/api/auth/security', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { captcha_enabled: false } }),
    });
  });
  await page.route('**/api/auth/login', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_token: 'placeholder',
          user: {
            id: 1,
            account: 'admin',
            role: 'admin',
          },
        },
      }),
    });
  });
  await page.route('**/api/auth/refresh', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_token: 'placeholder',
          user: {
            id: 1,
            account: 'admin',
            role: 'admin',
          },
        },
      }),
    });
  });
}

export async function mockUserSession(page: Page) {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      'xrayc.session',
      JSON.stringify({
        accessToken: 'placeholder',
        user: {
          id: 1001,
          account: 'demo@example.test',
          name: 'Demo User',
          role: 'user',
        },
      }),
    );
  });
  await page.route('**/api/auth/security', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ success: true, data: { captcha_enabled: false } }),
    });
  });
  await page.route('**/api/auth/login', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_token: 'placeholder',
          user: {
            id: 1001,
            account: 'demo@example.test',
            role: 'user',
          },
        },
      }),
    });
  });
  await page.route('**/api/auth/refresh', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        success: true,
        data: {
          access_token: 'placeholder',
          user: {
            id: 1001,
            account: 'demo@example.test',
            role: 'user',
          },
        },
      }),
    });
  });
}

function envValue(...names: string[]) {
  for (const name of names) {
    const value = process.env[name]?.trim();
    if (value) {
      return value;
    }
  }
  return '';
}
