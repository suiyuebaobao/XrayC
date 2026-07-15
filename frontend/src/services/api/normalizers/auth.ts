// 本文件负责认证、安全设置、验证码和邀请信息的响应标准化。
// 它把后端返回的认证字段转换成前端稳定类型，避免页面直接读 snake_case。
// 它保留后端安全配置字段原值，管理员后台可直接读取真实 SMTP 配置。
// 本文件不读写 localStorage，不发送网络请求，只处理纯数据转换。
// 后续认证页面新增字段时，应优先在这里补齐兼容映射。

import type {
  AdminInviteCode,
  AuthSecuritySettings,
  CaptchaChallenge,
  EmailCodeChallenge,
  SessionUser,
  UserInviteCode,
  UserInviteCodesInfo,
} from '../types';
import {
  arrayValue,
  booleanValue,
  idValue,
  numberValue,
  recordValue,
  stringArrayValue,
  stringValue,
} from '../primitives';

export function normalizeSession(data: Record<string, unknown>, fallbackAccount: string) {
  return {
    accessToken: stringValue(data.accessToken ?? data.access_token ?? data.token),
    user: normalizeUser(recordValue(data.user), fallbackAccount),
  };
}

export function normalizeAuthSecurity(data: Record<string, unknown>): AuthSecuritySettings {
  const captcha = recordValue(data.captcha);
  const email = recordValue(data.emailVerification ?? data.email_verification);
  const guard = recordValue(data.loginGuard ?? data.login_guard);
  const registerCaptcha = booleanValue(captcha.registerEnabled ?? captcha.register_enabled);
  const userLoginCaptcha = booleanValue(captcha.userLoginEnabled ?? captcha.user_login_enabled);
  const adminLoginCaptcha = booleanValue(captcha.adminLoginEnabled ?? captcha.admin_login_enabled);

  return {
    captchaEnabled: booleanValue(data.captchaEnabled ?? data.captcha_enabled)
      || registerCaptcha
      || userLoginCaptcha
      || adminLoginCaptcha,
    captchaRegisterEnabled: registerCaptcha,
    captchaUserLoginEnabled: userLoginCaptcha,
    captchaAdminLoginEnabled: adminLoginCaptcha,
    emailVerificationEnabled: booleanValue(
      data.emailVerificationEnabled ?? data.email_verification_enabled ?? email.enabled,
    ),
    inviteRequired: booleanValue(data.inviteRequired ?? data.invite_required ?? data.requireInviteCode ?? data.require_invite_code),
    allowUserInviteGeneration: booleanValue(
      data.allowUserInviteGeneration ?? data.allow_user_invite_generation,
    ),
    maxInviteCodesPerUser: numberValue(data.maxInviteCodesPerUser ?? data.max_invite_codes_per_user),
    allowedEmailDomains: stringArrayValue(email.allowedDomains ?? email.allowed_domains),
    emailCooldownSeconds: numberValue(email.cooldownSeconds ?? email.cooldown_seconds) || 60,
    smtpHost: stringValue(email.smtpHost ?? email.smtp_host),
    smtpPort: numberValue(email.smtpPort ?? email.smtp_port) || 587,
    smtpUsername: stringValue(email.smtpUsername ?? email.smtp_username),
    smtpFrom: stringValue(email.smtpFrom ?? email.smtp_from),
    smtpPassword: stringValue(email.smtpPassword ?? email.smtp_password),
    loginLockEnabled: booleanValue(data.loginLockEnabled ?? data.login_lock_enabled ?? guard.enabled),
    loginFailureThreshold: numberValue(
      data.loginFailureThreshold ?? data.login_failure_threshold ?? guard.failureThreshold ?? guard.failure_threshold,
    ),
    loginLockMinutes: numberValue(data.loginLockMinutes ?? data.login_lock_minutes ?? guard.lockMinutes ?? guard.lock_minutes),
  };
}

export function authSecurityPayload(payload: AuthSecuritySettings) {
  return {
    require_invite_code: payload.inviteRequired,
    allow_user_invite_generation: payload.allowUserInviteGeneration,
    max_invite_codes_per_user: payload.maxInviteCodesPerUser,
    captcha: {
      register_enabled: payload.captchaRegisterEnabled,
      user_login_enabled: payload.captchaUserLoginEnabled,
      admin_login_enabled: payload.captchaAdminLoginEnabled,
      ttl_seconds: 60,
    },
    email_verification: {
      enabled: payload.emailVerificationEnabled,
      allowed_domains: payload.allowedEmailDomains,
      cooldown_seconds: payload.emailCooldownSeconds,
      smtp_host: payload.smtpHost,
      smtp_port: payload.smtpPort,
      smtp_username: payload.smtpUsername,
      smtp_from: payload.smtpFrom,
      smtp_password: payload.smtpPassword || undefined,
    },
    login_guard: {
      enabled: payload.loginLockEnabled,
      failure_threshold: payload.loginFailureThreshold,
      lock_minutes: payload.loginLockMinutes,
    },
  };
}

export function normalizeCaptchaChallenge(data: Record<string, unknown>): CaptchaChallenge {
  return {
    id: stringValue(data.id),
    scene: stringValue(data.scene),
    question: stringValue(data.question),
    expiresIn: numberValue(data.expiresIn ?? data.expires_in) || 60,
  };
}

export function normalizeEmailCodeChallenge(data: Record<string, unknown>): EmailCodeChallenge {
  return {
    id: stringValue(data.id),
    expiresIn: numberValue(data.expiresIn ?? data.expires_in) || 60,
    cooldownSeconds: numberValue(data.cooldownSeconds ?? data.cooldown_seconds) || 60,
  };
}

export function normalizeUserInviteCodes(data: Record<string, unknown>): UserInviteCodesInfo {
  const rawItems = arrayValue(data.items ?? data.inviteCodes ?? data.invite_codes);
  const item = recordValue(data.item);
  const items = rawItems.length > 0 ? rawItems : item.code ? [item] : [];

  return {
    enabled: booleanValue(data.enabled ?? data.allowUserInviteGeneration ?? data.allow_user_invite_generation),
    maxCount: numberValue(data.maxCount ?? data.max_count),
    generatedCount: numberValue(data.generatedCount ?? data.generated_count ?? items.length),
    remainingCount: numberValue(data.remainingCount ?? data.remaining_count),
    items: items.map(normalizeUserInviteCode),
  };
}

export function normalizeAdminInviteCodes(data: Record<string, unknown>): AdminInviteCode[] {
  return arrayValue(data.items ?? data.inviteCodes ?? data.invite_codes).map(normalizeAdminInviteCode);
}

function normalizeUserInviteCode(value: unknown): UserInviteCode {
  const data = recordValue(value);
  return {
    code: stringValue(data.code),
    isUsed: booleanValue(data.isUsed ?? data.is_used),
    usedByUserId: stringValue(data.usedByUserId ?? data.used_by_user_id),
    usedByEmail: stringValue(data.usedByEmail ?? data.used_by_email ?? data.used_by_email_snapshot),
    usedAt: stringValue(data.usedAt ?? data.used_at),
    createdAt: stringValue(data.createdAt ?? data.created_at),
  };
}

function normalizeAdminInviteCode(value: unknown): AdminInviteCode {
  const data = recordValue(value);
  return {
    ...normalizeUserInviteCode(data),
    inviterUserId: stringValue(data.inviterUserId ?? data.inviter_user_id),
    inviterEmail: stringValue(data.inviterEmail ?? data.inviter_email ?? data.inviter_email_snapshot),
  };
}

function normalizeUser(data: Record<string, unknown>, fallbackAccount: string): SessionUser {
  const account = stringValue(data.account ?? data.email ?? data.username) || fallbackAccount;

  return {
    id: idValue(data.id),
    name: stringValue(data.name ?? data.display_name ?? data.email ?? data.username) || account,
    account,
    role: data.role === 'admin' || data.is_admin === true ? 'admin' : 'user',
  };
}
