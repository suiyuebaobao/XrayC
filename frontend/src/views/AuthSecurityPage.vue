<!--
  本页面用于后台配置注册、登录、验证码、邮箱验证和邀请规则。
  它保存认证安全策略，影响普通用户登录注册和管理员登录保护。
  管理员后台需要回显 SMTP 密码，便于确认当前发信配置。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type AuthSecuritySettings } from '@/services/api';

const loading = ref(true);
const saving = ref(false);

const form = reactive<AuthSecuritySettings>({
  captchaEnabled: false,
  captchaRegisterEnabled: false,
  captchaUserLoginEnabled: false,
  captchaAdminLoginEnabled: false,
  emailVerificationEnabled: false,
  inviteRequired: false,
  allowUserInviteGeneration: false,
  maxInviteCodesPerUser: 0,
  allowedEmailDomains: [],
  emailCooldownSeconds: 60,
  smtpHost: '',
  smtpPort: 587,
  smtpUsername: '',
  smtpFrom: '',
  smtpPassword: '',
  loginLockEnabled: false,
  loginFailureThreshold: 5,
  loginLockMinutes: 15,
});
const allowedEmailDomainsText = ref('');

onMounted(load);

async function load() {
  loading.value = true;
  try {
    applySettings(await apiClient.getAdminAuthSecurity());
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载认证安全配置失败');
  } finally {
    loading.value = false;
  }
}

async function save() {
  saving.value = true;
  try {
    applySettings(
      await apiClient.updateAdminAuthSecurity({
        ...form,
        allowedEmailDomains: splitAllowedEmailDomains(allowedEmailDomainsText.value),
      }),
    );
    ElMessage.success('认证安全配置已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存认证安全配置失败');
  } finally {
    saving.value = false;
  }
}

function applySettings(settings: AuthSecuritySettings) {
  form.captchaEnabled = settings.captchaEnabled;
  form.captchaRegisterEnabled = settings.captchaRegisterEnabled;
  form.captchaUserLoginEnabled = settings.captchaUserLoginEnabled;
  form.captchaAdminLoginEnabled = settings.captchaAdminLoginEnabled;
  form.emailVerificationEnabled = settings.emailVerificationEnabled;
  form.inviteRequired = settings.inviteRequired;
  form.allowUserInviteGeneration = settings.allowUserInviteGeneration;
  form.maxInviteCodesPerUser = settings.maxInviteCodesPerUser || 0;
  form.allowedEmailDomains = settings.allowedEmailDomains;
  form.emailCooldownSeconds = settings.emailCooldownSeconds || 60;
  form.smtpHost = settings.smtpHost;
  form.smtpPort = settings.smtpPort || 587;
  form.smtpUsername = settings.smtpUsername;
  form.smtpFrom = settings.smtpFrom;
  form.smtpPassword = settings.smtpPassword;
  allowedEmailDomainsText.value = settings.allowedEmailDomains.join('\n');
  form.loginLockEnabled = settings.loginLockEnabled;
  form.loginFailureThreshold = settings.loginFailureThreshold || 5;
  form.loginLockMinutes = settings.loginLockMinutes || 15;
}

function splitAllowedEmailDomains(value: string) {
  return value
    .split(/[\n,，\s]+/)
    .map((item) => item.trim().replace(/^@/, '').toLowerCase())
    .filter(Boolean);
}
</script>

<template>
  <PageHeader title="认证安全" description="配置注册、登录和验证码安全策略。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :loading="saving" @click="save">保存配置</el-button>
  </PageHeader>

  <el-card shadow="never">
    <template #header>安全策略</template>
    <el-skeleton v-if="loading" :rows="6" animated />
    <el-form v-else label-position="top" class="security-form">
      <el-row :gutter="18">
        <el-col :xs="24" :md="8">
          <el-form-item label="注册算术验证码">
            <el-switch v-model="form.captchaRegisterEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="用户登录算术验证码">
            <el-switch v-model="form.captchaUserLoginEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="8">
          <el-form-item label="管理员登录算术验证码">
            <el-switch v-model="form.captchaAdminLoginEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="邮箱验证码">
            <el-switch v-model="form.emailVerificationEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="邀请码注册">
            <el-switch v-model="form.inviteRequired" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="用户自助生成邀请码">
            <el-switch v-model="form.allowUserInviteGeneration" active-text="允许" inactive-text="禁止" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="每用户可生成邀请码数量">
            <el-input-number v-model="form.maxInviteCodesPerUser" :min="0" :max="1000" controls-position="right" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="登录失败锁定">
            <el-switch v-model="form.loginLockEnabled" active-text="开启" inactive-text="关闭" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="失败阈值">
            <el-input-number v-model="form.loginFailureThreshold" :min="1" :max="100" controls-position="right" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="锁定分钟">
            <el-input-number v-model="form.loginLockMinutes" :min="1" :max="1440" controls-position="right" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="邮箱验证码冷却秒数">
            <el-input-number v-model="form.emailCooldownSeconds" :min="10" :max="3600" controls-position="right" />
          </el-form-item>
        </el-col>
        <el-col :xs="24">
          <el-form-item label="允许注册邮箱域名">
            <el-input
              v-model="allowedEmailDomainsText"
              type="textarea"
              :rows="4"
              placeholder="每行一个域名，例如 qq.com、163.com；留空表示不限制"
            />
          </el-form-item>
        </el-col>
        <el-col :xs="24">
          <el-divider content-position="left">SMTP 发信配置</el-divider>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="SMTP 主机">
            <el-input v-model="form.smtpHost" placeholder="smtp.example.com" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="SMTP 端口">
            <el-input-number v-model="form.smtpPort" :min="1" :max="65535" controls-position="right" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="SMTP 用户名">
            <el-input v-model="form.smtpUsername" placeholder="通常为发件邮箱" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="发件人邮箱">
            <el-input v-model="form.smtpFrom" placeholder="留空使用 SMTP 用户名" />
          </el-form-item>
        </el-col>
        <el-col :xs="24" :md="12">
          <el-form-item label="SMTP 密码">
            <el-input
              v-model="form.smtpPassword"
              placeholder="当前 SMTP 密码"
              show-password
              type="password"
            />
          </el-form-item>
        </el-col>
      </el-row>
    </el-form>
  </el-card>
</template>

<style scoped>
.security-form {
  max-width: 900px;
}
</style>
