<!--
  本文件承载套餐基础字段的新建和编辑弹窗。
  它只维护表单展示和用户输入绑定，不直接调用 API。
  保存动作通过 save 事件交给 usePlansPage 统一处理。
  这样 PlansPage 不再包含大段表单模板。
-->
<script setup lang="ts">
import type { PlanInfo } from '@/services/api';
import type { PlanFormDraft } from '@/views/plans/types';

const visible = defineModel<boolean>({ required: true });
const form = defineModel<PlanFormDraft>('form', { required: true });

defineProps<{
  mode: 'create' | 'edit';
  editingPlan?: PlanInfo;
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'save'): void;
}>();
</script>

<template>
  <el-dialog
    v-model="visible"
    :title="mode === 'create' ? '新增套餐' : `编辑套餐基础：${editingPlan?.name ?? ''}`"
    width="760px"
  >
    <el-form label-position="top" class="plan-form">
      <div class="plan-form__grid">
        <el-form-item label="套餐名称" required>
          <el-input v-model="form.name" maxlength="60" show-word-limit placeholder="例如：标准套餐" />
        </el-form-item>
        <el-form-item label="启用状态">
          <el-switch v-model="form.enabled" active-text="启用" inactive-text="停用" />
        </el-form-item>
        <el-form-item label="流量额度（GB）">
          <el-input-number v-model="form.trafficLimitGb" :min="0" :precision="2" :step="10" controls-position="right" />
        </el-form-item>
        <el-form-item label="上行限速（Mbps，0=不限）">
          <el-input-number v-model="form.rateLimitUpMbps" :min="0" :precision="2" :step="10" controls-position="right" />
        </el-form-item>
        <el-form-item label="下行限速（Mbps，0=不限）">
          <el-input-number v-model="form.rateLimitDownMbps" :min="0" :precision="2" :step="10" controls-position="right" />
        </el-form-item>
        <el-form-item label="扣费倍率">
          <el-input-number v-model="form.multiplier" :min="0" :precision="3" :step="0.1" controls-position="right" />
        </el-form-item>
        <el-form-item label="价格">
          <el-input-number v-model="form.price" :min="0" :precision="2" :step="1" controls-position="right" />
        </el-form-item>
        <el-form-item label="币种">
          <el-input v-model="form.currency" maxlength="12" placeholder="USDT" />
        </el-form-item>
        <el-form-item label="周期（天）">
          <el-input-number v-model="form.durationDays" :min="1" :max="3650" controls-position="right" />
        </el-form-item>
        <el-form-item label="排序权重">
          <el-input-number v-model="form.sortWeight" :min="0" :max="999999" controls-position="right" />
        </el-form-item>
      </div>
    </el-form>

    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="emit('save')">
        {{ mode === 'create' ? '创建套餐' : '保存基础字段' }}
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.plan-form {
  margin-top: 18px;
}

.plan-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 2px 18px;
}

@media (max-width: 900px) {
  .plan-form__grid {
    grid-template-columns: 1fr;
  }
}
</style>
