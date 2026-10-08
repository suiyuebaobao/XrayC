<!--
  本页面维护接入入口和入口到出口的绑定。
  入口负责客户端入站协议，绑定节点负责订阅可见的 entry + exit 组合。
-->
<script setup lang="ts">
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import PageHeader from '@/components/PageHeader.vue';
import AccessEntryDialog from '@/views/access-entries/AccessEntryDialog.vue';
import {
  entryPayloadForMode,
  entryTransportsCarriageFromMode,
  selectedEntryNetworkModes,
} from '@/views/access-entries/entryPayload';
import {
  CF_SUPPORTED_HTTPS_PORTS,
  entryAddressType,
  isCfSupportedHttpsPort,
  type EntryAddressType,
} from '@/views/access-entries/entryProtocolMatrix';
import {
  applyNodeAddressDefaults,
  applyProtocolFieldDefaults,
  createAddressDirtyFlags,
  nodeListenHostForMode,
  resetAddressDirtyFlags,
} from '@/views/access-entries/useEntryAddressAutofill';
import { useEntryExitBinding } from '@/views/access-entries/useEntryExitBinding';
import { useEntryPortDefaults } from '@/views/access-entries/useEntryPortDefaults';
import {
  apiClient,
  type AccessEntryExitBindingSummary,
  type AccessEntrySummary,
  type AccessNodeSummary,
  type ExitEndpointSummary,
  type ExitResourceSummary,
} from '@/services/api';

type EntryFormMode = 'create' | 'edit';

const route = useRoute();
const loading = ref(false);
const saving = ref(false);
const entryDialogOpen = ref(false);
const entryMode = ref<EntryFormMode>('create');
const selectedEntryId = ref('');
const accessNodes = ref<AccessNodeSummary[]>([]);
const entries = ref<AccessEntrySummary[]>([]);
const bindings = ref<AccessEntryExitBindingSummary[]>([]);
const exitEndpoints = ref<ExitEndpointSummary[]>([]);
const exitResources = ref<ExitResourceSummary[]>([]); // 出口资源：endpoint→节点映射，算本机出口占用端口

const entryForm = reactive({
  id: '',
  accessNodeId: '',
  name: '',
  listenHost: '',
  listenPort: 443,
  protocol: 'vless',
  transport: 'tcp',
  transports: ['tcp'],
  carriage: 'tcp',
  security: 'reality',
  serverName: '',
  vlessQuantumEncryption: false,
  wsPath: '',
  wsHost: '',
  cdnEnabled: false,
  cdnProvider: '',
  cdnHostname: '',
  nodeDomainId: '',
  // 连接方式（地址类型）：ip/domain/cf，显式驱动地址类型+选域名+协议+CDN，Dialog 据此归一。
  connectionMode: 'ip' as EntryAddressType,
  enabled: true,
  sortWeight: 100,
});

// 地址脏标记：管理员手动改过直连地址/CF 域名后置真，切换节点/协议时尊重不覆盖。
const addressDirty = createAddressDirtyFlags();

function findAccessNode(accessNodeId: string) {
  return accessNodes.value.find((node) => node.id === accessNodeId) ?? null;
}

// 当前选中节点域名的域名字符串（用于按连接方式带出监听地址；未选则空）。
function currentSelectedDomain(node: AccessNodeSummary | null): string {
  return node?.domains.find((domain) => domain.id === entryForm.nodeDomainId)?.domain ?? '';
}

// 当前入口是否为 CF（橙云）：按选中节点域名 kind 优先，未选时回退 cdnEnabled/地址形态推导，
// 与 AccessEntryDialog 的 addressType 同口径，用于提交前的 CF 端口护栏。
function isCfEntry(): boolean {
  const node = findAccessNode(entryForm.accessNodeId);
  const domain = node?.domains.find((item) => item.id === entryForm.nodeDomainId) ?? null;
  return entryAddressType({
    cdnEnabled: entryForm.cdnEnabled,
    listenHost: entryForm.listenHost,
    nodeDomainKind: domain?.kind ?? '',
  }) === 'cf';
}

const entryDialogTitle = computed(() => entryMode.value === 'create' ? '创建入口' : '编辑入口');
const selectedEntry = computed(() => entries.value.find((entry) => entry.id === selectedEntryId.value) ?? entries.value[0] ?? null);
const selectedBindings = computed(() => {
  const entryId = selectedEntry.value?.id ?? '';
  return bindings.value.filter((binding) => binding.accessEntryId === entryId);
});
const entriesByNode = computed(() =>
  [...entries.value].sort((left, right) =>
    left.accessNodeName.localeCompare(right.accessNodeName, 'zh-Hans-CN')
    || left.sortWeight - right.sortWeight
    || left.name.localeCompare(right.name, 'zh-Hans-CN'),
  ),
);

// 入口↔出口绑定弹窗逻辑抽到 composable，注入选中入口/出口列表/选中 id/刷新/保存中标记。
const {
  bindingDialogOpen,
  bindingForm,
  openBindExitDialog,
  saveBinding,
  deleteBinding,
  exitEndpointLabel,
} = useEntryExitBinding({ selectedEntry, exitEndpoints, selectedEntryId, load, saving });

// 智能默认端口 + 即时端口校验：新建态按「节点 + 连接方式」填未占用端口，编辑态保留现有端口。
const { portError, applyDefaultPort } = useEntryPortDefaults(
  { entryForm, entryMode, entries, exitEndpoints, exitResources },
);

onMounted(async () => { await load(); if (route.query.action === 'create') openCreateEntryDialog(); });
watch(() => route.query.entry, (id) => { if (typeof id === 'string' && entries.value.some((entry) => entry.id === id)) selectedEntryId.value = id; });

async function load() {
  loading.value = true;
  try {
    const [controlPlane, nextEntries, nextBindings, nextExitEndpoints, nextExitResources] = await Promise.all([
      apiClient.getControlPlane(),
      apiClient.listAccessEntries(),
      apiClient.listAccessEntryExitBindings(),
      apiClient.getExitEndpoints(),
      apiClient.getExitResources(),
    ]);
    accessNodes.value = controlPlane.accessNodes;
    entries.value = nextEntries;
    bindings.value = nextBindings;
    exitEndpoints.value = nextExitEndpoints;
    exitResources.value = nextExitResources;
    if (typeof route.query.entry === 'string' && nextEntries.some((entry) => entry.id === route.query.entry)) selectedEntryId.value = route.query.entry;
    if (!selectedEntryId.value && nextEntries[0]) {
      selectedEntryId.value = nextEntries[0].id;
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载入口失败');
  } finally {
    loading.value = false;
  }
}

function openCreateEntryDialog() {
  entryMode.value = 'create';
  const firstNode = accessNodes.value[0];
  Object.assign(entryForm, {
    id: '',
    accessNodeId: firstNode?.id ?? '',
    name: '',
    // 新建默认 IP 直连：监听地址默认取节点 IP 直连地址（ip_direct_address），省去管理员手填、不带出证书域名。
    listenHost: nodeListenHostForMode(firstNode, 'ip'),
    listenPort: firstNode?.publicPort || 443,
    protocol: 'vless',
    transport: 'tcp',
    transports: ['tcp'],
    carriage: 'tcp',
    security: 'reality',
    serverName: '',
    vlessQuantumEncryption: false,
    wsPath: '',
    wsHost: '',
    cdnEnabled: false,
    cdnProvider: '',
    cdnHostname: '',
    nodeDomainId: '',
    // 新建默认 IP 直连；Dialog 打开时按 availableConnectionModes 校正（无 ip 域名节点仍回 ip）。
    connectionMode: 'ip' as EntryAddressType,
    enabled: true,
    sortWeight: 100,
  });
  resetAddressDirtyFlags(addressDirty, entryForm, firstNode, 'create');
  // 新建按首个节点 + IP 直连智能填未占用端口（覆盖上面 publicPort 占位），避免一开就撞已占端口。
  applyDefaultPort();
  entryDialogOpen.value = true;
}

function openEditEntryDialog(entry: AccessEntrySummary) {
  entryMode.value = 'edit';
  Object.assign(entryForm, {
    id: entry.id,
    accessNodeId: entry.accessNodeId,
    name: entry.name,
    listenHost: entry.listenHost,
    listenPort: entry.listenPort || 443,
    protocol: entry.protocol || 'vless',
    transport: entry.transport || 'tcp',
    // 编辑态把已存单值网络模式反推回「传输 + 承载」；打开弹窗后 normalize 会再据协议/安全收敛。
    ...entryTransportsCarriageFromMode(entry.protocol || 'vless', entry.transport || 'tcp'),
    security: entry.security || 'reality',
    serverName: entry.serverName,
    // 编辑回填后端返回的量子加密标记（vless_decryption 非空派生）。
    vlessQuantumEncryption: entry.vlessQuantumEncryption,
    wsPath: entry.wsPath,
    wsHost: entry.wsHost,
    cdnEnabled: entry.cdnEnabled,
    cdnProvider: entry.cdnProvider,
    cdnHostname: entry.cdnHostname,
    // 编辑态带出入口已选的节点域名（后端读模型 nodeDomainId）；免证书入口为空。
    nodeDomainId: entry.nodeDomainId,
    // 占位，真正的连接方式由 Dialog 打开时按 cdnEnabled/选中域名 kind 反推（见 Dialog open 分支）。
    connectionMode: 'ip' as EntryAddressType,
    enabled: entry.enabled,
    sortWeight: entry.sortWeight || 100,
  });
  // 编辑态：与节点默认一致的地址仍随节点自动更新，自定义值视为已手改不覆盖。
  resetAddressDirtyFlags(addressDirty, entryForm, findAccessNode(entry.accessNodeId), 'edit');
  entryDialogOpen.value = true;
}

function handleAccessNodeChange(accessNodeId: string) {
  const node = findAccessNode(accessNodeId);
  if (!node) {
    return;
  }
  // 切节点后旧选中的域名不再属于新节点：清空，回退免证书，避免护栏判定串到旧节点。
  if (entryForm.nodeDomainId && !node.domains.some((domain) => domain.id === entryForm.nodeDomainId)) {
    entryForm.nodeDomainId = '';
  }
  // 未手改的地址自动带出新节点地址（按连接方式分流：IP 取节点 IP、域名取选中域名、CF 取 CF 域名），手改过的尊重不覆盖。
  applyNodeAddressDefaults(entryForm, node, addressDirty, entryForm.connectionMode, currentSelectedDomain(node));
  // 切节点后地址域名可能变化：未手改的 Server Name / 路径 / Host 随之重派生（如 TLS 的 SNI 跟证书域名走）。
  applyProtocolFieldDefaults(entryForm, node, addressDirty);
  applyDefaultPort();
}

// 选中节点域名变化：把监听地址带出该域名（管理员未手改时），并按域名 kind 走护栏。
function handleNodeDomainChange(nodeDomainId: string) {
  const node = findAccessNode(entryForm.accessNodeId);
  const domain = node?.domains.find((item) => item.id === nodeDomainId) ?? null;
  // 选了域名且监听地址未被手改：带出该域名作为监听/连接地址；清空选择则回退节点默认。
  if (domain && !addressDirty.listenHost) {
    entryForm.listenHost = domain.domain;
  } else if (!nodeDomainId && !addressDirty.listenHost) {
    applyNodeAddressDefaults(entryForm, node, addressDirty, entryForm.connectionMode, currentSelectedDomain(node));
  }
  // 换域名后未手改的 Server Name / 路径 / Host 随地址域名重派生（TLS SNI / WS Host 跟域名走）。
  applyProtocolFieldDefaults(entryForm, node, addressDirty);
}

// 管理员手动改地址后置脏标记，避免后续切换节点/协议时被默认值覆盖。
function markListenHostDirty() {
  addressDirty.listenHost = true;
}

function markCdnHostnameDirty() {
  addressDirty.cdnHostname = true;
}

// 管理员手改 Server Name / 路径 / Host 后置脏标记，后续归一/切节点不再自动覆盖。
function markServerNameDirty() {
  addressDirty.serverName = true;
}

function markWsPathDirty() {
  addressDirty.wsPath = true;
}

function markWsHostDirty() {
  addressDirty.wsHost = true;
}

// Dialog 归一协议/安全/网络后回调：据当前协议×安全×网络模式重派生未手改的 Server Name / 路径 / Host 默认值。
function handleFieldsNormalized() {
  applyProtocolFieldDefaults(entryForm, findAccessNode(entryForm.accessNodeId), addressDirty);
}

// 直连↔CF 切换（CDN 开关变化）时，把未手改的 CF 域名重新带出当前节点默认值。
function handleCdnModeChange() {
  const node = findAccessNode(entryForm.accessNodeId);
  applyNodeAddressDefaults(entryForm, node, addressDirty, entryForm.connectionMode, currentSelectedDomain(node));
  // 直连↔CF 切换后 Host 来源（CDN Hostname vs SNI/域名）改变：未手改的 Host 随之重派生。
  applyProtocolFieldDefaults(entryForm, node, addressDirty);
}

async function saveEntry() {
  if (!entryForm.accessNodeId) {
    ElMessage.warning('请选择接入节点');
    return;
  }
  if (!entryForm.name.trim()) {
    ElMessage.warning('请输入入口名称');
    return;
  }
  // 端口即时校验：手填端口被占用 / CF 选了非 CF 端口时先拦住，别把 400 丢给后端护栏。
  if (portError.value) {
    ElMessage.warning(portError.value);
    return;
  }
  saving.value = true;
  try {
    const modes = selectedEntryNetworkModes(entryForm);
    if (modes.length === 0) {
      ElMessage.warning('请选择至少一个网络模式');
      return;
    }
    // 编辑态只更新当前入口，禁止多模式隐式新建多条；多选时明确提示而不是静默创建端口递增的入口。
    if (entryMode.value === 'edit' && modes.length > 1) {
      ElMessage.warning('编辑入口只能保留一个网络模式，如需多模式请回到新建入口');
      return;
    }
    const basePort = Number(entryForm.listenPort || 443);
    if (basePort + modes.length - 1 > 65535) {
      ElMessage.warning('入口端口超出 65535，请减少网络模式或调低起始端口');
      return;
    }
    // CF（橙云）入口的监听端口必须落在 CF 支持的 HTTPS 端口集合内（与后端校验同口径），
    // 否则客户端到 CF 边缘会超时连不上。多网络模式会按 index 递增端口，逐个校验。
    if (isCfEntry()) {
      const cfPorts = modes.map((_, index) => basePort + index);
      if (cfPorts.some((port) => !isCfSupportedHttpsPort(port))) {
        ElMessage.warning(`橙云入口端口只能用 CF 支持的 HTTPS 端口（${CF_SUPPORTED_HTTPS_PORTS.join('/')}），且多网络模式不会逐个递增端口`);
        return;
      }
    }
    const payloads = modes.map((mode, index) => entryPayloadForMode(entryForm, mode, index, modes.length));
    let entryId = entryForm.id;
    if (entryMode.value === 'create') {
      for (const payload of payloads) {
        const createdId = await apiClient.createAccessEntry(payload);
        entryId ||= createdId;
      }
    }
    if (entryMode.value === 'edit') {
      await apiClient.updateAccessEntry(entryForm.id, payloads[0]);
    }
    selectedEntryId.value = entryId || entryForm.id;
    entryDialogOpen.value = false;
    await load();
    ElMessage.success(entryMode.value === 'create' ? `入口已创建 ${payloads.length} 条` : '入口已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存入口失败');
  } finally {
    saving.value = false;
  }
}

async function deleteEntry(entry: AccessEntrySummary) {
  try {
    await ElMessageBox.confirm(`确认删除入口「${entry.name || entry.id}」？`, '删除入口', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  saving.value = true;
  try {
    await apiClient.deleteAccessEntry(entry.id);
    if (selectedEntryId.value === entry.id) {
      selectedEntryId.value = '';
    }
    await load();
    ElMessage.success('入口已删除');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '删除入口失败');
  } finally {
    saving.value = false;
  }
}

function entryAddress(entry: AccessEntrySummary) {
  const host = entry.cdnEnabled && entry.cdnHostname ? entry.cdnHostname : entry.listenHost;
  return `${host || '-'}:${entry.listenPort || 443}`;
}
</script>

<template>
  <PageHeader title="入口管理" description="维护客户端入口，并把入口绑定到出口线路形成订阅节点。">
    <el-button :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" @click="openCreateEntryDialog">创建入口</el-button>
  </PageHeader>

  <el-row :gutter="16" class="entry-summary">
    <el-col :xs="24" :sm="8">
      <el-card shadow="never"><span>入口</span><strong>{{ entries.length }}</strong></el-card>
    </el-col>
    <el-col :xs="24" :sm="8">
      <el-card shadow="never"><span>绑定节点</span><strong>{{ bindings.length }}</strong></el-card>
    </el-col>
    <el-col :xs="24" :sm="8">
      <el-card shadow="never"><span>可选出口</span><strong>{{ exitEndpoints.length }}</strong></el-card>
    </el-col>
  </el-row>

  <el-card shadow="never" class="entry-card">
    <el-table v-loading="loading" :data="entriesByNode" row-key="id" stripe highlight-current-row @current-change="(row?: AccessEntrySummary) => { selectedEntryId = row?.id || selectedEntryId; }">
      <el-table-column label="入口" min-width="220">
        <template #default="{ row }: { row: AccessEntrySummary }">
          <div class="stacked">
            <strong>{{ row.name || '未命名入口' }}</strong>
            <small>{{ row.accessNodeName || row.accessNodeId }}</small>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="连接地址" min-width="190">
        <template #default="{ row }: { row: AccessEntrySummary }">{{ entryAddress(row) }}</template>
      </el-table-column>
      <el-table-column label="协议" width="150">
        <template #default="{ row }: { row: AccessEntrySummary }">
          {{ row.protocol }} / {{ row.transport }} / {{ row.security || 'none' }}
        </template>
      </el-table-column>
      <el-table-column label="CDN" min-width="170">
        <template #default="{ row }: { row: AccessEntrySummary }">
          <el-tag v-if="row.cdnEnabled" effect="plain">{{ row.cdnProvider || 'cdn' }} / {{ row.cdnHostname }}</el-tag>
          <span v-else class="muted-text">未启用</span>
        </template>
      </el-table-column>
      <el-table-column label="状态" width="90">
        <template #default="{ row }: { row: AccessEntrySummary }">
          <el-tag :type="row.enabled ? 'success' : 'info'" effect="plain">{{ row.enabled ? '启用' : '停用' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="220" fixed="right">
        <template #default="{ row }: { row: AccessEntrySummary }">
          <el-button text type="primary" @click="openEditEntryDialog(row)">编辑</el-button>
          <el-button text type="primary" @click="openBindExitDialog(row)">绑定线路</el-button>
          <el-button text type="danger" @click="deleteEntry(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>

  <el-card shadow="never" class="entry-card">
    <template #header>
      <div class="card-header">
        <strong>已绑定线路</strong>
        <span class="muted-text">{{ selectedEntry?.name || '请选择入口' }}</span>
      </div>
    </template>
    <el-table :data="selectedBindings" row-key="id" stripe>
      <el-table-column label="绑定名称" min-width="220">
        <template #default="{ row }: { row: AccessEntryExitBindingSummary }">
          {{ row.name || row.exitEndpointName || row.id }}
        </template>
      </el-table-column>
      <el-table-column label="出口线路" min-width="220">
        <template #default="{ row }: { row: AccessEntryExitBindingSummary }">
          {{ row.exitEndpointName || row.exitEndpointId }}
        </template>
      </el-table-column>
      <el-table-column label="状态" width="90">
        <template #default="{ row }: { row: AccessEntryExitBindingSummary }">
          <el-tag :type="row.enabled ? 'success' : 'info'" effect="plain">{{ row.enabled ? '启用' : '停用' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="90" fixed="right">
        <template #default="{ row }: { row: AccessEntryExitBindingSummary }">
          <el-button text type="danger" @click="deleteBinding(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>

  <AccessEntryDialog
    v-model="entryDialogOpen"
    :access-nodes="accessNodes"
    :entry-form="entryForm"
    :saving="saving"
    :title="entryDialogTitle"
    :port-error="portError"
    @access-node-change="handleAccessNodeChange"
    @connection-mode-change="applyDefaultPort"
    @listen-host-input="markListenHostDirty"
    @cdn-hostname-input="markCdnHostnameDirty"
    @cdn-mode-change="handleCdnModeChange"
    @node-domain-change="handleNodeDomainChange"
    @fields-normalized="handleFieldsNormalized"
    @server-name-input="markServerNameDirty"
    @transport-path-input="markWsPathDirty"
    @transport-host-input="markWsHostDirty"
    @save="saveEntry"
  />

  <el-dialog v-model="bindingDialogOpen" title="绑定线路" width="640px">
    <el-form label-position="top">
      <el-form-item label="出口线路">
        <el-select v-model="bindingForm.exitEndpointId" filterable class="entry-field">
          <el-option v-for="endpoint in exitEndpoints" :key="endpoint.id" :label="exitEndpointLabel(endpoint)" :value="endpoint.id" />
        </el-select>
      </el-form-item>
      <el-form-item label="绑定名称">
        <el-input v-model="bindingForm.name" placeholder="可选，不填使用出口线路名" />
      </el-form-item>
      <el-form-item label="备注">
        <el-input v-model="bindingForm.remark" />
      </el-form-item>
      <el-form-item label="启用状态">
        <el-switch v-model="bindingForm.enabled" active-text="启用" inactive-text="停用" />
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="bindingDialogOpen = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="saveBinding">保存</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.entry-summary,
.entry-card {
  margin-bottom: 16px;
}

.entry-summary span,
.stacked small,
.muted-text {
  color: #6b7280;
}

.entry-summary strong {
  display: block;
  margin-top: 8px;
  font-size: 28px;
}

.stacked {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.card-header {
  align-items: center;
  display: flex;
  justify-content: space-between;
}

.entry-field {
  width: 100%;
}
</style>
