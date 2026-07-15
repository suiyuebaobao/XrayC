<!--
  本组件为管理员重置指定用户密码的弹窗。
  admin 本身即授权，直接设新密码、无需邮箱验证码，提交成功后该用户需重新登录。
  组件自包含表单校验与提交，UsersPage 只负责传入目标用户并打开弹窗。
-->
<script setup lang="ts">
import { ElMessage, type FormInstance, type FormRules } from 'element-plus';
import { reactive, ref, watch } from 'vue';
import { apiClient, type AdminUser } from '@/services/api';

const props = defineProps<{
  modelValue: boolean;
  user?: AdminUser;
}>();
const emit = defineEmits<{
  'update:modelValue': [value: boolean];
  done: [];
}>();

const formRef = ref<FormInstance>();
const submitting = ref(false);
const form = reactive({
  newPassword: '',
  confirmPassword: '',
});

const rules: FormRules<typeof form> = {
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

// 每次打开弹窗都清空表单，避免残留上一次输入的密码。
watch(
  () => props.modelValue,
  (open) => {
    if (open) {
      form.newPassword = '';
      form.confirmPassword = '';
      formRef.value?.clearValidate();
    }
  },
);

function close() {
  emit('update:modelValue', false);
}

async function submit() {
  if (!props.user) {
    return;
  }
  await formRef.value?.validate();
  submitting.value = true;
  try {
    await apiClient.resetAdminUserPassword(props.user.id, form.newPassword);
    ElMessage.success('已重置，该用户需用新密码重新登录');
    emit('done');
    close();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '重置密码失败');
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="`重置密码：${user?.account ?? ''}`"
    width="460px"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <el-alert
      class="reset-password__hint"
      type="warning"
      :closable="false"
      title="重置后该用户所有设备登录态立即失效，需用新密码重新登录。"
    />
    <el-form ref="formRef" :model="form" :rules="rules" label-position="top" @keyup.enter="submit">
      <el-form-item label="新密码" prop="newPassword">
        <el-input v-model="form.newPassword" placeholder="至少 8 位" show-password />
      </el-form-item>
      <el-form-item label="确认新密码" prop="confirmPassword">
        <el-input v-model="form.confirmPassword" placeholder="再次输入新密码" show-password />
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="close">取消</el-button>
      <el-button type="primary" :loading="submitting" @click="submit">确认重置</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.reset-password__hint {
  margin-bottom: 16px;
}
</style>
