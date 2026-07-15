<!--
  本文件承载套餐可用分组配置弹窗。
  它只让管理员选择套餐可访问的扁平分组。
  数据同步和保存动作通过事件交给 usePlansPage 处理。
  拆出后主页面避免包含大段分组表格模板。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type { LineGroupSummary, PlanInfo } from '@/services/api';
import { shortId } from '@/views/plans/format';
import type { PlanLineGroupDraft } from '@/views/plans/types';

const visible = defineModel<boolean>({ required: true });
const selectedLineGroupIds = defineModel<string[]>('selectedLineGroupIds', { required: true });
const draftBindings = defineModel<PlanLineGroupDraft[]>('draftBindings', { required: true });
const defaultLineGroupId = defineModel<string>('defaultLineGroupId', { required: true });

const props = defineProps<{
  editingPlan?: PlanInfo;
  lineGroups: LineGroupSummary[];
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'save'): void;
  (event: 'syncBindings', lineGroupIds: string[]): void;
  (event: 'removeBinding', lineGroupId: string): void;
}>();

const lineGroupById = computed(() => new Map(props.lineGroups.map((group) => [group.id, group])));
const selectableLineGroups = computed(() =>
  props.lineGroups
    .slice()
    .sort((left, right) => left.sortOrder - right.sortOrder || left.name.localeCompare(right.name, 'zh-Hans-CN')),
);
const defaultLineGroupOptions = computed(() =>
  draftBindings.value.map((binding) => ({
    id: binding.lineGroupId,
    name: lineGroupName(binding.lineGroupId),
    disabled: lineGroupById.value.get(binding.lineGroupId)?.enabled === false,
  })),
);

function lineGroupName(lineGroupId: string) {
  return lineGroupById.value.get(lineGroupId)?.name || `分组 ${shortId(lineGroupId)}`;
}

function lineGroupLineCount(lineGroupId: string) {
  return rawLineGroupLineCount(lineGroupId);
}

function rawLineGroupLineCount(lineGroupId: string) {
  const group = lineGroupById.value.get(lineGroupId);
  return group?.bindingNodeIds.length || group?.exitEndpointIds.length || 0;
}

function handleSelectionChange(value: string[] | string) {
  emit('syncBindings', Array.isArray(value) ? value : [value]);
}

</script>

<template>
  <el-dialog v-model="visible" :title="`配置套餐分组：${editingPlan?.name ?? ''}`" width="760px">
    <el-alert
      type="info"
      show-icon
      :closable="false"
      title="套餐只添加分组；分组成员在分组页维护，这里设置分组倍率。"
    />

    <el-form label-position="top" class="authorization-form">
      <el-form-item label="可用分组">
        <el-select
          v-model="selectedLineGroupIds"
          multiple
          filterable
          class="authorization-form__select"
          placeholder="选择套餐允许访问的分组"
          @change="handleSelectionChange"
        >
          <el-option
            v-for="group in selectableLineGroups"
            :key="group.id"
            :label="`${group.name}（${lineGroupLineCount(group.id)} 个绑定节点）`"
            :value="group.id"
            :disabled="!group.enabled"
          >
            <span>{{ group.name }}</span>
            <small class="muted-option">
              {{ lineGroupLineCount(group.id) }} 个绑定节点
              <template v-if="!group.enabled"> · 已停用</template>
            </small>
          </el-option>
        </el-select>
      </el-form-item>
      <el-form-item v-if="draftBindings.length > 0" label="默认出口分组">
        <el-select
          v-model="defaultLineGroupId"
          class="authorization-form__select"
          placeholder="选择未命中规则时使用的分组"
        >
          <el-option
            v-for="group in defaultLineGroupOptions"
            :key="group.id"
            :label="group.name"
            :value="group.id"
            :disabled="group.disabled"
          />
        </el-select>
      </el-form-item>
    </el-form>

    <el-empty v-if="draftBindings.length === 0" description="未选择分组，保存后该套餐不会开放任何线路。" />
    <el-table v-else :data="draftBindings" row-key="lineGroupId" stripe>
      <el-table-column label="分组" min-width="220">
        <template #default="{ row }">
          <strong>{{ lineGroupName(row.lineGroupId) }}</strong>
          <p class="muted">
            绑定节点 {{ rawLineGroupLineCount(row.lineGroupId) }} 个
          </p>
          <p v-if="lineGroupById.get(row.lineGroupId)?.enabled === false" class="muted muted-danger">
            该分组已停用，保存前建议移除或重新启用。
          </p>
        </template>
      </el-table-column>
      <el-table-column label="扣费倍率" width="170">
        <template #default="{ row }">
          <el-input-number
            v-model="row.billingMultiplier"
            :min="0.001"
            :max="100"
            :step="0.1"
            :precision="3"
            controls-position="right"
            class="authorization-form__number"
          />
        </template>
      </el-table-column>
      <el-table-column label="操作" width="90">
        <template #default="{ row }">
          <el-button text type="danger" @click="emit('removeBinding', row.lineGroupId)">移除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <template #footer>
      <el-button @click="visible = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="emit('save')">保存分组</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.authorization-form {
  margin-top: 18px;
}

.authorization-form__select {
  width: 100%;
}

.authorization-form__number {
  width: 100%;
}

.muted-option {
  color: var(--ink-soft);
  float: right;
}

.muted-danger {
  color: var(--el-color-danger);
}

</style>
