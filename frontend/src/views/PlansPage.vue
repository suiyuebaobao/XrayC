<!--
  本文件是后台“套餐授权”页面入口。
  它只负责页面骨架、刷新入口和拆分组件编排。
  套餐数据加载与保存删除逻辑放在 views/plans/usePlansPage.ts。
  列表、统计卡片和两个弹窗均拆到 views/plans 目录。
-->
<script setup lang="ts">
import { Plus, Refresh } from '@element-plus/icons-vue';
import PageHeader from '@/components/PageHeader.vue';
import PlanAuthorizationDialog from '@/views/plans/PlanAuthorizationDialog.vue';
import PlanFormDialog from '@/views/plans/PlanFormDialog.vue';
import PlansSummaryCards from '@/views/plans/PlansSummaryCards.vue';
import PlansTable from '@/views/plans/PlansTable.vue';
import { usePlansPage } from '@/views/plans/usePlansPage';

const {
  loading,
  saving,
  deletingId,
  plans,
  lineGroups,
  planDialogVisible,
  planDialogMode,
  authorizationDialogVisible,
  editingPlan,
  planForm,
  selectedLineGroupIds,
  draftBindings,
  defaultLineGroupId,
  defaultPlan,
  authorizedLineGroupCount,
  planLineGroupBindingCount,
  loadPlans,
  openCreatePlan,
  openEditPlan,
  openAuthorization,
  syncDraftBindings,
  removeDraftBinding,
  savePlan,
  saveAuthorization,
  explainDefaultDelete,
  deletePlan,
} = usePlansPage();
</script>

<template>
  <PageHeader title="套餐授权" description="管理套餐基础字段、流量额度和可访问的分组。">
    <el-button :icon="Plus" type="primary" @click="openCreatePlan">新增套餐</el-button>
    <el-button :icon="Refresh" :loading="loading" @click="loadPlans">刷新</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="10" animated />
  <template v-else>
    <PlansSummaryCards
      :plans="plans"
      :default-plan="defaultPlan"
      :authorized-line-group-count="authorizedLineGroupCount"
      :line-group-binding-count="planLineGroupBindingCount"
    />
    <PlansTable
      :plans="plans"
      :line-groups="lineGroups"
      :deleting-id="deletingId"
      @edit="openEditPlan"
      @authorize="openAuthorization"
      @delete="deletePlan"
      @explain-default-delete="explainDefaultDelete"
    />
  </template>

  <PlanFormDialog
    v-model="planDialogVisible"
    v-model:form="planForm"
    :mode="planDialogMode"
    :editing-plan="editingPlan"
    :saving="saving"
    @save="savePlan"
  />

  <PlanAuthorizationDialog
    v-model="authorizationDialogVisible"
    v-model:selected-line-group-ids="selectedLineGroupIds"
    v-model:draft-bindings="draftBindings"
    v-model:default-line-group-id="defaultLineGroupId"
    :editing-plan="editingPlan"
    :line-groups="lineGroups"
    :saving="saving"
    @sync-bindings="syncDraftBindings"
    @remove-binding="removeDraftBinding"
    @save="saveAuthorization"
  />
</template>
