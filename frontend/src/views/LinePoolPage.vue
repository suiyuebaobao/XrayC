<!--
  本文件是后台“出口管理”页面入口。
  页面主列表展示可被分组选择的线路。
  管理员可在这里新增、查看、编辑、删除和探测线路。
  用户订阅和公开接口不能暴露这些线路的连接凭据。
-->
<script setup lang="ts">
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import {
  apiClient,
  type ExitEndpointConfig,
  type ExitEndpointSummary,
  type ExitResourceSummary,
  type ThirdPartyExitEndpointOutboundType,
} from '@/services/api';
import {
  defaultOutboundConfig,
  ensureNoHongKongHysteria,
  outboundOptionsForRegion,
  parseJsonObject,
  protocolConfigHint,
  setVlessOutboundSecurity,
  validateProtocolConfig,
  vlessSecurityFromConfigText,
  vlessSecurityOptions,
} from './exit-pools/exitPoolsPageHelpers';
import { formatJson } from './line-pool/format';
import { parseThirdPartyLines, type ParsedThirdPartyLine } from './line-pool/importParser';
import ExitEndpointsTable from './line-pool/ExitEndpointsTable.vue';

const loading = ref(true);
const saving = ref(false);
const dialogOpen = ref(false);
const editingEndpointId = ref('');
const editingResourceId = ref('');
const exitResources = ref<ExitResourceSummary[]>([]);
const exitEndpoints = ref<ExitEndpointSummary[]>([]);
const rawImportText = ref('');
const importLines = ref<ParsedThirdPartyLine[]>([]);
const importErrors = ref<string[]>([]);

const form = reactive({
  resourceName: '',
  regionCode: '',
  providerName: '',
  ownership: 'third_party',
  outboundType: 'socks' as ThirdPartyExitEndpointOutboundType,
  host: '',
  port: 0,
  outboundConfig: defaultOutboundConfig('socks'),
  streamConfig: '{}',
  probeConfig: '{}',
  enabled: true,
});

const protocolHint = computed(() => protocolConfigHint(form.outboundType));
const vlessSecurityMode = computed({
  get: () => vlessSecurityFromConfigText(form.outboundConfig),
  set: (value: string) => {
    form.outboundConfig = setVlessOutboundSecurity(
      form.outboundConfig,
      value === 'none' || value === 'tls' || value === 'reality' ? value : 'reality',
    );
  },
});
const lineOutboundOptions = computed(() => outboundOptionsForRegion(form.regionCode));
const dialogTitle = computed(() => editingEndpointId.value ? '查看/编辑线路' : '添加线路');
const totalExitEndpoints = computed(() => exitEndpoints.value.length);
const enabledExitEndpoints = computed(() =>
  exitEndpoints.value.filter((endpoint) => endpoint.enabled && endpoint.exitResourceEnabled).length,
);
const regionCount = computed(() => new Set(
  exitResources.value
    .map((resource) => resource.region)
    .filter((region) => region.trim()),
).size);
const resourceById = computed(() => {
  const items = new Map<string, ExitResourceSummary>();
  for (const resource of exitResources.value) {
    items.set(resource.id, resource);
  }
  return items;
});

onMounted(load);

async function load() {
  loading.value = true;
  try {
    const [resources, endpoints] = await Promise.all([
      apiClient.getExitResources(),
      apiClient.getExitEndpoints(),
    ]);
    exitResources.value = resources;
    exitEndpoints.value = endpoints;
  } finally {
    loading.value = false;
  }
}

function openCreateDialog() {
  editingEndpointId.value = '';
  editingResourceId.value = '';
  rawImportText.value = '';
  importLines.value = [];
  importErrors.value = [];
  Object.assign(form, {
    resourceName: '',
    regionCode: '',
    providerName: '',
    ownership: 'third_party',
    outboundType: 'socks',
    host: '',
    port: 0,
    outboundConfig: defaultOutboundConfig('socks'),
    streamConfig: '{}',
    probeConfig: '{}',
    enabled: true,
  });
  dialogOpen.value = true;
}

function openEditDialog(endpoint: ExitEndpointSummary) {
  if (endpoint.outboundType === 'direct') {
    ElMessage.warning('历史 direct 线路只读，请新建普通线路替换。');
    return;
  }
  const resource = resourceById.value.get(endpoint.exitResourceId);
  editingEndpointId.value = endpoint.id;
  editingResourceId.value = endpoint.exitResourceId;
  rawImportText.value = '';
  importLines.value = [];
  importErrors.value = [];
  Object.assign(form, {
    resourceName: resource?.name || endpoint.resourceName || endpoint.name,
    regionCode: resource?.region || '',
    providerName: resource?.providerName || '',
    ownership: resource?.ownership || 'third_party',
    outboundType: endpoint.outboundType as ThirdPartyExitEndpointOutboundType,
    host: endpoint.host,
    port: endpoint.port,
    outboundConfig: formatJson(endpoint.outboundConfig),
    streamConfig: formatJson(endpoint.streamConfig),
    probeConfig: formatJson(endpoint.probeConfig),
    enabled: endpoint.enabled && endpoint.exitResourceEnabled,
  });
  dialogOpen.value = true;
}

function handleRawImportInput() {
  const result = parseThirdPartyLines(rawImportText.value);
  importLines.value = result.lines;
  importErrors.value = result.errors;
  if (result.lines[0]) {
    applyParsedLine(result.lines[0]);
  }
}

function applyParsedLine(line: ParsedThirdPartyLine) {
  Object.assign(form, {
    resourceName: line.resourceName,
    regionCode: line.regionCode,
    providerName: line.providerName,
    ownership: 'third_party',
    outboundType: line.outboundType,
    host: line.host,
    port: line.port,
    outboundConfig: formatJson(line.outboundConfig),
    streamConfig: formatJson(line.streamConfig),
    probeConfig: formatJson(line.probeConfig),
    enabled: true,
  });
  handleRegionChange();
}

function handleOutboundTypeChange(value: ThirdPartyExitEndpointOutboundType) {
  if (value === 'hysteria') {
    try {
      ensureNoHongKongHysteria(value, form.regionCode);
    } catch (error) {
      ElMessage.warning(error instanceof Error ? error.message : '香港线路不支持 HY2');
      form.outboundType = 'vless';
      form.outboundConfig = defaultOutboundConfig('vless');
      form.streamConfig = '{}';
      form.probeConfig = '{}';
      return;
    }
  }
  form.outboundConfig = defaultOutboundConfig(value);
  form.streamConfig = '{}';
  form.probeConfig = '{}';
}

function handleRegionChange() {
  if (form.outboundType === 'hysteria') {
    try {
      ensureNoHongKongHysteria(form.outboundType, form.regionCode);
    } catch (error) {
      ElMessage.warning(error instanceof Error ? error.message : '香港线路不支持 HY2');
      handleOutboundTypeChange('vless');
    }
  }
}

async function submitLine() {
  saving.value = true;
  try {
    const outboundConfig = parseJsonObject(form.outboundConfig);
    ensureNoHongKongHysteria(form.outboundType, form.regionCode);
    validateProtocolConfig(form.outboundType, outboundConfig);
    if (!form.host.trim() || Number(form.port || 0) <= 0) {
      throw new Error('线路地址和端口不能为空。');
    }

    if (editingEndpointId.value) {
      await updateLine(outboundConfig);
    } else {
      await createNewLine(outboundConfig);
    }
    dialogOpen.value = false;
    await load();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存线路失败');
  } finally {
    saving.value = false;
  }
}

async function createNewLine(outboundConfig: ExitEndpointConfig) {
  const resourceId = await apiClient.createExitResource({
    name: form.resourceName,
    region_code: form.regionCode,
    provider_name: form.providerName,
    ownership: form.ownership,
    enabled: form.enabled,
  });
  await apiClient.createExitEndpoint({
    exit_resource_id: resourceId,
    name: form.resourceName,
    outbound_type: form.outboundType,
    host: form.host,
    port: Number(form.port),
    outbound_config: outboundConfig,
    stream_config: parseJsonObject(form.streamConfig),
    probe_config: parseJsonObject(form.probeConfig),
    enabled: form.enabled,
  });
  ElMessage.success('线路已加入出口管理');
}

async function updateLine(outboundConfig: ExitEndpointConfig) {
  await apiClient.updateExitResource(editingResourceId.value, {
    name: form.resourceName,
    region_code: form.regionCode,
    provider_name: form.providerName,
    ownership: form.ownership,
    enabled: form.enabled,
  });
  await apiClient.updateExitEndpoint(editingEndpointId.value, {
    exit_resource_id: editingResourceId.value,
    name: form.resourceName,
    outbound_type: form.outboundType,
    host: form.host,
    port: Number(form.port),
    outbound_config: outboundConfig,
    stream_config: parseJsonObject(form.streamConfig),
    probe_config: parseJsonObject(form.probeConfig),
    enabled: form.enabled,
  });
  ElMessage.success('线路已更新，关联中转节点将自动重新同步');
}

async function deleteLine(endpoint: ExitEndpointSummary) {
  try {
    await ElMessageBox.confirm(
      `确认删除线路「${endpoint.resourceName || endpoint.name}」？系统会同步清理分组成员、中转入口绑定和用户入口/出口分配。`,
      '删除线路',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    );
  } catch {
    return;
  }
  await apiClient.deleteExitEndpoint(endpoint.id);
  ElMessage.success('线路已删除，关联分组、中转入口绑定和用户入口/出口分配已同步清理');
  await load();
}

async function triggerProbe(endpoint: ExitEndpointSummary) {
  await apiClient.triggerExitEndpointProbe(endpoint.id);
  ElMessage.success('已触发线路探测');
}
</script>

<template>
  <PageHeader title="出口管理" description="在这里添加、查询、编辑和删除出口线路；分组从这里选择出口线路用于套餐授权和归类，中转节点直接绑定具体线路。">
    <el-button @click="load">刷新</el-button>
    <el-button type="primary" @click="openCreateDialog">添加线路</el-button>
  </PageHeader>

  <el-row :gutter="16" class="line-pool-summary">
    <el-col :xs="24" :sm="12" :md="6">
      <el-card shadow="never"><span>线路</span><strong>{{ totalExitEndpoints }}</strong></el-card>
    </el-col>
    <el-col :xs="24" :sm="12" :md="6">
      <el-card shadow="never"><span>可用线路</span><strong>{{ enabledExitEndpoints }}</strong></el-card>
    </el-col>
    <el-col :xs="24" :sm="12" :md="6">
      <el-card shadow="never"><span>地区</span><strong>{{ regionCount }}</strong></el-card>
    </el-col>
  </el-row>

  <ExitEndpointsTable
    :endpoints="exitEndpoints"
    :resources="exitResources"
    :loading="loading"
    @edit="openEditDialog"
    @delete="deleteLine"
    @probe="triggerProbe"
  />

  <el-dialog v-model="dialogOpen" :title="dialogTitle" width="840px">
    <el-alert
      class="line-pool-alert"
      type="info"
      show-icon
      :closable="false"
      title="出口管理是唯一的线路维护入口；删除或修改后，关联分组和中转节点会自动同步。"
    />
    <el-form label-position="top">
      <el-row :gutter="16">
        <el-col v-if="!editingEndpointId" :span="24">
          <el-form-item label="原始线路配置">
            <el-input
              v-model="rawImportText"
              type="textarea"
              :rows="7"
              placeholder="粘贴 Clash/mihomo YAML、vless://、trojan://、ss://、hy2://、socks5://、Xray outbound JSON 或半结构化文本，系统会自动识别并预填下面的表单。"
              @input="handleRawImportInput"
            />
          </el-form-item>
          <el-alert
            v-if="importLines.length > 0"
            class="line-pool-alert"
            type="success"
            show-icon
            :closable="false"
            :title="`已自动识别 ${importLines.length} 条线路，当前预填第 1 条：${importLines[0].resourceName}`"
          />
          <el-alert
            v-else-if="importErrors.length > 0"
            class="line-pool-alert"
            type="warning"
            show-icon
            :closable="false"
            :title="importErrors[0]"
          />
        </el-col>
        <el-col :span="12">
          <el-form-item label="线路名称"><el-input v-model="form.resourceName" /></el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="供应商"><el-input v-model="form.providerName" /></el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="地区代码"><el-input v-model="form.regionCode" @change="handleRegionChange" /></el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="协议">
            <el-select v-model="form.outboundType" class="line-pool-field" @change="handleOutboundTypeChange">
              <el-option
                v-for="option in lineOutboundOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
                :disabled="option.disabled"
              />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col v-if="form.outboundType === 'vless'" :span="12">
          <el-form-item label="VLESS 安全模式">
            <el-select v-model="vlessSecurityMode" class="line-pool-field">
              <el-option
                v-for="option in vlessSecurityOptions"
                :key="option.value"
                :label="option.label"
                :value="option.value"
              />
            </el-select>
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="地址"><el-input v-model="form.host" /></el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="端口">
            <el-input-number v-model="form.port" :min="1" :max="65535" class="line-pool-field" />
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="状态"><el-switch v-model="form.enabled" active-text="启用" inactive-text="停用" /></el-form-item>
        </el-col>
        <el-col :span="24">
          <el-form-item label="连接配置 JSON">
            <el-alert :title="protocolHint" type="info" show-icon :closable="false" class="line-pool-alert" />
            <el-input v-model="form.outboundConfig" type="textarea" :rows="6" />
          </el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="Stream 配置 JSON"><el-input v-model="form.streamConfig" type="textarea" :rows="5" /></el-form-item>
        </el-col>
        <el-col :span="12">
          <el-form-item label="探测配置 JSON"><el-input v-model="form.probeConfig" type="textarea" :rows="5" /></el-form-item>
        </el-col>
      </el-row>
    </el-form>
    <template #footer>
      <el-button @click="dialogOpen = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="submitLine">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.line-pool-summary {
  margin-bottom: 16px;
}

.line-pool-summary span {
  color: #6b7280;
}

.line-pool-summary strong {
  display: block;
  margin-top: 8px;
  font-size: 28px;
}

.line-pool-alert {
  margin-bottom: 14px;
}

.line-pool-field {
  width: 100%;
}

.line-pool-credential,
.line-pool-config-json {
  white-space: pre-wrap;
  word-break: break-word;
}

.line-pool-config-json {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
}
</style>
