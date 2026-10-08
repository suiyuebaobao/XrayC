<!--
  本页面维护订阅通用规则和可复用规则库。
  通用规则保存到订阅设置 default_rules，规则库通过分组绑定使用。
  分组不再直接编辑 dedicated_rules，避免同一规则在多个分组里重复维护。
-->
<script setup lang="ts">
import { Delete, Edit, Plus, Refresh } from '@element-plus/icons-vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type SubscriptionRuleSet, type SubscriptionSettings } from '@/services/api';
import RuleEditor from './subscription-settings/RuleEditor.vue';

const loading = ref(true);
const savingCommon = ref(false);
const savingRuleSet = ref(false);
const ruleSetDialogOpen = ref(false);
const editingRuleSetId = ref('');
const ruleSets = ref<SubscriptionRuleSet[]>([]);

const settings = reactive<SubscriptionSettings>({
  mixedPort: 7890,
  allowLan: false,
  mode: 'rule',
  logLevel: 'info',
  profileName: 'XrayC',
  updateIntervalHours: 24,
  defaultRules: [],
  autoTestEnabled: false,
  autoTestName: '自动选择',
  autoTestUrl: 'http://cp.cloudflare.com/generate_204',
  autoTestIntervalSeconds: 86400,
  blockUnhealthyLines: false,
});

const ruleSetForm = reactive({
  name: '',
  description: '',
  enabled: true,
  rules: [] as string[],
});

const sortedRuleSets = computed(() =>
  [...ruleSets.value].sort((left, right) =>
    Number(right.enabled) - Number(left.enabled)
    || left.name.localeCompare(right.name, 'zh-Hans-CN')
    || left.id.localeCompare(right.id),
  ),
);

const ruleSetDialogTitle = computed(() => editingRuleSetId.value ? '编辑规则库' : '创建规则库');

onMounted(load);

async function load() {
  loading.value = true;
  try {
    const [nextSettings, nextRuleSets] = await Promise.all([
      apiClient.getSubscriptionSettings(),
      apiClient.listSubscriptionRuleSets(),
    ]);
    applySettings(nextSettings);
    ruleSets.value = nextRuleSets;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载规则设置失败');
  } finally {
    loading.value = false;
  }
}

async function saveCommonRules() {
  savingCommon.value = true;
  try {
    applySettings(await apiClient.updateSubscriptionSettings({ defaultRules: [...settings.defaultRules] }));
    ElMessage.success('通用规则已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存通用规则失败');
  } finally {
    savingCommon.value = false;
  }
}

function openCreateRuleSetDialog() {
  editingRuleSetId.value = '';
  Object.assign(ruleSetForm, {
    name: '',
    description: '',
    enabled: true,
    rules: [],
  });
  ruleSetDialogOpen.value = true;
}

function openEditRuleSetDialog(ruleSet: SubscriptionRuleSet) {
  editingRuleSetId.value = ruleSet.id;
  Object.assign(ruleSetForm, {
    name: ruleSet.name,
    description: ruleSet.description,
    enabled: ruleSet.enabled,
    rules: [...ruleSet.rules],
  });
  ruleSetDialogOpen.value = true;
}

async function saveRuleSet() {
  const name = ruleSetForm.name.trim();
  if (!name) {
    ElMessage.warning('请输入规则库名称');
    return;
  }
  savingRuleSet.value = true;
  try {
    const payload = {
      name,
      description: ruleSetForm.description,
      enabled: ruleSetForm.enabled,
      rules: [...ruleSetForm.rules],
    };
    if (editingRuleSetId.value) {
      await apiClient.updateSubscriptionRuleSet(editingRuleSetId.value, payload);
    } else {
      await apiClient.createSubscriptionRuleSet(payload);
    }
    ruleSetDialogOpen.value = false;
    await load();
    ElMessage.success(editingRuleSetId.value ? '规则库已保存' : '规则库已创建');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存规则库失败');
  } finally {
    savingRuleSet.value = false;
  }
}

async function deleteRuleSet(ruleSet: SubscriptionRuleSet) {
  if (ruleSet.bindingCount > 0) {
    ElMessage.warning('规则库已被分组绑定，请先解除绑定');
    return;
  }
  try {
    await ElMessageBox.confirm(`确认删除规则库「${ruleSet.name}」？`, '删除规则库', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  savingRuleSet.value = true;
  try {
    await apiClient.deleteSubscriptionRuleSet(ruleSet.id);
    await load();
    ElMessage.success('规则库已删除');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '删除规则库失败');
  } finally {
    savingRuleSet.value = false;
  }
}

function applySettings(next: SubscriptionSettings) {
  settings.mixedPort = next.mixedPort;
  settings.allowLan = next.allowLan;
  settings.mode = next.mode === 'global' ? 'global' : 'rule';
  settings.logLevel = next.logLevel || 'info';
  settings.profileName = next.profileName || 'XrayC';
  settings.updateIntervalHours = next.updateIntervalHours || 24;
  settings.defaultRules = [...next.defaultRules];
  settings.autoTestEnabled = next.autoTestEnabled;
  settings.autoTestName = next.autoTestName || '自动选择';
  settings.autoTestUrl = next.autoTestUrl || 'http://cp.cloudflare.com/generate_204';
  settings.autoTestIntervalSeconds = next.autoTestIntervalSeconds || 86400;
  settings.blockUnhealthyLines = next.blockUnhealthyLines;
}
</script>

<template>
  <PageHeader title="规则设置" description="维护通用规则和可复用规则库；规则库在分组里绑定，未命中时走套餐默认分组。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :icon="Plus" @click="openCreateRuleSetDialog">创建规则库</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="12" animated />
  <div v-else class="rule-settings-layout">
    <section data-testid="common-rules-section">
      <RuleEditor
        v-model="settings.defaultRules"
        title="通用规则"
        description="通用规则在分组绑定规则之后匹配；直连和拒绝会保持原动作，未命中时系统自动兜底走套餐默认分组。"
      />
      <div class="rule-settings-actions">
        <el-button type="primary" :loading="savingCommon" @click="saveCommonRules">保存通用规则</el-button>
      </div>
    </section>

    <section data-testid="rule-library-section">
      <div class="rule-library-header">
        <h2>规则库</h2>
        <el-button type="primary" :icon="Plus" @click="openCreateRuleSetDialog">创建规则库</el-button>
      </div>
      <el-table :data="sortedRuleSets" row-key="id" stripe>
        <el-table-column label="名称" min-width="180">
          <template #default="{ row }: { row: SubscriptionRuleSet }">
            <div class="rule-set-name">
              <strong>{{ row.name }}</strong>
              <small v-if="row.description">{{ row.description }}</small>
            </div>
          </template>
        </el-table-column>
        <el-table-column label="规则" width="90">
          <template #default="{ row }: { row: SubscriptionRuleSet }">{{ row.rules.length }} 条</template>
        </el-table-column>
        <el-table-column label="绑定" width="90">
          <template #default="{ row }: { row: SubscriptionRuleSet }">{{ row.bindingCount }} 个分组</template>
        </el-table-column>
        <el-table-column label="状态" width="90">
          <template #default="{ row }: { row: SubscriptionRuleSet }">
            <el-tag :type="row.enabled ? 'success' : 'info'" effect="plain">{{ row.enabled ? '启用' : '停用' }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="160" fixed="right">
          <template #default="{ row }: { row: SubscriptionRuleSet }">
            <el-button text type="primary" :icon="Edit" @click="openEditRuleSetDialog(row)">编辑</el-button>
            <el-button text type="danger" :icon="Delete" @click="deleteRuleSet(row)">删除</el-button>
          </template>
        </el-table-column>
      </el-table>
    </section>
  </div>

  <el-dialog v-model="ruleSetDialogOpen" :title="ruleSetDialogTitle" width="760px">
    <el-form label-position="top">
      <el-form-item label="规则库名称">
        <el-input v-model="ruleSetForm.name" placeholder="例如：AI 规则、游戏规则、国内直连规则" />
      </el-form-item>
      <el-form-item label="备注">
        <el-input v-model="ruleSetForm.description" type="textarea" :rows="2" />
      </el-form-item>
      <el-form-item label="启用状态">
        <el-switch v-model="ruleSetForm.enabled" active-text="启用" inactive-text="停用" />
      </el-form-item>
    </el-form>
    <RuleEditor
      v-model="ruleSetForm.rules"
      title="规则内容"
      description="规则库可绑定到多个分组；“走代理”会在生成订阅时指向当前绑定的分组。"
      proxy-action-label="走绑定分组"
      :allow-match="false"
    />
    <template #footer>
      <el-button @click="ruleSetDialogOpen = false">取消</el-button>
      <el-button type="primary" :loading="savingRuleSet" @click="saveRuleSet">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.rule-settings-layout {
  display: grid;
  gap: 16px;
}

.rule-settings-actions,
.rule-library-header {
  align-items: center;
  display: flex;
  justify-content: space-between;
  margin-top: 12px;
}

.rule-library-header {
  margin: 0 0 12px;
}

.rule-library-header h2 {
  font-size: 18px;
  font-weight: 600;
  margin: 0;
}

.rule-set-name {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.rule-set-name small {
  color: #6b7280;
}
</style>
