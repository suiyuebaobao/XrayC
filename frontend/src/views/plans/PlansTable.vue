<!--
  本文件展示后台套餐列表和展开后的分组详情。
  它接收套餐、分组和分组内绑定节点数据，并把操作通过事件交给父级。
  格式化函数来自同目录 format.ts，不在组件里重复实现业务文案。
  删除、编辑和分组保存仍由 usePlansPage 统一处理。
-->
<script setup lang="ts">
import { Delete, Edit, EditPen } from '@element-plus/icons-vue';
import { computed } from 'vue';
import type { LineGroupSummary, PlanInfo } from '@/services/api';
import {
  formatGb,
  formatMbps,
  formatPrice,
  multiplierLabel,
  shortId,
  totalPlanTrafficGb,
} from '@/views/plans/format';

const props = defineProps<{
  plans: PlanInfo[];
  lineGroups: LineGroupSummary[];
  deletingId: string;
}>();

const emit = defineEmits<{
  (event: 'edit', plan: PlanInfo): void;
  (event: 'authorize', plan: PlanInfo): void;
  (event: 'delete', plan: PlanInfo): void;
  (event: 'explainDefaultDelete', plan: PlanInfo): void;
}>();

const lineGroupById = computed(() => new Map(props.lineGroups.map((group) => [group.id, group])));

function lineGroupName(lineGroupId: string) {
  return lineGroupById.value.get(lineGroupId)?.name || `分组 ${shortId(lineGroupId)}`;
}

function lineGroupStatusLabel(lineGroupId: string) {
  const group = lineGroupById.value.get(lineGroupId);
  if (!group) {
    return '分组信息缺失';
  }
  return group.enabled ? '已匹配分组' : '分组已停用';
}

function lineGroupMultiplierLabel(plan: PlanInfo, lineGroupId: string) {
  const bindingMultiplier = plan.lineGroups.find((binding) => binding.lineGroupId === lineGroupId)?.billingMultiplier;
  return multiplierLabel(bindingMultiplier || 1);
}

function lineGroupLineCount(lineGroupId: string) {
  const group = lineGroupById.value.get(lineGroupId);
  return group?.bindingNodeIds.length || group?.exitEndpointIds.length || 0;
}

function isDefaultLineGroup(plan: PlanInfo, lineGroupId: string) {
  return plan.defaultLineGroupId === lineGroupId;
}

</script>

<template>
  <el-card shadow="never" class="section-row">
    <template #header>套餐列表</template>
    <el-table :data="plans" row-key="id" stripe>
      <el-table-column type="expand">
        <template #default="{ row }">
          <div class="plan-groups">
            <el-empty v-if="row.lineGroups.length === 0" description="暂未配置分组" />
            <section v-for="binding in row.lineGroups" :key="binding.lineGroupId" class="plan-group-card">
              <div class="plan-group-card__header">
                <div>
                  <strong>{{ lineGroupName(binding.lineGroupId) }}</strong>
                  <p>{{ shortId(binding.lineGroupId) }}</p>
                </div>
                <div class="plan-group-card__tags">
                  <el-tag type="success" effect="plain">已授权</el-tag>
                  <el-tag v-if="isDefaultLineGroup(row, binding.lineGroupId)" type="warning" effect="plain">默认出口</el-tag>
                </div>
              </div>
              <div class="plan-group-card__meta">
                <span>分组：{{ lineGroupStatusLabel(binding.lineGroupId) }}</span>
                <span>绑定节点：{{ lineGroupLineCount(binding.lineGroupId) }} 条</span>
                <span>授权倍率：{{ lineGroupMultiplierLabel(row, binding.lineGroupId) }}</span>
              </div>
              <p class="muted">入口由入口管理绑定出口后生成。</p>
            </section>
          </div>
        </template>
      </el-table-column>

      <el-table-column label="套餐" min-width="190">
        <template #default="{ row }">
          <div class="plan-name">
            <strong>{{ row.name }}</strong>
            <span>{{ shortId(row.id) }}</span>
          </div>
          <div class="plan-tags">
            <el-tag v-if="row.isDefault" type="success" effect="plain">基础套餐</el-tag>
            <el-tag :type="row.enabled ? 'primary' : 'info'" effect="plain">
              {{ row.enabled ? '启用' : '停用' }}
            </el-tag>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="流量额度" min-width="120">
        <template #default="{ row }">
          {{ formatGb(totalPlanTrafficGb(row)) }}
        </template>
      </el-table-column>
      <el-table-column label="限速 上/下" min-width="170">
        <template #default="{ row }">
          ↑ {{ formatMbps(row.rateLimitUpBps != null ? row.rateLimitUpBps / 1_000_000 : row.rateLimitMbps) }}
          / ↓ {{ formatMbps(row.rateLimitDownBps != null ? row.rateLimitDownBps / 1_000_000 : row.rateLimitMbps) }}
        </template>
      </el-table-column>
      <el-table-column label="扣费倍率" min-width="150">
        <template #default="{ row }">
          {{ multiplierLabel(row.billingMultiplier) }}
        </template>
      </el-table-column>
      <el-table-column label="价格 / 周期" min-width="140">
        <template #default="{ row }">
          <div class="multiplier-stack">
            <span>{{ formatPrice(row) }}</span>
            <span>{{ row.durationDays }} 天</span>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="排序" min-width="90">
        <template #default="{ row }">
          {{ row.sortWeight }}
        </template>
      </el-table-column>
      <el-table-column label="授权分组" min-width="220">
        <template #default="{ row }">
          <div v-if="row.lineGroups.length > 0" class="group-tags">
            <el-tag v-for="binding in row.lineGroups" :key="binding.lineGroupId" effect="plain">
              {{ lineGroupName(binding.lineGroupId) }}
              <template v-if="isDefaultLineGroup(row, binding.lineGroupId)"> · 默认</template>
            </el-tag>
          </div>
          <span v-else class="muted">未配置分组</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="250" fixed="right">
        <template #default="{ row }">
          <el-button :icon="Edit" text type="primary" @click="emit('edit', row)">编辑基础</el-button>
          <el-button :icon="EditPen" text type="primary" @click="emit('authorize', row)">编辑分组</el-button>
          <el-tooltip v-if="row.isDefault" content="基础套餐不能删除" placement="top">
            <el-button :icon="Delete" text type="danger" disabled @click="emit('explainDefaultDelete', row)">删除</el-button>
          </el-tooltip>
          <el-button
            v-else
            :icon="Delete"
            text
            type="danger"
            :loading="deletingId === row.id"
            @click="emit('delete', row)"
          >
            删除
          </el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>
</template>

<style scoped>
.plan-name,
.multiplier-stack {
  display: grid;
  gap: 4px;
}

.plan-name span,
.multiplier-stack span,
.plan-group-card__header p {
  margin: 0;
  color: var(--ink-soft);
}

.plan-tags,
.group-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 10px;
}

.plan-groups {
  display: grid;
  gap: 14px;
  padding: 8px 24px 18px 62px;
}

.plan-group-card {
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.58);
}

.plan-group-card__header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.plan-group-card__tags {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.plan-group-card__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 12px;
  color: var(--ink-soft);
}

.plan-group-card__alert {
  margin-top: 12px;
}

.plan-line-table {
  display: grid;
  gap: 8px;
  margin-top: 14px;
}

.plan-line-table__head,
.plan-line-table__row {
  display: grid;
  grid-template-columns: minmax(150px, 1.3fr) minmax(180px, 1.1fr) minmax(160px, 1fr);
  gap: 12px;
  align-items: center;
}

.plan-line-table__head {
  color: var(--ink-soft);
  font-size: 12px;
}

.plan-line-table__row {
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.64);
}

.plan-line-table__row span {
  min-width: 0;
  overflow-wrap: anywhere;
}

.plan-line-table__row small {
  display: block;
  margin-top: 3px;
  color: var(--ink-soft);
}

@media (max-width: 900px) {
  .plan-groups {
    padding: 8px 0 18px;
  }

  .plan-line-table__head {
    display: none;
  }

  .plan-line-table__row {
    grid-template-columns: 1fr;
  }
}
</style>
