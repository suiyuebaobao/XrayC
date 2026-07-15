// 本文件维护前端登录会话状态。
// 它封装登录、注册、刷新、退出和本地 session 持久化。
// 页面通过该 store 判断角色和访问权限。
// 真实鉴权仍以后端 JWT 与 refresh cookie 为准。
import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { apiClient, type LoginPayload, type RegisterPayload, type SessionUser } from '@/services/api';

const STORAGE_KEY = 'xrayc.session';

export const useSessionStore = defineStore('session', () => {
  const storedSession = readStoredSession();
  const user = ref<SessionUser | null>(storedSession.user);
  const token = ref<string | null>(storedSession.accessToken);

  const isAuthenticated = computed(() => Boolean(user.value && token.value));
  const isAdmin = computed(() => user.value?.role === 'admin');

  async function login(payload: LoginPayload) {
    const result = await apiClient.login(payload);
    persistSession(result);
  }

  async function register(payload: RegisterPayload) {
    const result = await apiClient.register(payload);
    persistSession(result);
  }

  async function refresh() {
    const result = await apiClient.refresh();
    persistSession(result);
  }

  async function logout() {
    try {
      await apiClient.logout();
    } finally {
      user.value = null;
      token.value = null;
      localStorage.removeItem(STORAGE_KEY);
    }
  }

  function persistSession(result: { accessToken: string; user: SessionUser }) {
    user.value = result.user;
    token.value = result.accessToken;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(result));
  }

  if (typeof window !== 'undefined') {
    window.addEventListener('xrayc:session-sync', () => {
      const latest = readStoredSession();
      user.value = latest.user;
      token.value = latest.accessToken;
    });
  }

  return {
    user,
    token,
    isAuthenticated,
    isAdmin,
    login,
    register,
    refresh,
    logout,
  };
});

function readStoredSession(): { accessToken: string | null; user: SessionUser | null } {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) {
    return { accessToken: null, user: null };
  }

  try {
    const parsed = JSON.parse(raw) as Partial<{ accessToken: string; token: string; user: SessionUser }>;

    if (parsed.user) {
      return { accessToken: parsed.accessToken || parsed.token || null, user: parsed.user };
    }

    return { accessToken: null, user: parsed as SessionUser };
  } catch {
    localStorage.removeItem(STORAGE_KEY);
    return { accessToken: null, user: null };
  }
}
