// 本文件封装前端 API 的 HTTP 请求、CSRF 头和会话刷新逻辑。
// 它只处理浏览器请求与本地 session 同步，不包含业务字段标准化。
// 认证失败时会尝试刷新 token，刷新失败则清理本地登录态。
// 业务客户端统一调用 request，避免各页面重复处理鉴权细节。

import type { SessionUser } from './types';
import { isRecord, recordValue, stringValue } from './primitives';
import { normalizeSession } from './normalizers/auth';
import { rustApiPaths } from './paths';

const SESSION_STORAGE_KEY = 'xrayc.session';
const CSRF_HEADER_NAME = 'X-XrayC-CSRF';
const CSRF_HEADER_VALUE = '1';
const CSRF_SAFE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);

export async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  return requestWithAuth<T>(path, init, true);
}

async function requestWithAuth<T>(path: string, init: RequestInit, allowRefresh: boolean): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set('Accept', 'application/json');

  if (init.body && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  if (requiresCsrfHeader(init)) {
    headers.set(CSRF_HEADER_NAME, CSRF_HEADER_VALUE);
  }

  const token = readStoredToken();
  if (token) {
    headers.set('Authorization', `Bearer ${token}`);
  }

  const response = await fetch(path, {
    credentials: 'include',
    ...init,
    headers,
  });

  if (!response.ok) {
    if (response.status === 401 && allowRefresh && shouldAttemptTokenRefresh(path)) {
      const refreshedToken = await refreshStoredSession();
      if (refreshedToken) {
        return requestWithAuth<T>(path, init, false);
      }
    }
    throw new Error(await errorMessage(response));
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const raw = (await response.json()) as unknown;
  return unwrap(raw) as T;
}

function readStoredToken() {
  const raw = localStorage.getItem(SESSION_STORAGE_KEY);
  if (!raw) {
    return '';
  }

  try {
    const session = JSON.parse(raw) as unknown;
    return isRecord(session) ? stringValue(session.accessToken ?? session.access_token ?? session.token) : '';
  } catch {
    return '';
  }
}

function shouldAttemptTokenRefresh(path: string) {
  return path !== rustApiPaths.login
    && path !== rustApiPaths.register
    && path !== rustApiPaths.refresh
    && path !== rustApiPaths.logout;
}

function requiresCsrfHeader(init: RequestInit) {
  const method = (init.method ?? 'GET').toUpperCase();
  return !CSRF_SAFE_METHODS.has(method);
}

async function refreshStoredSession() {
  const stored = readStoredSessionRecord();
  if (!stored) {
    return '';
  }

  const response = await fetch(rustApiPaths.refresh, {
    method: 'POST',
    credentials: 'include',
    headers: {
      Accept: 'application/json',
      [CSRF_HEADER_NAME]: CSRF_HEADER_VALUE,
    },
  });
  if (!response.ok) {
    clearStoredSession();
    return '';
  }

  const raw = unwrap((await response.json()) as unknown);
  if (!isRecord(raw)) {
    clearStoredSession();
    return '';
  }

  const storedUser = recordValue(stored.user);
  const fallbackAccount = stringValue(storedUser.account ?? storedUser.email ?? storedUser.username);
  const session = normalizeSession(raw, fallbackAccount);
  if (!session.accessToken) {
    clearStoredSession();
    return '';
  }
  localStorage.setItem(SESSION_STORAGE_KEY, JSON.stringify(session));
  dispatchSessionEvent(session);
  return session.accessToken;
}

function readStoredSessionRecord() {
  const raw = localStorage.getItem(SESSION_STORAGE_KEY);
  if (!raw) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as unknown;
    return isRecord(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function clearStoredSession() {
  localStorage.removeItem(SESSION_STORAGE_KEY);
  dispatchSessionEvent(null);
}

function dispatchSessionEvent(session: { accessToken: string; user: SessionUser } | null) {
  window.dispatchEvent(new CustomEvent('xrayc:session-sync', { detail: session }));
}

function unwrap(raw: unknown) {
  if (!isRecord(raw)) {
    return raw;
  }

  if ('data' in raw) {
    return raw.data;
  }

  return raw;
}

async function errorMessage(response: Response) {
  try {
    const body = (await response.json()) as unknown;
    if (isRecord(body)) {
      return stringValue(body.message ?? body.error) || `请求失败：${response.status}`;
    }
  } catch {
    // 错误响应不是 JSON 时，回退到 HTTP 状态文本。
  }

  return response.statusText || `请求失败：${response.status}`;
}
