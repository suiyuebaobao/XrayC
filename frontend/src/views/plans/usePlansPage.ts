// 本文件封装后台套餐分组页面的数据加载、弹窗状态和保存删除动作。
// 页面入口只消费这里返回的状态与事件函数，避免继续膨胀。
// 这里仅调用既有 apiClient，不修改 services/api.ts 或共享 API 类型。
// 表单默认值和展示换算来自同目录 format.ts，便于组件复用。
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import {
  apiClient,
  type AdminPlanPayload,
  type LineGroupSummary,
  type PlanInfo,
} from '@/services/api';
import { emptyPlanForm, formFromPlan } from '@/views/plans/format';
import type { PlanFormDraft, PlanLineGroupDraft } from '@/views/plans/types';

export function usePlansPage() {
  const loading = ref(true);
  const saving = ref(false);
  const deletingId = ref('');
  const plans = ref<PlanInfo[]>([]);
  const lineGroups = ref<LineGroupSummary[]>([]);
  const planDialogVisible = ref(false);
  const planDialogMode = ref<'create' | 'edit'>('create');
  const authorizationDialogVisible = ref(false);
  const editingPlan = ref<PlanInfo>();
  const planForm = ref<PlanFormDraft>(emptyPlanForm());
  const selectedLineGroupIds = ref<string[]>([]);
  const draftBindings = ref<PlanLineGroupDraft[]>([]);
  const defaultLineGroupId = ref('');

  const defaultPlan = computed(() => plans.value.find((plan) => plan.isDefault));
  const authorizedLineGroupCount = computed(() => {
    const groupIds = new Set(plans.value.flatMap((plan) => plan.lineGroups.map((binding) => binding.lineGroupId)));
    return groupIds.size;
  });
  const planLineGroupBindingCount = computed(() => plans.value.flatMap((plan) => plan.lineGroups).length);

  onMounted(loadPlans);

  async function loadPlans() {
    loading.value = true;
    try {
      const [planData, controlPlane] = await Promise.all([apiClient.getAdminPlans(), apiClient.getControlPlane()]);
      plans.value = planData;
      lineGroups.value = controlPlane.lineGroups;
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '加载套餐失败');
    } finally {
      loading.value = false;
    }
  }

  function openCreatePlan() {
    planDialogMode.value = 'create';
    editingPlan.value = undefined;
    planForm.value = emptyPlanForm();
    planDialogVisible.value = true;
  }

  function openEditPlan(plan: PlanInfo) {
    planDialogMode.value = 'edit';
    editingPlan.value = plan;
    planForm.value = formFromPlan(plan);
    planDialogVisible.value = true;
  }

  function openAuthorization(plan: PlanInfo) {
    editingPlan.value = plan;
    selectedLineGroupIds.value = plan.lineGroups.map((binding) => binding.lineGroupId);
    draftBindings.value = plan.lineGroups.map((binding) => ({
      lineGroupId: binding.lineGroupId,
      billingMultiplier: normalizeBillingMultiplier(binding.billingMultiplier || 1),
    }));
    defaultLineGroupId.value = selectedLineGroupIds.value.includes(plan.defaultLineGroupId)
      ? plan.defaultLineGroupId
      : selectedLineGroupIds.value[0] ?? '';
    authorizationDialogVisible.value = true;
  }

  function syncDraftBindings(lineGroupIds: string[]) {
    const existing = new Map(draftBindings.value.map((binding) => [binding.lineGroupId, binding]));
    draftBindings.value = lineGroupIds.map(
      (lineGroupId) => existing.get(lineGroupId) ?? {
        lineGroupId,
        billingMultiplier: 1,
      },
    );
    if (!lineGroupIds.includes(defaultLineGroupId.value)) {
      defaultLineGroupId.value = lineGroupIds[0] ?? '';
    }
  }

  function removeDraftBinding(lineGroupId: string) {
    selectedLineGroupIds.value = selectedLineGroupIds.value.filter((id) => id !== lineGroupId);
    syncDraftBindings(selectedLineGroupIds.value);
  }

  async function savePlan() {
    const draft = planForm.value;
    if (!draft.name.trim()) {
      ElMessage.warning('请输入套餐名称');
      return;
    }

    if (!draft.currency.trim()) {
      ElMessage.warning('请输入币种');
      return;
    }

    saving.value = true;
    try {
      // 编辑时若用户没动流量字段（草稿 GB 仍等于原套餐回填值），保留原始精确字节，
      // 不让 gbToBytes(round(...)) 把非整 GB 套餐的字节额度悄悄改掉。
      const original = editingPlan.value;
      const trafficUnchanged = planDialogMode.value === 'edit'
        && original !== undefined
        && draft.trafficLimitGb === Number(original.trafficLimitGb.toFixed(2));

      const payload: AdminPlanPayload = {
        name: draft.name,
        enabled: draft.enabled,
        trafficLimitGb: draft.trafficLimitGb,
        trafficLimitBytesExact: trafficUnchanged ? original.trafficLimitBytes : undefined,
        // 上下行直接生效;对称列 rateLimitMbps 仅作旧展示兜底,取上下行较大值(0/0=不限)。
        rateLimitMbps: Math.max(draft.rateLimitUpMbps, draft.rateLimitDownMbps),
        rateLimitUpMbps: draft.rateLimitUpMbps,
        rateLimitDownMbps: draft.rateLimitDownMbps,
        billingMultiplier: draft.multiplier,
        price: draft.price,
        currency: draft.currency,
        durationDays: draft.durationDays,
        sortWeight: draft.sortWeight,
      };

      if (planDialogMode.value === 'create') {
        await apiClient.createAdminPlan(payload);
        ElMessage.success('套餐已新增');
      } else {
        await apiClient.updateAdminPlan(draft.id, payload);
        ElMessage.success('套餐基础字段已更新');
      }

      planDialogVisible.value = false;
      await loadPlans();
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '保存套餐失败');
    } finally {
      saving.value = false;
    }
  }

  async function saveAuthorization() {
    if (!editingPlan.value) {
      return;
    }

    saving.value = true;
    try {
      if (draftBindings.value.length > 0 && !defaultLineGroupId.value) {
        defaultLineGroupId.value = draftBindings.value[0].lineGroupId;
      }
      await apiClient.replacePlanLineGroups(
        editingPlan.value.id,
        draftBindings.value.map((binding) => ({
          lineGroupId: binding.lineGroupId,
          billingMultiplier: normalizeBillingMultiplier(binding.billingMultiplier),
        })),
        defaultLineGroupId.value,
      );
      ElMessage.success('套餐分组已更新');
      authorizationDialogVisible.value = false;
      await loadPlans();
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '保存套餐分组失败');
    } finally {
      saving.value = false;
    }
  }

  function explainDefaultDelete(plan: PlanInfo) {
    if (!plan.isDefault) {
      return;
    }

    ElMessage.info('基础套餐不能删除，可编辑基础字段或调整分组。');
  }

  async function deletePlan(plan: PlanInfo) {
    if (plan.isDefault) {
      explainDefaultDelete(plan);
      return;
    }

    try {
      await ElMessageBox.confirm(`确认删除套餐「${plan.name}」？删除后将由后端按规则处理关联订阅。`, '删除套餐', {
        type: 'warning',
        confirmButtonText: '删除',
        cancelButtonText: '取消',
      });
    } catch {
      return;
    }

    deletingId.value = plan.id;
    try {
      await apiClient.deleteAdminPlan(plan.id);
      ElMessage.success('套餐已删除');
      await loadPlans();
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '删除套餐失败');
    } finally {
      deletingId.value = '';
    }
  }

  function normalizeBillingMultiplier(value: number) {
    const multiplier = Number(value);
    return Number.isFinite(multiplier) && multiplier > 0 ? Number(multiplier.toFixed(3)) : 1;
  }

  return {
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
  };
}
