/*
 * 用途：集中管理 runtime no-mock e2e 的 API 路径、登录和响应等待工具。
 * 本文件不修改业务实现，只为拆分 Playwright 测试流程复用逻辑。
 */
import { expect, type Page, type Response, test } from '@playwright/test';

import { isRecord } from './runtime-values';

export const API = {
  login: '/api/auth/login',
  authSecurity: '/api/auth/security',
  salesLanding: '/api/sales-landing',
  userSubscription: '/api/user/subscription',
  userPlans: '/api/plans',
  userOrders: '/api/orders',
  userInviteCodes: '/api/user/invite-codes',
  operationsSummary: '/api/admin/access-operations/summary',
  accessRouting: '/api/admin/access-routing',
  exitResources: '/api/admin/exit-resources',
  exitEndpoints: '/api/admin/exit-endpoints',
  exitPools: '/api/admin/exit-pools',
  adminPlans: '/api/admin/plans',
  adminUsers: '/api/admin/users',
  adminOrders: '/api/admin/orders',
  adminRedeemCodes: '/api/admin/redeem-codes',
  adminAuditLogs: '/api/admin/audit-logs',
  adminSubscriptionSettings: '/api/admin/subscription-settings',
  adminAuthSecurity: '/api/admin/auth-security',
  adminSalesLanding: '/api/admin/sales-landing',
} as const;

export type ApiPath = typeof API[keyof typeof API];

type HttpMethod = 'GET' | 'POST';

export type ApiStatusCollector = {
  assert2xx(path: ApiPath): void;
  statuses(path: ApiPath): number[];
};

export const requiredUserApiPaths = [
  API.login,
  API.userSubscription,
  API.userPlans,
  API.userOrders,
  API.userInviteCodes,
] as const satisfies readonly ApiPath[];

export const requiredAdminApiPaths = [
  API.login,
  API.operationsSummary,
  API.accessRouting,
  API.exitResources,
  API.exitEndpoints,
  API.adminPlans,
  API.adminUsers,
  API.adminOrders,
  API.adminRedeemCodes,
  API.adminAuditLogs,
  API.adminSubscriptionSettings,
  API.adminAuthSecurity,
  API.adminSalesLanding,
] as const satisfies readonly ApiPath[];

export const runtimeAuth = {
  userAccount: envValue('E2E_USER_ACCOUNT', 'E2E_NO_MOCK_USER_ACCOUNT', 'SMOKE_LOGIN_ACCOUNT'),
  userPassword: envValue('E2E_USER_PASSWORD', 'E2E_NO_MOCK_USER_PASSWORD', 'SMOKE_LOGIN_PASSWORD'),
  adminAccount: envValue('E2E_ADMIN_ACCOUNT', 'E2E_NO_MOCK_ADMIN_ACCOUNT', 'ADMIN_LOGIN_ACCOUNT'),
  adminPassword: envValue('E2E_ADMIN_PASSWORD', 'E2E_NO_MOCK_ADMIN_PASSWORD', 'ADMIN_LOGIN_PASSWORD'),
};

const requireNoSkippedRuntimeTests = process.env.E2E_REQUIRE_NO_SKIP === '1'
  || process.env.XRAYC_REAL_RELEASE === '1';

export async function login(
  page: Page,
  loginPath: string,
  account: string,
  password: string,
  expectedUrl: RegExp,
) {
  const securityResponse = waitForApiResponse(page, API.authSecurity, { timeout: 10_000 }).catch(() => null);
  await page.goto(loginPath);
  await expect(page.getByRole('heading', { name: '登录控制台' })).toBeVisible();
  await securityResponse;

  await page.getByPlaceholder('admin 或 user@example.com').fill(account);
  await page.getByPlaceholder('请输入密码').fill(password);
  await solveCaptchaIfShown(page);

  const loginResponse = waitForApiResponse(page, API.login, { method: 'POST' });
  await page.getByRole('button', { name: '进入控制台' }).click();
  await loginResponse;
  await expect(page).toHaveURL(expectedUrl);
}

export function skipOrFailMissingCredentials(hasCredentials: boolean, message: string) {
  if (hasCredentials) {
    return;
  }
  if (requireNoSkippedRuntimeTests) {
    throw new Error(message);
  }
  test.skip(true, message);
}

export async function gotoAndWaitForApi(page: Page, path: string, apiPath: ApiPath) {
  const response = waitForApiResponse(page, apiPath);
  await page.goto(path);
  return response;
}

export async function gotoAndWaitForApis(page: Page, path: string, apiPaths: readonly ApiPath[]) {
  const responses = Promise.all(apiPaths.map((apiPath) => waitForApiResponse(page, apiPath)));
  await page.goto(path);
  return responses;
}

export async function waitForApiResponse(
  page: Page,
  path: ApiPath,
  options: { method?: HttpMethod; timeout?: number } = {},
) {
  const method = options.method ?? 'GET';
  const response = await page.waitForResponse(
    async (candidate) => {
      if (apiPathname(candidate) !== path || candidate.request().method() !== method) {
        return false;
      }
      // 導覽時舊頁面的輪詢可能已收到標頭、卻被取消而沒有本文。
      // 等待可讀取的完整回應，避免把空資料誤判成後端缺少欄位。
      try {
        await candidate.body();
        return true;
      } catch {
        return false;
      }
    },
    { timeout: options.timeout ?? 15_000 },
  );
  expect(response.status(), `${method} ${path} response status`).toBeGreaterThanOrEqual(200);
  expect(response.status(), `${method} ${path} response status`).toBeLessThan(300);
  return response;
}

export function collectApiStatuses(page: Page, paths: readonly ApiPath[]): ApiStatusCollector {
  const watched = new Set(paths);
  const statusMap = new Map<ApiPath, number[]>(paths.map((path) => [path, []]));

  page.on('response', (response) => {
    const path = apiPathname(response);
    if (!watched.has(path as ApiPath)) {
      return;
    }
    statusMap.get(path as ApiPath)?.push(response.status());
  });

  return {
    statuses(path: ApiPath) {
      return statusMap.get(path) ?? [];
    },
    assert2xx(path: ApiPath) {
      const statuses = statusMap.get(path) ?? [];
      expect(statuses.length, `${path} response statuses`).toBeGreaterThan(0);
      expect(statuses.some((status) => status >= 200 && status < 300), `${path} statuses: ${statuses.join(', ')}`)
        .toBeTruthy();
    },
  };
}

export async function responseData(response: Response) {
  const raw = await response.json();
  return unwrap(raw);
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

async function solveCaptchaIfShown(page: Page) {
  const captchaButton = page.getByRole('button', { name: /获取验证码|刷新验证码/ });
  const visible = await captchaButton
    .waitFor({ state: 'visible', timeout: 1_500 })
    .then(() => true)
    .catch(() => false);
  if (!visible) {
    return;
  }

  await captchaButton.click();
  const challenge = page.getByText(/\d+\s*(?:\+|-|x|X|\*|×)\s*\d+/).first();
  await expect(challenge).toBeVisible();
  const answer = solveArithmetic(await challenge.textContent());
  await page.getByPlaceholder('请输入计算结果').fill(String(answer));
}

function solveArithmetic(text: string | null) {
  const match = (text ?? '').match(/(\d+)\s*(\+|-|x|X|\*|×)\s*(\d+)/);
  if (!match) {
    throw new Error(`无法解析验证码问题：${text ?? ''}`);
  }
  const left = Number(match[1]);
  const right = Number(match[3]);
  if (match[2] === '-') {
    return left - right;
  }
  if (match[2] === '+') {
    return left + right;
  }
  return left * right;
}

function apiPathname(response: Response) {
  try {
    return new URL(response.url()).pathname;
  } catch {
    return '';
  }
}

function unwrap(value: unknown): unknown {
  if (isRecord(value) && 'data' in value) {
    return value.data;
  }
  return value;
}
