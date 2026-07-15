// 本文件维护认证、安全设置和用户邀请码相关 API 方法。
// 它只组织请求参数和调用认证 normalizer，不保存浏览器会话。
// register/login 的 token 写入由调用方的 session store 负责。
// 新增登录安全接口时，应放在这里而不是页面组件里。

import type {
  AdminInviteCode,
  AuthSecuritySettings,
  CaptchaChallenge,
  ChangePasswordPayload,
  CreateAdminInviteCodesPayload,
  EmailCodeChallenge,
  LoginPayload,
  RegisterPayload,
  UserInviteCodesInfo,
} from '../types';
import { request } from '../http';
import { isRecord } from '../primitives';
import {
  authSecurityPayload,
  normalizeAuthSecurity,
  normalizeAdminInviteCodes,
  normalizeCaptchaChallenge,
  normalizeEmailCodeChallenge,
  normalizeSession,
  normalizeUserInviteCodes,
} from '../normalizers/auth';
import { plannedApiPaths, rustApiPaths } from '../paths';

export const authApi = {
  async register(payload: RegisterPayload) {
    const data = await request<Record<string, unknown>>(rustApiPaths.register, {
      method: 'POST',
      body: JSON.stringify({
        email: payload.email,
        password: payload.password,
        invite_code: payload.inviteCode || undefined,
        captcha_id: payload.captchaId || undefined,
        captcha_answer: payload.captchaAnswer || undefined,
        email_code_id: payload.emailCodeId || undefined,
        email_code: payload.emailCode || undefined,
      }),
    });
    return normalizeSession(data, payload.email);
  },

  async login(payload: LoginPayload) {
    const data = await request<Record<string, unknown>>(rustApiPaths.login, {
      method: 'POST',
      body: JSON.stringify({
        account: payload.account,
        password: payload.password,
        captcha_id: payload.captchaId || undefined,
        captcha_answer: payload.captchaAnswer || undefined,
      }),
    });
    return normalizeSession(data, payload.account);
  },

  async createCaptcha(scene: string, target: string): Promise<CaptchaChallenge> {
    const params = new URLSearchParams({ scene, target });
    const data = await request<Record<string, unknown>>(`${rustApiPaths.captcha}?${params.toString()}`);
    return normalizeCaptchaChallenge(data);
  },

  async sendEmailCode(email: string): Promise<EmailCodeChallenge> {
    const data = await request<Record<string, unknown>>(rustApiPaths.emailCode, {
      method: 'POST',
      body: JSON.stringify({ email }),
    });
    return normalizeEmailCodeChallenge(data);
  },

  // 已登录用户请求改密邮箱验证码：发到注册邮箱，邮箱由后端取自 JWT，无需传参。
  async sendChangePasswordCode(): Promise<EmailCodeChallenge> {
    const data = await request<Record<string, unknown>>(rustApiPaths.changePasswordSendCode, {
      method: 'POST',
    });
    return normalizeEmailCodeChallenge(data);
  },

  // 已登录用户提交邮箱验证码 + 新密码完成改密；成功后后端会撤销全部刷新令牌。
  async changePassword(payload: ChangePasswordPayload): Promise<void> {
    await request<unknown>(rustApiPaths.changePassword, {
      method: 'POST',
      body: JSON.stringify({
        email_code_id: payload.emailCodeId || undefined,
        email_code: payload.emailCode || undefined,
        new_password: payload.newPassword,
      }),
    });
  },

  async refresh() {
    const data = await request<Record<string, unknown>>(rustApiPaths.refresh, { method: 'POST' });
    return normalizeSession(data, '');
  },

  async logout() {
    await request<unknown>(rustApiPaths.logout, { method: 'POST' });
  },

  async getAuthSecurity(): Promise<AuthSecuritySettings> {
    const data = await request<Record<string, unknown>>(rustApiPaths.authSecurity);
    return normalizeAuthSecurity(data);
  },

  async getAdminAuthSecurity(): Promise<AuthSecuritySettings> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminAuthSecurity);
    return normalizeAuthSecurity(data);
  },

  async updateAdminAuthSecurity(payload: AuthSecuritySettings): Promise<AuthSecuritySettings> {
    const data = await request<unknown>(plannedApiPaths.adminAuthSecurity, {
      method: 'PUT',
      body: JSON.stringify(authSecurityPayload(payload)),
    });
    return isRecord(data) ? normalizeAuthSecurity(data) : payload;
  },

  async getUserInviteCodes(): Promise<UserInviteCodesInfo> {
    const data = await request<Record<string, unknown>>(rustApiPaths.userInviteCodes);
    return normalizeUserInviteCodes(data);
  },

  async createUserInviteCode(): Promise<UserInviteCodesInfo> {
    const data = await request<Record<string, unknown>>(rustApiPaths.userInviteCodes, { method: 'POST' });
    return normalizeUserInviteCodes(data);
  },

  async getAdminInviteCodes(): Promise<AdminInviteCode[]> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminInviteCodes);
    return normalizeAdminInviteCodes(data);
  },

  async createAdminInviteCodes(payload: CreateAdminInviteCodesPayload): Promise<AdminInviteCode[]> {
    const data = await request<Record<string, unknown>>(plannedApiPaths.adminInviteCodes, {
      method: 'POST',
      body: JSON.stringify({ count: payload.count }),
    });
    return normalizeAdminInviteCodes(data);
  },

  async deleteAdminInviteCode(code: string): Promise<void> {
    await request<unknown>(`${plannedApiPaths.adminInviteCodes}/${encodeURIComponent(code)}`, {
      method: 'DELETE',
    });
  },
};
