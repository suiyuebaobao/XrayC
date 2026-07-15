<!--
  本文件展示单个中转节点卡片、运行态统计和入口表格。
  它只接收聚合后的 RelayNodeView，并通过事件通知页面动作。
  节点状态、数字和延迟格式化来自同目录 format.ts。
  拆出后主页面不再包含大段节点表格模板。
-->
<script setup lang="ts">
import { computed, ref } from 'vue';
import type { AccessLine } from '@/services/api';
import {
  latencyValue,
  nodeHealthLabel,
  nodeHealthTitle,
  nodeHealthType,
  nodeStatusLabel,
  nodeStatusType,
  nodeTlsRemainingLabel,
  nodeTlsStatusLabel,
  nodeTlsStatusType,
  nodeTlsSummary,
  reportedNumber,
  statusType,
} from '@/views/access-lines/format';
import type { RelayNodeView } from '@/views/access-lines/types';

const props = defineProps<{
  node: RelayNodeView;
  loading: boolean;
  selected: boolean;
  renewingTls: boolean;
  rebooting: boolean;
}>();

// 内核待升级：connmark 不可用或后端显式标记待升级，提示需重启该节点应用新内核。
const kernelUpgradePending = computed(
  () => props.node.kernelConnmarkAvailable === false || props.node.kernelUpgradePending === true,
);

// 重启请求进行中：本地提交 loading 或后端读模型已记 queued/running，按钮禁用并显示「重启中」。
const rebootInProgress = computed(
  () => props.rebooting
    || props.node.rebootStatus === 'queued'
    || props.node.rebootStatus === 'running',
);

// 按 kind 拆出直连/CF 域名清单，用于卡片计数与展开清单。
const directDomains = computed(() => props.node.domains.filter((domain) => domain.kind === 'direct'));
const cfDomains = computed(() => props.node.domains.filter((domain) => domain.kind === 'cf'));
const ipDirectAddress = computed(() => props.node.ipDirectAddress?.trim() ?? '');

// 对外直连地址概览：直连域名 N 个 / CF 域名 N 个 / 直连 IP。
const directAddressSummary = computed(() => {
  const parts = [
    `直连域名 ${directDomains.value.length} 个`,
    `CF 域名 ${cfDomains.value.length} 个`,
    ipDirectAddress.value ? '直连 IP 1 个' : '直连 IP 0 个',
  ];
  return parts.join(' / ');
});

// 是否有任何对外直连地址可展开查看清单。
const hasAnyDirectAddress = computed(
  () => props.node.domains.length > 0 || Boolean(ipDirectAddress.value),
);

// 默认展开域名清单：直连域名 + CF 域名 + 各自证书状态/续约入口都直接可见，不用再点「查看清单」。
const domainsExpanded = ref(true);

function nodeDomainLabel(domain: { domain: string; kind: 'direct' | 'cf'; isPrimary: boolean }) {
  const kindLabel = domain.kind === 'cf' ? 'CF' : '直连';
  return `${domain.domain}（${kindLabel}${domain.isPrimary ? ' · 主' : ''}）`;
}

const emit = defineEmits<{
  (event: 'selection-change', node: RelayNodeView, checked: boolean): void;
  (event: 'edit', node: RelayNodeView): void;
  (event: 'delete', node: RelayNodeView): void;
  (event: 'local-exit', node: RelayNodeView): void;
  (event: 'renew-tls', node: RelayNodeView): void;
  (event: 'reboot', node: RelayNodeView): void;
}>();

// SSL 续约是否禁用：无证书、或已有续约任务排队/执行中（与卡片「续期 SSL」按钮同口径）。
const tlsRenewDisabled = computed(
  () => props.node.tlsCertificates.length === 0
    || props.node.tlsRenewStatus === 'queued'
    || props.node.tlsRenewStatus === 'running',
);

// 点域名行后的「续约 SSL 证书」标签：下发节点级 SSL 续约（续该节点全部证书，含本域名）；禁用/续约中不触发。
function renewSsl() {
  if (tlsRenewDisabled.value || props.renewingTls) {
    return;
  }
  emit('renew-tls', props.node);
}

function lineDisplayName(line: AccessLine) {
  return line.exitEndpointName || line.name || line.exitEndpointId || '未返回线路';
}
</script>

<template>
  <el-card v-loading="loading" shadow="never" class="relay-card">
    <template #header>
      <div class="relay-card__header">
        <div class="relay-card__title">
          <el-checkbox
            data-testid="relay-node-select"
            aria-label="选择中转节点"
            :model-value="selected"
            @change="emit('selection-change', node, Boolean($event))"
          />
          <div>
            <strong>{{ node.name }}</strong>
            <p>{{ node.publicHost ? `${node.publicHost}:${node.publicPort || 443}` : '未设置用户访问地址' }}</p>
            <p v-if="node.remark" class="relay-card__remark">{{ node.remark }}</p>
          </div>
        </div>
        <div class="relay-card__actions">
          <el-tag :type="nodeStatusType(node)" effect="plain">{{ nodeStatusLabel(node) }}</el-tag>
          <el-tag :type="nodeHealthType(node)" effect="plain" :title="nodeHealthTitle(node)">
            {{ nodeHealthLabel(node) }}
          </el-tag>
          <el-tag :type="nodeTlsStatusType(node)" effect="plain">{{ nodeTlsStatusLabel(node) }}</el-tag>
          <el-tag v-if="node.tlsRenewMessage" type="warning" effect="plain">{{ node.tlsRenewMessage }}</el-tag>
          <el-tag v-if="kernelUpgradePending" type="warning" effect="plain">内核待升级·需重启</el-tag>
          <el-button
            text
            type="primary"
            :loading="renewingTls"
            :disabled="node.tlsCertificates.length === 0 || node.tlsRenewStatus === 'queued' || node.tlsRenewStatus === 'running'"
            @click="emit('renew-tls', node)"
          >
            续期 SSL
          </el-button>
          <el-button
            text
            type="danger"
            :loading="rebooting"
            :disabled="rebootInProgress"
            @click="emit('reboot', node)"
          >
            {{ rebootInProgress ? '重启中' : '重启节点' }}
          </el-button>
          <el-button text type="primary" @click="emit('edit', node)">编辑</el-button>
          <el-button text type="primary" @click="emit('local-exit', node)">本机出口服务</el-button>
          <el-button text type="danger" @click="emit('delete', node)">删除</el-button>
        </div>
      </div>
    </template>

    <el-descriptions :column="4" border class="relay-card__stats">
      <el-descriptions-item label="运行入口数">{{ node.lines.length }}</el-descriptions-item>
      <el-descriptions-item label="运行入口">{{ node.boundLineNames.length ? node.boundLineNames.join('、') : '未创建' }}</el-descriptions-item>
      <el-descriptions-item label="SSL">{{ nodeTlsSummary(node) }} / {{ nodeTlsRemainingLabel(node) }}</el-descriptions-item>
      <el-descriptions-item label="对外直连地址" :span="3">
        <span>{{ directAddressSummary }}</span>
        <el-button
          v-if="hasAnyDirectAddress"
          text
          type="primary"
          size="small"
          class="relay-card__expand"
          @click="domainsExpanded = !domainsExpanded"
        >
          {{ domainsExpanded ? '收起' : '查看清单' }}
        </el-button>
      </el-descriptions-item>
      <el-descriptions-item label="在线用户">{{ reportedNumber(node.onlineUsers) }}</el-descriptions-item>
      <el-descriptions-item label="活跃连接">{{ reportedNumber(node.activeConnections) }}</el-descriptions-item>
    </el-descriptions>

    <div v-if="domainsExpanded && hasAnyDirectAddress" class="relay-card__domains">
      <div v-if="ipDirectAddress" class="relay-card__domain-row">
        <el-tag size="small" type="info" effect="plain">IP 直连</el-tag>
        <span>{{ ipDirectAddress }}</span>
      </div>
      <div v-for="domain in node.domains" :key="domain.id" class="relay-card__domain-row">
        <el-tag size="small" :type="domain.kind === 'cf' ? 'warning' : 'success'" effect="plain">
          {{ domain.kind === 'cf' ? 'CF 域名' : '直连域名' }}
        </el-tag>
        <span>{{ nodeDomainLabel(domain) }}</span>
        <el-tag v-if="domain.certStatus" size="small" effect="plain">证书：{{ domain.certStatus }}</el-tag>
        <el-tag
          size="small"
          type="primary"
          effect="dark"
          class="relay-card__renew-tag"
          :class="{ 'relay-card__renew-tag--disabled': tlsRenewDisabled || renewingTls }"
          :title="domain.kind === 'cf' ? 'CF 域名经 HTTP-01 自动续约（免 token）' : '直连域名 HTTP-01 自动续约'"
          @click="renewSsl"
        >
          {{ renewingTls ? '续约中…' : '续约 SSL 证书' }}
        </el-tag>
      </div>
    </div>

    <el-table :data="node.lines" empty-text="该中转节点还没有运行入口，请到入口管理创建。" stripe>
      <el-table-column label="出口线路" min-width="180">
        <template #default="{ row }: { row: AccessLine }">{{ lineDisplayName(row) }}</template>
      </el-table-column>
      <el-table-column prop="inboundProtocol" label="入口协议" min-width="110" />
      <el-table-column prop="networkMode" label="网络模式" min-width="100" />
      <el-table-column label="UDP" min-width="90">
        <template #default="{ row }: { row: AccessLine }">
          <el-tag :type="row.udpEnabled ? 'success' : 'info'" effect="plain">
            {{ row.udpEnabled ? '开' : '关' }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="客户端连接地址" min-width="190">
        <template #default="{ row }: { row: AccessLine }">
          {{ row.listenHost }}:{{ row.listenPort }}
        </template>
      </el-table-column>
      <el-table-column label="状态" min-width="100">
        <template #default="{ row }: { row: AccessLine }">
          <el-tag :type="statusType(row.status)" effect="plain">{{ row.status }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="探测延迟" min-width="110">
        <template #default="{ row }: { row: AccessLine }">{{ latencyValue(row.latencyMs ?? row.probeLatencyMs) }}</template>
      </el-table-column>
    </el-table>
  </el-card>
</template>

<style scoped>
.relay-card {
  margin-bottom: 16px;
}

.relay-card__header {
  align-items: center;
  display: flex;
  justify-content: space-between;
  gap: 16px;
}

.relay-card__title {
  align-items: center;
  display: flex;
  gap: 10px;
}

.relay-card__header p {
  color: #667085;
  margin: 6px 0 0;
}

.relay-card__remark {
  color: #475467;
}

.relay-card__actions {
  align-items: center;
  display: flex;
  gap: 8px;
}

.relay-card__stats {
  margin-bottom: 14px;
}

.relay-card__expand {
  margin-left: 8px;
}

.relay-card__domains {
  background: #f9fafb;
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 14px;
  padding: 12px;
}

.relay-card__domain-row {
  align-items: center;
  display: flex;
  gap: 8px;
}

/* 域名行后的「续约 SSL 证书」标签：可点击下发节点级续约；禁用/续约中置灰不可点。 */
.relay-card__renew-tag {
  cursor: pointer;
  margin-left: auto;
  user-select: none;
}

.relay-card__renew-tag--disabled {
  cursor: not-allowed;
  opacity: 0.5;
}
</style>
