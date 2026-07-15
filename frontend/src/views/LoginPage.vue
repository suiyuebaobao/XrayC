<!--
  本页面用于普通用户和管理员登录。
  它根据后台安全策略决定是否展示验证码。
  登录成功后只把后端返回的会话交给 session store。
-->
<script setup lang="ts">
import { Lock, User } from '@element-plus/icons-vue';
import { ElMessage, type FormInstance, type FormRules } from 'element-plus';
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { apiClient, type AuthSecuritySettings, type CaptchaChallenge } from '@/services/api';
import { useSessionStore } from '@/stores/session';

const route = useRoute();
const router = useRouter();
const session = useSessionStore();
const formRef = ref<FormInstance>();
const loading = ref(false);

const form = reactive({
  account: '',
  password: '',
  captchaAnswer: '',
  captchaId: '',
});
const security = ref<AuthSecuritySettings | null>(null);
const captcha = ref<CaptchaChallenge | null>(null);
const captchaLoading = ref(false);
const adminLoginPath = computed(() => route.path.startsWith('/admin'));
const loginScene = computed(() =>
  adminLoginPath.value || form.account.trim().toLowerCase() === 'admin' ? 'admin_login' : 'login',
);
const captchaRequired = computed(() => {
  if (loginScene.value === 'admin_login') {
    return Boolean(security.value?.captchaAdminLoginEnabled);
  }
  return Boolean(security.value?.captchaUserLoginEnabled);
});

const rules: FormRules<typeof form> = {
  account: [{ required: true, message: '请输入邮箱或管理员账号', trigger: 'blur' }],
  password: [{ required: true, message: '请输入密码', trigger: 'blur' }],
  captchaAnswer: [
    {
      validator: (_rule, value, callback) => {
        if (captchaRequired.value && !String(value || '').trim()) {
          callback(new Error('请输入算术验证码'));
        } else {
          callback();
        }
      },
      trigger: 'blur',
    },
  ],
};

onMounted(async () => {
  try {
    security.value = await apiClient.getAuthSecurity();
  } catch {
    security.value = null;
  }
});

watch(
  () => [form.account, loginScene.value],
  () => {
    captcha.value = null;
    form.captchaId = '';
    form.captchaAnswer = '';
  },
);

async function submit() {
  await formRef.value?.validate();
  if (captchaRequired.value && !form.captchaId) {
    await loadCaptcha();
    ElMessage.warning('请填写算术验证码后再登录');
    return;
  }
  loading.value = true;
  try {
    await session.login({
      account: form.account,
      password: form.password,
      captchaId: form.captchaId,
      captchaAnswer: form.captchaAnswer,
    });
    ElMessage.success('登录成功');
    const defaultPath = session.isAdmin ? '/overview' : '/dashboard';
    router.push(String(route.query.redirect || defaultPath));
  } catch (error) {
    if (captchaRequired.value) {
      await loadCaptcha();
    }
    ElMessage.error(error instanceof Error ? error.message : '登录失败，请检查账号、密码和安全验证');
  } finally {
    loading.value = false;
  }
}

async function loadCaptcha() {
  if (!form.account.trim()) {
    ElMessage.warning('请先输入账号');
    return;
  }
  captchaLoading.value = true;
  try {
    captcha.value = await apiClient.createCaptcha(loginScene.value, form.account);
    form.captchaId = captcha.value.id;
    form.captchaAnswer = '';
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '获取验证码失败');
  } finally {
    captchaLoading.value = false;
  }
}
</script>

<template>
  <main class="login-page">
    <section class="login-hero">
      <p class="eyebrow">V2 control plane</p>
      <h1>中转节点、分组、套餐授权统一管理。</h1>
      <p>控制台聚焦认证、订阅、出口线路和中转入口授权。</p>
    </section>

    <el-card class="login-card" shadow="never">
      <template #header>
        <div>
          <h2>登录控制台</h2>
          <span>管理员可输入 admin，用户可输入邮箱。</span>
        </div>
      </template>

      <el-form ref="formRef" :model="form" :rules="rules" label-position="top" @keyup.enter="submit">
        <el-form-item label="账号" prop="account">
          <el-input v-model="form.account" :prefix-icon="User" placeholder="admin 或 user@example.com" />
        </el-form-item>
        <el-form-item label="密码" prop="password">
          <el-input v-model="form.password" :prefix-icon="Lock" placeholder="请输入密码" show-password />
        </el-form-item>
        <el-form-item v-if="captchaRequired" label="算术验证码" prop="captchaAnswer">
          <el-space fill class="captcha-row">
            <el-alert
              v-if="captcha"
              type="info"
              :closable="false"
              :title="`${captcha.question}，1 分钟内有效`"
            />
            <el-button :loading="captchaLoading" @click="loadCaptcha">
              {{ captcha ? '刷新验证码' : '获取验证码' }}
            </el-button>
          </el-space>
          <el-input v-model="form.captchaAnswer" placeholder="请输入计算结果" />
        </el-form-item>
        <el-button type="primary" size="large" :loading="loading" class="login-card__button" @click="submit">
          进入控制台
        </el-button>
        <p class="auth-switch">
          还没有账号？
          <RouterLink to="/register">注册并自动领取基础套餐</RouterLink>
        </p>
      </el-form>
    </el-card>
  </main>
</template>

<style scoped>
.captcha-row {
  width: 100%;
  margin-bottom: 10px;
}
</style>
