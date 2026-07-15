// 入口↔出口线路绑定弹窗的状态与操作逻辑。
// 从 AccessEntriesPage 抽出：维护绑定表单、打开绑定弹窗、保存绑定、删除绑定，
// 以及出口线路下拉的展示标签。只负责绑定弹窗这一块，与入口创建/编辑表单解耦。
import { ElMessage } from 'element-plus';
import { reactive, ref, type Ref } from 'vue';
import {
  apiClient,
  type AccessEntryExitBindingSummary,
  type AccessEntrySummary,
  type ExitEndpointSummary,
} from '@/services/api';

// 调用方注入的依赖：当前选中入口（只读计算）、出口线路列表、选中入口 id（可写）、刷新回调、保存中标记。
interface UseEntryExitBindingDeps {
  selectedEntry: Ref<AccessEntrySummary | null>;
  exitEndpoints: Ref<ExitEndpointSummary[]>;
  selectedEntryId: Ref<string>;
  load: () => Promise<void>;
  saving: Ref<boolean>;
}

export function useEntryExitBinding(deps: UseEntryExitBindingDeps) {
  const { selectedEntry, selectedEntryId, load, saving } = deps;

  const bindingDialogOpen = ref(false);
  const bindingForm = reactive({
    exitEndpointId: '',
    name: '',
    remark: '',
    enabled: true,
    sortWeight: 100,
  });

  function openBindExitDialog(entry?: AccessEntrySummary) {
    const targetEntry = entry ?? selectedEntry.value;
    if (!targetEntry) {
      ElMessage.warning('请先选择入口');
      return;
    }
    selectedEntryId.value = targetEntry.id;
    Object.assign(bindingForm, {
      exitEndpointId: '',
      name: '',
      remark: '',
      enabled: true,
      sortWeight: 100,
    });
    bindingDialogOpen.value = true;
  }

  async function saveBinding() {
    const entry = selectedEntry.value;
    if (!entry) {
      ElMessage.warning('请先选择入口');
      return;
    }
    if (!bindingForm.exitEndpointId) {
      ElMessage.warning('请选择出口线路');
      return;
    }
    saving.value = true;
    try {
      await apiClient.createAccessEntryExitBinding(entry.id, {
        exit_endpoint_id: bindingForm.exitEndpointId,
        name: bindingForm.name.trim(),
        remark: bindingForm.remark.trim(),
        enabled: bindingForm.enabled,
        sort_weight: Number(bindingForm.sortWeight || 100),
      });
      bindingDialogOpen.value = false;
      await load();
      ElMessage.success('线路已绑定');
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '绑定线路失败');
    } finally {
      saving.value = false;
    }
  }

  async function deleteBinding(binding: AccessEntryExitBindingSummary) {
    saving.value = true;
    try {
      await apiClient.deleteAccessEntryExitBinding(binding.id);
      await load();
      ElMessage.success('绑定已删除');
    } catch (error) {
      ElMessage.error(error instanceof Error ? error.message : '删除绑定失败');
    } finally {
      saving.value = false;
    }
  }

  function exitEndpointLabel(endpoint: ExitEndpointSummary) {
    const host = endpoint.host ? `${endpoint.host}:${endpoint.port || ''}` : '未提供地址';
    return `${endpoint.name || endpoint.resourceName || endpoint.id} / ${host}`;
  }

  return {
    bindingDialogOpen,
    bindingForm,
    openBindExitDialog,
    saveBinding,
    deleteBinding,
    exitEndpointLabel,
  };
}
