// 本文件定义认证、安全和邀请相关的前端 API 类型。
// 这些类型描述登录、注册、验证码、邮箱验证码和后台认证安全设置。
// 文件只包含类型定义，不发送请求、不处理 token，也不写入浏览器状态。
// 其它模块通过 services/api/types 统一重新导出这些类型。

export type LoginPayload = {
  account: string;
  password: string;
  captchaId?: string;
  captchaAnswer?: string;
};

export type RegisterPayload = {
  email: string;
  password: string;
  inviteCode?: string;
  captchaId?: string;
  captchaAnswer?: string;
  emailCodeId?: string;
  emailCode?: string;
};

export type SessionUser = {
  id: number | string;
  name: string;
  account: string;
  role: 'admin' | 'user';
};

export type AuthSecuritySettings = {
  captchaEnabled: boolean;
  captchaRegisterEnabled: boolean;
  captchaUserLoginEnabled: boolean;
  captchaAdminLoginEnabled: boolean;
  emailVerificationEnabled: boolean;
  inviteRequired: boolean;
  allowUserInviteGeneration: boolean;
  maxInviteCodesPerUser: number;
  allowedEmailDomains: string[];
  emailCooldownSeconds: number;
  smtpHost: string;
  smtpPort: number;
  smtpUsername: string;
  smtpFrom: string;
  smtpPassword: string;
  loginLockEnabled: boolean;
  loginFailureThreshold: number;
  loginLockMinutes: number;
};

export type CaptchaChallenge = {
  id: string;
  scene: string;
  question: string;
  expiresIn: number;
};

export type EmailCodeChallenge = {
  id: string;
  expiresIn: number;
  cooldownSeconds: number;
};

// 已登录用户改密：提交邮箱验证码 + 新密码，邮箱以登录身份为准不在此传入。
export type ChangePasswordPayload = {
  emailCodeId: string;
  emailCode: string;
  newPassword: string;
};

export type UserInviteCode = {
  code: string;
  isUsed: boolean;
  usedByUserId: string;
  usedByEmail: string;
  usedAt: string;
  createdAt: string;
};

export type UserInviteCodesInfo = {
  enabled: boolean;
  maxCount: number;
  generatedCount: number;
  remainingCount: number;
  items: UserInviteCode[];
};

export type AdminInviteCode = UserInviteCode & {
  inviterUserId: string;
  inviterEmail: string;
};

export type CreateAdminInviteCodesPayload = {
  count: number;
};
