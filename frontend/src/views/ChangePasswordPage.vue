<!--
  本页面用于已登录用户修改自己的登录密码。
  验证方式为邮箱验证码：验证码发送到注册邮箱，无需填写旧密码。
  改密成功后后端会使所有登录态失效，前端随即退出并引导重新登录。
-->
<script setup lang="ts">
import { Lock } from '@element-plus/icons-vue';
import { ElMessage, type FormInstance, type FormRules } from 'element-plus';
import { computed, onUnmounted, reactive, ref } from 'vue';
import { useRouter } from 'vue-router';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient } from '@/services/api';
import { useSessionStore } from '@/stores/session';

const router = useRouter();
const session = useSessionStore();
const formRef = ref<FormInstance>();
const submitting = ref(false);
const emailCodeLoading = ref(false);
const emailCooldownLeft = ref(0);
let emailCooldownTimer: number | undefined;

const form = reactive({
  emailCodeId: '',
  emailCode: '',
  newPassword: '',
  confirmPassword: '',
});

// 验证码发到当前登录账号邮箱，这里只做提示展示，不允许用户改邮箱。
const accountEmail = computed(() => session.user?.account || '');

const rules: FormRules<typeof form> = {
  emailCode: [
    { required: true, message: '请输入邮箱验证码', trigger: 'blur' },
    { min: 6, max: 6, message: '邮箱验证码为 6 位', trigger: 'blur' },
  ],
  newPassword: [
    { required: true, message: '请输入新密码', trigger: 'blur' },
    { min: 8, message: '新密码至少 8 位', trigger: 'blur' },
  ],
  confirmPassword: [
    { required: true, message: '请再次输入新密码', trigger: 'blur' },
    {
      validator: (_rule, value, callback) => {
        if (String(value || '') !== form.newPassword) {
          callback(new Error('两次输入的新密码不一致'));
        } else {
          callback();
        }
      },
      trigger: 'blur',
    },
  ],
};

onUnmounted(() => {
  if (emailCooldownTimer) {
    window.clearInterval(emailCooldownTimer);
  }
});

async function sendEmailCode() {
  emailCodeLoading.value = true;
  try {
    const result = await apiClient.sendChangePasswordCode();
    form.emailCodeId = result.id;
    form.emailCode = '';
    startEmailCooldown(result.cooldownSeconds);
    ElMessage.success('邮箱验证码已发送至注册邮箱');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '邮箱验证码发送失败');
  } finally {
    emailCodeLoading.value = false;
  }
}

async function submit() {
  await formRef.value?.validate();
  if (!form.emailCodeId) {
    ElMessage.warning('请先获取邮箱验证码');
    return;
  }
  submitting.value = true;
  try {
    await apiClient.changePassword({
      emailCodeId: form.emailCodeId,
      emailCode: form.emailCode,
      newPassword: form.newPassword,
    });
    // 改密成功后所有登录态已失效，本地立即退出并跳转登录页用新密码登录。
    ElMessage.success('密码已修改，请使用新密码重新登录');
    await session.logout();
    router.push('/login');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '修改密码失败，请检查验证码与新密码');
  } finally {
    submitting.value = false;
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
  <PageHeader title="修改密码" description="通过注册邮箱验证码验证身份后设置新密码，无需填写旧密码。" />

  <el-row :gutter="18">
    <el-col :xs="24" :lg="12">
      <el-card shadow="never">
        <template #header>账户安全</template>
        <el-alert
          class="change-password__hint"
          type="info"
          :closable="false"
          :title="`验证码将发送到注册邮箱：${accountEmail}`"
        />
        <el-form ref="formRef" :model="form" :rules="rules" label-position="top" @keyup.enter="submit">
          <el-form-item label="邮箱验证码" prop="emailCode">
            <el-space fill class="change-password__code">
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
          <el-form-item label="新密码" prop="newPassword">
            <el-input
              v-model="form.newPassword"
              :prefix-icon="Lock"
              placeholder="至少 8 位"
              show-password
            />
          </el-form-item>
          <el-form-item label="确认新密码" prop="confirmPassword">
            <el-input
              v-model="form.confirmPassword"
              :prefix-icon="Lock"
              placeholder="再次输入新密码"
              show-password
            />
          </el-form-item>
          <el-button
            type="primary"
            size="large"
            :loading="submitting"
            class="change-password__button"
            @click="submit"
          >
            确认修改密码
          </el-button>
          <p class="muted">
            修改成功后所有设备的登录态都会失效，需要用新密码重新登录。
          </p>
        </el-form>
      </el-card>
    </el-col>
  </el-row>
</template>

<style scoped>
.change-password__hint {
  margin-bottom: 18px;
}

.change-password__code {
  width: 100%;
  margin-bottom: 10px;
}

.change-password__button {
  width: 100%;
  margin-top: 6px;
}
</style>
