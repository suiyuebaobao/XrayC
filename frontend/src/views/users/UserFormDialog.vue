<!--
  用户表单弹窗统一承载管理员新增和编辑用户。
  页面层只负责打开弹窗和刷新列表，避免用户页继续膨胀。
  新增用户会直接调用管理端创建接口，不走公开注册验证码流程。
  编辑模式邮箱只读（账号身份/登录与验证码锚点，创建后不可改），沿用原有状态、角色和套餐授权逻辑。
-->
<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { computed, reactive, ref, watch } from 'vue';
import { apiClient, type AdminUser, type PlanInfo } from '@/services/api';

const props = defineProps<{
  modelValue: boolean;
  mode: 'create' | 'edit';
  user?: AdminUser;
  plans: PlanInfo[];
}>();

const emit = defineEmits<{
  'update:modelValue': [value: boolean];
  saved: [];
}>();

const saving = ref(false);
const form = reactive({
  id: '' as number | string,
  account: '',
  email: '',
  password: '',
  status: 'active' as 'active' | 'disabled',
  role: 'user' as AdminUser['role'],
  planId: '',
  originalPlanId: '',
  currentPlanName: '',
  rateLimitMode: 'inherit' as 'inherit' | 'unlimited' | 'custom',
  rateLimitMbps: 0,
  rateLimitUpMbps: 0,
  rateLimitDownMbps: 0,
});

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
});
const selectablePlans = computed(() => props.plans.filter((plan) => plan.enabled && !plan.isDeleted));
const title = computed(() => (props.mode === 'create' ? '新增用户' : `编辑用户：${form.account || form.email}`));
const currentPlanUnavailable = computed(() => (
  props.mode === 'edit'
  && visible.value
  && Boolean(form.originalPlanId || form.currentPlanName)
  && !isSelectablePlan(form.originalPlanId)
));

watch(
  () => [props.modelValue, props.mode, props.user?.id],
  () => {
    if (props.modelValue) {
      resetForm();
    }
  },
  { immediate: true },
);

async function save() {
  if (!form.email.trim()) {
    ElMessage.warning('请输入邮箱');
    return;
  }
  if (props.mode === 'create' && form.password.length < 8) {
    ElMessage.warning('请输入至少 8 位密码');
    return;
  }

  saving.value = true;
  try {
    if (props.mode === 'create') {
      await apiClient.createAdminUser({
        email: form.email.trim(),
        password: form.password,
        status: form.status,
        role: form.role,
        planId: form.planId || undefined,
        rateLimitBps: userRateLimitPayload(),
        rateLimitUpBps: userDirectionalPayload(form.rateLimitUpMbps),
        rateLimitDownBps: userDirectionalPayload(form.rateLimitDownMbps),
      });
    } else {
      const payload: Parameters<typeof apiClient.updateAdminUser>[1] = {
        email: form.email.trim(),
        status: form.status,
        role: form.role,
      };
      if (form.planId && form.planId !== form.originalPlanId) {
        payload.planId = form.planId;
      }
      payload.rateLimitBps = userRateLimitPayload();
      payload.rateLimitUpBps = userDirectionalPayload(form.rateLimitUpMbps);
      payload.rateLimitDownBps = userDirectionalPayload(form.rateLimitDownMbps);
      await apiClient.updateAdminUser(form.id, payload);
    }
    visible.value = false;
    emit('saved');
    ElMessage.success(props.mode === 'create' ? '用户已创建' : '用户已更新');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存用户失败');
  } finally {
    saving.value = false;
  }
}

function resetForm() {
  if (props.mode === 'create') {
    form.id = '';
    form.account = '';
    form.email = '';
    form.password = '';
    form.status = 'active';
    form.role = 'user';
    form.originalPlanId = '';
    form.currentPlanName = '';
    form.planId = '';
    form.rateLimitMode = 'inherit';
    form.rateLimitMbps = 0;
    form.rateLimitUpMbps = 0;
    form.rateLimitDownMbps = 0;
    return;
  }
  const user = props.user;
  const currentPlanId = user ? user.planId || planIdByName(user.planName) : '';
  form.id = user?.id ?? '';
  form.account = user?.account || user?.email || '';
  form.email = user?.email || '';
  form.password = '';
  form.status = user?.status === 'disabled' ? 'disabled' : 'active';
  form.role = user?.role ?? 'user';
  form.originalPlanId = currentPlanId;
  form.currentPlanName = user?.planName || planNameById(currentPlanId);
  form.planId = currentPlanId && isSelectablePlan(currentPlanId) ? currentPlanId : '';
  if (user?.userRateLimitBps === null || user?.userRateLimitBps === undefined) {
    form.rateLimitMode = 'inherit';
    form.rateLimitMbps = 0;
  } else if (user.userRateLimitBps <= 0) {
    form.rateLimitMode = 'unlimited';
    form.rateLimitMbps = 0;
  } else {
    form.rateLimitMode = 'custom';
    form.rateLimitMbps = Number((user.userRateLimitBps / 1_000_000).toFixed(3));
  }
  // 方向限速用户级覆盖：未设（null）或 0 一律回填 0，表示该方向沿用套餐。
  form.rateLimitUpMbps = user?.userRateLimitUpBps
    ? Number((user.userRateLimitUpBps / 1_000_000).toFixed(3))
    : 0;
  form.rateLimitDownMbps = user?.userRateLimitDownBps
    ? Number((user.userRateLimitDownBps / 1_000_000).toFixed(3))
    : 0;
}

function planIdByName(planName: string) {
  return props.plans.find((plan) => plan.name === planName)?.id ?? '';
}

function planNameById(planId: string) {
  return props.plans.find((plan) => plan.id === planId)?.name ?? '';
}

function isSelectablePlan(planId: string) {
  return Boolean(planId && selectablePlans.value.some((plan) => plan.id === planId));
}

function userRateLimitPayload() {
  if (form.rateLimitMode === 'inherit') {
    return null;
  }
  if (form.rateLimitMode === 'unlimited') {
    return 0;
  }
  if (!Number.isFinite(form.rateLimitMbps) || form.rateLimitMbps <= 0) {
    return 0;
  }
  return Math.round(form.rateLimitMbps * 1_000_000);
}

// 用户方向限速覆盖：0/空=该方向沿用套餐，发 null 让后端回退套餐；正值=用户级独立限速。
function userDirectionalPayload(mbps: number): number | null {
  if (!Number.isFinite(mbps) || mbps <= 0) {
    return null;
  }
  return Math.round(mbps * 1_000_000);
}
</script>

<template>
  <el-dialog v-model="visible" :title="title" width="560px">
    <el-form label-width="96px" @submit.prevent>
      <el-form-item label="邮箱" required>
        <el-input
          v-model="form.email"
          :disabled="mode === 'edit'"
          autocomplete="off"
          placeholder="user@example.com"
        />
        <div v-if="mode === 'edit'" class="field-hint">邮箱是账号身份/登录与验证码锚点，创建后不可修改</div>
      </el-form-item>
      <el-form-item v-if="mode === 'create'" label="密码" required>
        <el-input v-model="form.password" type="password" show-password autocomplete="new-password" placeholder="至少 8 位" />
      </el-form-item>
      <el-form-item label="启用状态" required>
        <el-switch
          v-model="form.status"
          active-text="启用"
          inactive-text="禁用"
          active-value="active"
          inactive-value="disabled"
        />
      </el-form-item>
      <el-form-item label="管理员标识" required>
        <el-switch
          v-model="form.role"
          active-text="管理员"
          inactive-text="普通用户"
          active-value="admin"
          inactive-value="user"
        />
      </el-form-item>
      <el-form-item label="套餐">
        <el-alert
          v-if="currentPlanUnavailable"
          class="plan-alert"
          :title="`当前套餐 ${form.currentPlanName || form.originalPlanId} 不在可选启用套餐中`"
          description="未选择新套餐时，保存不会提交 plan_id，也不会变更用户当前套餐。"
          type="info"
          show-icon
          :closable="false"
        />
        <el-select v-model="form.planId" placeholder="不变更套餐" filterable clearable>
          <el-option v-for="plan in selectablePlans" :key="plan.id" :label="plan.name" :value="plan.id" />
        </el-select>
      </el-form-item>
      <el-form-item label="用户限速">
        <el-radio-group v-model="form.rateLimitMode">
          <el-radio-button label="inherit">继承套餐</el-radio-button>
          <el-radio-button label="unlimited">不限速</el-radio-button>
          <el-radio-button label="custom">自定义</el-radio-button>
        </el-radio-group>
        <el-input-number
          v-if="form.rateLimitMode === 'custom'"
          v-model="form.rateLimitMbps"
          class="rate-input"
          :min="0"
          :precision="2"
          :step="10"
          controls-position="right"
        />
      </el-form-item>
      <el-form-item label="上行限速">
        <el-input-number
          v-model="form.rateLimitUpMbps"
          class="rate-input"
          :min="0"
          :precision="2"
          :step="10"
          controls-position="right"
        />
        <div class="field-hint">Mbps，0 = 沿用套餐上行限速</div>
      </el-form-item>
      <el-form-item label="下行限速">
        <el-input-number
          v-model="form.rateLimitDownMbps"
          class="rate-input"
          :min="0"
          :precision="2"
          :step="10"
          controls-position="right"
        />
        <div class="field-hint">Mbps，0 = 沿用套餐下行限速</div>
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="save">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.plan-alert {
  margin-bottom: 10px;
}

.rate-input {
  display: block;
  margin-top: 10px;
}

.field-hint {
  margin-top: 4px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 1.4;
}
</style>
