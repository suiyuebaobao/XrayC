<!--
  本子组件展示某节点已建的本机出口（self_hosted）线路并支持就地编辑/删除。
  数据来自 GET .../local-exit-lines；编辑走 PUT、删除走 DELETE（Phase 4 端点）。
  编辑只改端口/选用域名/启用，不重建协议字段；删除前二次确认。
  改动成功后向父组件 emit changed，让页面统一刷新控制面与出口管理。
-->
<script setup lang="ts">
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, reactive, ref, watch } from 'vue';
import { apiClient } from '@/services/api';
import type { LocalExitLineSummary } from '@/services/api';
import type { RelayNodeView } from '@/views/access-lines/types';

const props = defineProps<{
  node: RelayNodeView | null;
  // 父弹窗可见时才加载，避免后台无谓请求。
  active: boolean;
}>();

const emit = defineEmits<{
  (event: 'changed'): void;
}>();

const loading = ref(false);
const saving = ref(false);
const lines = ref<LocalExitLineSummary[]>([]);
const editDialogOpen = ref(false);
const editingEndpointId = ref('');

// 就地编辑表单：只改端口、选用域名、启用状态。
const editForm = reactive({
  endpointName: '',
  port: 1443,
  nodeDomainId: '',
  enabled: true,
});

// 节点的可选域名清单（编辑时下拉）；留空=免证书。
const nodeDomainOptions = ref<RelayNodeView['domains']>([]);

// 本机出口的「实际出网 IP」= 所属中转节点的公网 IP（优先 ip_direct_address，无则 public_host）。
// 本机出口的 host（127.0.0.1 回环）只是节点上那个回环 SOCKS5/出口服务的内部连接地址，
// 真正出网走的是节点公网 IP；这里把出网 IP 单独算出来供「出口 IP」展示，避免管理员误以为出口在 127.0.0.1。
const nodeExitIp = computed(() => {
  const direct = props.node?.ipDirectAddress?.trim();
  if (direct) {
    return direct;
  }
  return props.node?.publicHost?.trim() ?? '';
});

// 某条线路的 host 是否为本机回环地址（127.0.0.1 / ::1 / localhost）。
// 是回环说明它只是节点内部的本机出口服务地址，需要明确标注为「内部」而非节点对外地址。
function isLoopbackHost(host: string) {
  const normalized = host.trim().toLowerCase();
  return normalized === '127.0.0.1' || normalized === '::1' || normalized === 'localhost';
}

// 出口 IP 展示文案：有节点公网 IP 时显示「<公网IP>（经节点公网出网）」，否则降级提示未配置。
function lineExitIpText() {
  return nodeExitIp.value
    ? `${nodeExitIp.value}（经节点公网出网）`
    : '节点公网 IP 未配置';
}

// 本机回环服务地址展示文案：仅当 host 为回环时显示，并明确标注是「内部连接地址」。
function loopbackServiceText(line: LocalExitLineSummary) {
  if (!isLoopbackHost(line.host)) {
    return '';
  }
  return `${line.host}:${line.port}（本机回环服务地址 · 内部）`;
}

watch(
  () => [props.active, props.node?.id ?? ''] as const,
  ([active]) => {
    nodeDomainOptions.value = props.node?.domains ?? [];
    if (active && props.node) {
      void loadLines();
    } else {
      lines.value = [];
    }
  },
  { immediate: true },
);

async function loadLines() {
  const node = props.node;
  if (!node) {
    return;
  }
  loading.value = true;
  try {
    lines.value = await apiClient.listLocalExitLines(node.id);
  } catch (error) {
    // 端点尚未合入（Phase 4）时静默清空，不打断本机出口创建表单。
    lines.value = [];
    ElMessage.warning(error instanceof Error ? error.message : '加载已建本机出口失败');
  } finally {
    loading.value = false;
  }
}

function nodeDomainLabel(domain: { domain: string; kind: 'direct' | 'cf'; isPrimary: boolean }) {
  const kindLabel = domain.kind === 'cf' ? 'CF' : '直连';
  return `${domain.domain}（${kindLabel}${domain.isPrimary ? ' · 主' : ''}）`;
}

function lineDomainText(line: LocalExitLineSummary) {
  if (line.nodeDomain?.domain) {
    return nodeDomainLabel({
      domain: line.nodeDomain.domain,
      kind: line.nodeDomain.kind,
      isPrimary: false,
    });
  }
  return '免证书';
}

function openEdit(line: LocalExitLineSummary) {
  editingEndpointId.value = line.exitEndpointId;
  editForm.endpointName = line.endpointName;
  editForm.port = line.port || 1443;
  editForm.nodeDomainId = line.nodeDomainId;
  editForm.enabled = line.enabled;
  editDialogOpen.value = true;
}

async function submitEdit() {
  const node = props.node;
  if (!node) {
    return;
  }
  const port = Number(editForm.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    ElMessage.warning('端口必须在 1-65535 之间');
    return;
  }
  saving.value = true;
  try {
    await apiClient.updateLocalExitLine(node.id, editingEndpointId.value, {
      endpoint_name: editForm.endpointName.trim() || undefined,
      port,
      node_domain_id: editForm.nodeDomainId.trim() || null,
      enabled: editForm.enabled,
    });
    editDialogOpen.value = false;
    await loadLines();
    emit('changed');
    ElMessage.success('本机出口线路已更新');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '更新本机出口线路失败');
  } finally {
    saving.value = false;
  }
}

async function removeLine(line: LocalExitLineSummary) {
  const node = props.node;
  if (!node) {
    return;
  }
  try {
    await ElMessageBox.confirm(
      `确认删除本机出口线路「${line.endpointName || line.resourceName || line.exitEndpointId}」？`,
      '删除本机出口线路',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    );
  } catch {
    return;
  }
  saving.value = true;
  try {
    await apiClient.deleteLocalExitLine(node.id, line.exitEndpointId);
    await loadLines();
    emit('changed');
    ElMessage.success('本机出口线路已删除');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '删除本机出口线路失败');
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="local-exit-list">
    <div class="local-exit-list__head">
      <strong>已建本机出口线路</strong>
      <el-button text type="primary" :loading="loading" @click="loadLines">刷新</el-button>
    </div>
    <p class="local-exit-list__note">
      本机出口实际从所属节点的公网 IP 出网；表中「本机服务地址（内部）」只是节点上本机出口服务的回环/内部连接地址，不是对外出口 IP。
    </p>
    <el-table
      v-loading="loading"
      :data="lines"
      empty-text="该节点暂无已建本机出口线路。"
      stripe
      size="small"
    >
      <el-table-column label="线路" min-width="180">
        <template #default="{ row }: { row: LocalExitLineSummary }">
          {{ row.endpointName || row.resourceName || row.exitEndpointId }}
        </template>
      </el-table-column>
      <el-table-column prop="outboundType" label="协议" min-width="90" />
      <el-table-column prop="networkMode" label="网络模式" min-width="100" />
      <el-table-column label="出口 IP（实际出网）" min-width="220">
        <template #default="{ row }: { row: LocalExitLineSummary }">
          <div class="exit-addr">
            <span class="exit-addr__ip">{{ lineExitIpText() }}</span>
            <span v-if="loopbackServiceText(row)" class="exit-addr__loopback">
              {{ loopbackServiceText(row) }}
            </span>
            <span v-else class="exit-addr__loopback">
              {{ row.host }}:{{ row.port }}（本机服务地址 · 内部）
            </span>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="选用域名" min-width="160">
        <template #default="{ row }: { row: LocalExitLineSummary }">{{ lineDomainText(row) }}</template>
      </el-table-column>
      <el-table-column label="状态" min-width="90">
        <template #default="{ row }: { row: LocalExitLineSummary }">
          <el-tag :type="row.enabled ? 'success' : 'info'" effect="plain">{{ row.enabled ? '启用' : '停用' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" min-width="120" fixed="right">
        <template #default="{ row }: { row: LocalExitLineSummary }">
          <el-button text type="primary" @click="openEdit(row)">编辑</el-button>
          <el-button text type="danger" :loading="saving" @click="removeLine(row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-dialog v-model="editDialogOpen" title="编辑本机出口线路" width="560px" append-to-body>
      <el-form label-position="top">
        <el-form-item label="协议档名称">
          <el-input v-model="editForm.endpointName" placeholder="为空保留原名" />
        </el-form-item>
        <el-form-item label="端口">
          <el-input-number v-model="editForm.port" :min="1" :max="65535" class="edit-field" />
        </el-form-item>
        <el-form-item label="选用域名">
          <el-select
            v-model="editForm.nodeDomainId"
            clearable
            class="edit-field"
            placeholder="留空=免证书"
          >
            <el-option
              v-for="domain in nodeDomainOptions"
              :key="domain.id"
              :label="nodeDomainLabel(domain)"
              :value="domain.id"
            />
          </el-select>
        </el-form-item>
        <el-form-item label="状态">
          <el-switch v-model="editForm.enabled" active-text="启用" inactive-text="停用" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="editDialogOpen = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="submitEdit">保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.local-exit-list {
  margin-bottom: 16px;
}

.local-exit-list__head {
  align-items: center;
  display: flex;
  justify-content: space-between;
  margin-bottom: 8px;
}

.local-exit-list__note {
  margin: 0 0 8px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--el-text-color-secondary);
}

.exit-addr {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.exit-addr__ip {
  color: var(--el-text-color-primary);
}

.exit-addr__loopback {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.edit-field {
  width: 100%;
}
</style>
