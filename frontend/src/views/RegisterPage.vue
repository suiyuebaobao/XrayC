<!--
  本页面用于普通用户邮箱注册。
  它按后台策略处理邀请码、图形验证码和邮箱验证码。
  注册成功后由会话 store 保存登录态并进入用户后台。
-->
<script setup lang="ts">
import { Lock, Message } from '@element-plus/icons-vue';
import { ElMessage, type FormInstance, type FormRules } from 'element-plus';
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { apiClient, type AuthSecuritySettings, type CaptchaChallenge } from '@/services/api';
import { useSessionStore } from '@/stores/session';

const router = useRouter();
const session = useSessionStore();
const formRef = ref<FormInstance>();
const loading = ref(false);

const form = reactive({
  email: '',
  password: '',
  inviteCode: '',
  captchaId: '',
  captchaAnswer: '',
  emailCodeId: '',
  emailCode: '',
});
const security = ref<AuthSecuritySettings | null>(null);
const captcha = ref<CaptchaChallenge | null>(null);
const captchaLoading = ref(false);
const emailCodeLoading = ref(false);
const emailCooldownLeft = ref(0);
let emailCooldownTimer: number | undefined;
const allowedDomainsText = computed(() => security.value?.allowedEmailDomains.join('、') || '');
const captchaRequired = computed(() => Boolean(security.value?.captchaRegisterEnabled));
const emailCodeRequired = computed(() => Boolean(security.value?.emailVerificationEnabled));
const inviteLabel = computed(() => (security.value?.inviteRequired ? '邀请码' : '邀请码（选填）'));

const rules: FormRules<typeof form> = {
  email: [
    { required: true, message: '请输入邮箱', trigger: 'blur' },
    { type: 'email', message: '邮箱格式不正确', trigger: 'blur' },
  ],
  password: [
    { required: true, message: '请输入密码', trigger: 'blur' },
    { min: 8, message: '密码至少 8 位', trigger: 'blur' },
  ],
  inviteCode: [
    {
      validator: (_rule, value, callback) => {
        if (security.value?.inviteRequired && !String(value || '').trim()) {
          callback(new Error('请输入邀请码'));
        } else {
          callback();
        }
      },
      trigger: 'blur',
    },
  ],
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
  emailCode: [
    {
      validator: (_rule, value, callback) => {
        if (emailCodeRequired.value && !String(value || '').trim()) {
          callback(new Error('请输入邮箱验证码'));
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

onUnmounted(() => {
  if (emailCooldownTimer) {
    window.clearInterval(emailCooldownTimer);
  }
});

watch(
  () => form.email,
  () => {
    captcha.value = null;
    form.captchaId = '';
    form.captchaAnswer = '';
    form.emailCodeId = '';
    form.emailCode = '';
  },
);

async function submit() {
  await formRef.value?.validate();
  if (captchaRequired.value && !form.captchaId) {
    await loadCaptcha();
    ElMessage.warning('请填写算术验证码后再注册');
    return;
  }
  if (emailCodeRequired.value && !form.emailCodeId) {
    ElMessage.warning('请先发送邮箱验证码');
    return;
  }
  loading.value = true;
  try {
    await session.register({
      email: form.email,
      password: form.password,
      inviteCode: form.inviteCode,
      captchaId: form.captchaId,
      captchaAnswer: form.captchaAnswer,
      emailCodeId: form.emailCodeId,
      emailCode: form.emailCode,
    });
    ElMessage.success('注册成功，已分配基础套餐');
    router.push('/subscription');
  } catch (error) {
    if (captchaRequired.value) {
      await loadCaptcha();
    }
    ElMessage.error(error instanceof Error ? error.message : '注册失败，请检查邀请码、邮箱和安全验证');
  } finally {
    loading.value = false;
  }
}

async function loadCaptcha() {
  if (!form.email.trim()) {
    ElMessage.warning('请先输入邮箱');
    return;
  }
  captchaLoading.value = true;
  try {
    captcha.value = await apiClient.createCaptcha('register', form.email);
    form.captchaId = captcha.value.id;
    form.captchaAnswer = '';
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '获取验证码失败');
  } finally {
    captchaLoading.value = false;
  }
}

async function sendEmailCode() {
  await formRef.value?.validateField('email');
  emailCodeLoading.value = true;
  try {
    const result = await apiClient.sendEmailCode(form.email);
    form.emailCodeId = result.id;
    form.emailCode = '';
    startEmailCooldown(result.cooldownSeconds);
    ElMessage.success('邮箱验证码已发送');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '邮箱验证码发送失败');
  } finally {
    emailCodeLoading.value = false;
  }
}

function startEmailCooldown(seconds: number) {
  if (emailCooldownTimer) {
    window.clearInterval(emailCooldownTimer);
  }
  emailCooldownLeft.value = seconds;
  emailCooldownTimer = window.setInterval(() => {
    emailCooldownLeft.value -= 1;
    if (emailCooldownLeft.value <= 0 && emailCooldownTimer) {
      window.clearInterval(emailCooldownTimer);
      emailCooldownTimer = undefined;
    }
  }, 1000);
}
</script>

<template>
  <main class="login-page">
    <section class="login-hero">
      <p class="eyebrow">Instant access</p>
      <h1>注册后立即获得订阅入口。</h1>
      <p>
        系统会自动创建基础套餐和唯一订阅 Token，套餐授权决定可访问节点分组。
      </p>
    </section>

    <el-card class="login-card" shadow="never">
      <template #header>
        <div>
          <h2>创建账号</h2>
          <span>邮箱会作为登录账号，注册后自动进入用户订阅页。</span>
        </div>
      </template>

      <el-form ref="formRef" :model="form" :rules="rules" label-position="top" @keyup.enter="submit">
        <el-form-item label="邮箱" prop="email">
          <el-input v-model="form.email" :prefix-icon="Message" placeholder="user@example.com" />
        </el-form-item>
        <el-form-item label="密码" prop="password">
          <el-input v-model="form.password" :prefix-icon="Lock" placeholder="至少 8 位" show-password />
        </el-form-item>
        <el-form-item :label="inviteLabel" prop="inviteCode">
          <el-input v-model="form.inviteCode" placeholder="可填写邀请人提供的邀请码" />
        </el-form-item>
        <el-form-item v-if="captchaRequired" label="算术验证码" prop="captchaAnswer">
          <el-space fill class="login-card__captcha">
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
        <el-form-item v-if="emailCodeRequired" label="邮箱验证码" prop="emailCode">
          <el-space fill class="login-card__captcha">
            <el-button
              :loading="emailCodeLoading"
              :disabled="emailCooldownLeft > 0"
              @click="sendEmailCode"
            >
              {{ emailCooldownLeft > 0 ? `${emailCooldownLeft}s 后重发` : '发送邮箱验证码' }}
            </el-button>
          </el-space>
          <el-input v-model="form.emailCode" placeholder="请输入 6 位邮箱验证码" />
        </el-form-item>
        <el-alert
          v-if="allowedDomainsText"
          class="login-card__alert"
          type="info"
          :closable="false"
          :title="`当前仅允许以下邮箱域名注册：${allowedDomainsText}`"
        />
        <el-button type="primary" size="large" :loading="loading" class="login-card__button" @click="submit">
          注册并领取基础套餐
        </el-button>
        <p class="auth-switch">
          已有账号？
          <RouterLink to="/login">返回登录</RouterLink>
        </p>
      </el-form>
    </el-card>
  </main>
</template>

<style scoped>
.login-card__alert {
  margin-bottom: 18px;
}

.login-card__captcha {
  width: 100%;
  margin-bottom: 10px;
}
</style>
