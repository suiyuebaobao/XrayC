<!--
  本页面维护扁平分组。
  分组没有层级区别，AI、游戏、GPT、视频等分组都选择入口+出口绑定节点。
-->
<script setup lang="ts">
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import {
  apiClient,
  type AccessEntryExitBindingSummary,
  type LineGroupSummary,
  type SubscriptionRuleSet,
} from '@/services/api';
import { countryOptions } from '@/views/shared/countryOptions';

type GroupFormMode = 'create' | 'edit';

const loading = ref(false);
const saving = ref(false);
const dialogOpen = ref(false);
const mode = ref<GroupFormMode>('create');
const groups = ref<LineGroupSummary[]>([]);
const bindingNodes = ref<AccessEntryExitBindingSummary[]>([]);
const ruleSets = ref<SubscriptionRuleSet[]>([]);
const form = reactive({
  id: '',
  name: '',
  countryCode: 'GLOBAL',
  enabled: true,
  sortOrder: 100,
  bindingNodeIds: [] as string[],
  ruleSetIds: [] as string[],
});

const visibleGroups = computed(() =>
  [...groups.value]
    .sort((left, right) => left.sortOrder - right.sortOrder || left.name.localeCompare(right.name, 'zh-Hans-CN')),
);
const groupBindingNodeCount = computed(() =>
  visibleGroups.value.reduce((total, group) => total + group.bindingNodeIds.length, 0),
);
const dialogTitle = computed(() => mode.value === 'create' ? '创建分组' : '编辑分组');

onMounted(load);

async function load() {
  loading.value = true;
  try {
    const [controlPlane, nextBindingNodes, nextRuleSets] = await Promise.all([
      apiClient.getControlPlane(),
      apiClient.listAccessEntryExitBindings(),
      apiClient.listSubscriptionRuleSets(),
    ]);
    groups.value = controlPlane.lineGroups;
    bindingNodes.value = nextBindingNodes;
    ruleSets.value = nextRuleSets;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载分组失败');
  } finally {
    loading.value = false;
  }
}

function openCreateDialog() {
  mode.value = 'create';
  Object.assign(form, {
    id: '',
    name: '',
    countryCode: 'GLOBAL',
    enabled: true,
    sortOrder: nextSortOrder(),
    bindingNodeIds: [],
    ruleSetIds: [],
  });
  dialogOpen.value = true;
}

function openEditDialog(group: LineGroupSummary) {
  mode.value = 'edit';
  Object.assign(form, {
    id: group.id,
    name: group.name,
    countryCode: group.countryCode || 'GLOBAL',
    enabled: group.enabled,
    sortOrder: group.sortOrder || 100,
    bindingNodeIds: [...group.bindingNodeIds],
    ruleSetIds: [...group.ruleSetBindings]
      .sort((left, right) => left.position - right.position || left.ruleSetName.localeCompare(right.ruleSetName, 'zh-Hans-CN'))
      .map((binding) => binding.ruleSetId),
  });
  dialogOpen.value = true;
}

async function saveGroup() {
  const name = form.name.trim();
  if (!name) {
    ElMessage.warning('请输入分组名称');
    return;
  }
  saving.value = true;
  try {
    const payload = {
      name,
      country_code: form.countryCode,
      enabled: form.enabled,
      sort_weight: form.sortOrder,
      rule_set_bindings: form.ruleSetIds.map((ruleSetId, index) => ({
        rule_set_id: ruleSetId,
        position: (index + 1) * 100,
        enabled: true,
      })),
    };
    const groupId = mode.value === 'create'
      ? await apiClient.createLineGroup(payload)
      : form.id;
    if (mode.value === 'edit') {
      await apiClient.updateLineGroup(form.id, payload);
    }
    await apiClient.replaceLineGroupBindingNodes(groupId, form.bindingNodeIds);
    dialogOpen.value = false;
    await load();
    ElMessage.success(mode.value === 'create' ? '分组已创建' : '分组已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存分组失败');
  } finally {
    saving.value = false;
  }
}

async function deleteGroup(group: LineGroupSummary) {
  try {
    await ElMessageBox.confirm(`确认删除分组「${group.name}」？套餐授权会同步剔除。`, '删除分组', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  saving.value = true;
  try {
    await apiClient.deleteLineGroup(group.id);
    await load();
    ElMessage.success('分组已删除');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '删除分组失败');
  } finally {
    saving.value = false;
  }
}

function bindingOptionValue(binding: AccessEntryExitBindingSummary) {
  return binding.id;
}

function bindingOptionLabel(binding: AccessEntryExitBindingSummary) {
  return `${bindingDisplayName(binding)} / ${bindingDetailLabel(binding)}`;
}

function bindingDisplayName(binding: AccessEntryExitBindingSummary) {
  const entryName = binding.accessEntryName || binding.accessEntryId || '未命名入口';
  const exitName = binding.exitEndpointName || binding.exitEndpointId || '未命名出口';
  return binding.name || `${entryName} + ${exitName}`;
}

function bindingDetailLabel(binding: AccessEntryExitBindingSummary) {
  const accessNodeName = binding.accessNodeName || binding.accessNodeId || '未命名接入节点';
  const entryName = binding.accessEntryName || binding.accessEntryId || '未命名入口';
  const exitName = binding.exitEndpointName || binding.exitEndpointId || '未命名出口';
  return `${accessNodeName} / ${entryName} + ${exitName}`;
}

function bindingNodeCountText(group: LineGroupSummary) {
  return `${group.bindingNodeIds.length} 条`;
}

function ruleSetNames(group: LineGroupSummary) {
  return [...group.ruleSetBindings]
    .sort((left, right) => left.position - right.position || left.ruleSetName.localeCompare(right.ruleSetName, 'zh-Hans-CN'))
    .map((binding) => binding.ruleSetName)
    .filter(Boolean);
}

function nextSortOrder() {
  return visibleGroups.value.reduce((max, group) => Math.max(max, group.sortOrder || 0), 90) + 10;
}

</script>

<template>
  <PageHeader title="分组" description="分组选择入口+出口绑定节点，可用于 AI、游戏、GPT、视频等不同用途。">
    <el-button :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" @click="openCreateDialog">创建分组</el-button>
  </PageHeader>

  <el-row :gutter="16" class="line-groups-summary">
    <el-col :xs="24" :sm="8">
      <el-card shadow="never"><span>分组</span><strong>{{ visibleGroups.length }}</strong></el-card>
    </el-col>
    <el-col :xs="24" :sm="8">
      <el-card shadow="never"><span>分组内绑定节点</span><strong>{{ groupBindingNodeCount }}</strong></el-card>
    </el-col>
    <el-col :xs="24" :sm="8">
      <el-card shadow="never"><span>可选绑定节点</span><strong>{{ bindingNodes.length }}</strong></el-card>
    </el-col>
  </el-row>

  <el-card shadow="never">
    <el-table v-loading="loading" :data="visibleGroups" row-key="id" stripe>
      <el-table-column label="分组" min-width="230">
        <template #default="{ row }: { row: LineGroupSummary }">
          <div class="group-name">
            <strong>{{ row.name }}</strong>
            <small>{{ row.countryCode || 'GLOBAL' }}</small>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="绑定节点" width="110">
        <template #default="{ row }: { row: LineGroupSummary }">{{ bindingNodeCountText(row) }}</template>
      </el-table-column>
      <el-table-column label="规则库" min-width="180">
        <template #default="{ row }: { row: LineGroupSummary }">
          <div v-if="ruleSetNames(row).length > 0" class="rule-set-tags">
            <el-tag v-for="name in ruleSetNames(row)" :key="name" effect="plain">{{ name }}</el-tag>
          </div>
          <span v-else class="muted-text">未绑定</span>
        </template>
      </el-table-column>
      <el-table-column label="状态" width="90">
        <template #default="{ row }: { row: LineGroupSummary }">
          <el-tag :type="row.enabled ? 'success' : 'info'" effect="plain">{{ row.enabled ? '启用' : '停用' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="sortOrder" label="排序" width="80" />
      <el-table-column label="操作" width="130" fixed="right">
        <template #default="{ row }: { row: LineGroupSummary }">
          <el-button text type="primary" @click="openEditDialog(row)">编辑</el-button>
          <el-button text type="danger" @click="deleteGroup(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>

  <el-dialog v-model="dialogOpen" :title="dialogTitle" width="720px">
    <el-form label-position="top">
      <el-row :gutter="16">
        <el-col :span="14">
          <el-form-item label="分组名称">
            <el-input v-model="form.name" placeholder="例如：AI 分组、游戏分组、GPT 分组、视频分组" />
          </el-form-item>
        </el-col>
        <el-col :span="10">
          <el-form-item label="启用状态">
            <el-switch v-model="form.enabled" active-text="启用" inactive-text="停用" />
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="国家/地区">
            <el-select
              v-model="form.countryCode"
              filterable
              class="line-group-field"
            >
              <el-option
                v-for="option in countryOptions"
                :key="option.code"
                :label="`${option.name} / ${option.code}`"
                :value="option.code"
              />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="排序">
            <el-input-number v-model="form.sortOrder" :min="0" :max="100000" class="line-group-field" />
          </el-form-item>
        </el-col>
        <el-col :span="24">
          <el-form-item label="绑定规则库">
            <el-select
              v-model="form.ruleSetIds"
              multiple
              filterable
              collapse-tags
              collapse-tags-tooltip
              class="line-group-field"
              placeholder="选择这个分组要使用的规则库"
            >
              <el-option
                v-for="ruleSet in ruleSets"
                :key="ruleSet.id"
                :label="ruleSet.name"
                :value="ruleSet.id"
                :disabled="!ruleSet.enabled"
              >
                <div class="line-option">
                  <strong>{{ ruleSet.name }}</strong>
                  <span>{{ ruleSet.rules.length }} 条规则</span>
                </div>
              </el-option>
            </el-select>
          </el-form-item>
        </el-col>
        <el-col :span="24">
          <el-form-item label="选择绑定节点">
            <el-select
              v-model="form.bindingNodeIds"
              multiple
              filterable
              collapse-tags
              collapse-tags-tooltip
              class="line-group-field"
              placeholder="选择入口+出口绑定节点"
            >
              <el-option
                v-for="binding in bindingNodes"
                :key="bindingOptionValue(binding)"
                :label="bindingOptionLabel(binding)"
                :value="bindingOptionValue(binding)"
                :disabled="!binding.enabled"
              >
                <div class="line-option">
                  <strong>{{ bindingDisplayName(binding) }}</strong>
                  <span>{{ bindingDetailLabel(binding) }}</span>
                </div>
              </el-option>
            </el-select>
          </el-form-item>
        </el-col>
      </el-row>
    </el-form>
    <template #footer>
      <el-button @click="dialogOpen = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="saveGroup">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.line-groups-summary {
  margin-bottom: 16px;
}

.line-groups-summary span,
.group-name small,
.line-option span,
.muted-text {
  color: #6b7280;
}

.line-groups-summary strong {
  display: block;
  margin-top: 8px;
  font-size: 28px;
}

.group-name {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.line-group-field {
  width: 100%;
}

.line-option {
  align-items: center;
  display: flex;
  justify-content: space-between;
  width: 100%;
}

.rule-set-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
</style>
