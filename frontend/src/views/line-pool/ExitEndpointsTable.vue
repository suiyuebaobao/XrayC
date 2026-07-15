<!--
  本文件展示出口管理里的线路。
  管理员在这里查看、搜索、编辑、删除和触发探测。
  表格允许展示真实地址、账号、密码和 JSON 配置。
  这些字段只属于管理员后台，不能复用到用户订阅或公开页面。
  删除动作会同步清理分组成员和绑定到该线路的中转入口。
-->
<script setup lang="ts">
import { computed, ref } from 'vue';
import type { ExitEndpointSummary, ExitResourceSummary } from '@/services/api';
import { formatJson, lineAccount, lineSecret, protocolLabel } from './format';

const props = defineProps<{
  endpoints: ExitEndpointSummary[];
  resources: ExitResourceSummary[];
  loading: boolean;
}>();

const emit = defineEmits<{
  (event: 'edit', endpoint: ExitEndpointSummary): void;
  (event: 'delete', endpoint: ExitEndpointSummary): void;
  (event: 'probe', endpoint: ExitEndpointSummary): void;
}>();

const keyword = ref('');

const resourceById = computed(() => {
  const items = new Map<string, ExitResourceSummary>();
  for (const resource of props.resources) {
    items.set(resource.id, resource);
  }
  return items;
});

const filteredEndpoints = computed(() => {
  const query = keyword.value.trim().toLowerCase();
  if (!query) {
    return props.endpoints;
  }
  return props.endpoints.filter((endpoint) => {
    const resource = resourceById.value.get(endpoint.exitResourceId);
    return [
      endpoint.resourceName,
      endpoint.name,
      endpoint.host,
      endpoint.outboundType,
      resource?.providerName,
      resource?.ownership,
      resource?.accessNodeName,
      resource?.region,
    ]
      .filter(Boolean)
      .some((value) => String(value).toLowerCase().includes(query));
  });
});

function resourceText(endpoint: ExitEndpointSummary) {
  const resource = resourceById.value.get(endpoint.exitResourceId);
  const provider = resource?.providerName ? ` / ${resource.providerName}` : '';
  return `${endpoint.resourceName || '-'}${provider}`;
}

function ownershipText(endpoint: ExitEndpointSummary) {
  const resource = resourceById.value.get(endpoint.exitResourceId);
  const ownership = resource?.ownership || '';
  if (ownership === 'self_hosted' && resource?.accessNodeName) {
    return `自己创建 / ${resource.accessNodeName}`;
  }
  if (ownership === 'self_hosted') {
    return '自己创建';
  }
  if (ownership === 'third_party') {
    return '外部';
  }
  if (ownership === 'local_direct') {
    return '历史';
  }
  return ownership || '-';
}

function endpointText(endpoint: ExitEndpointSummary) {
  const host = endpoint.host || '-';
  return endpoint.port > 0 ? `${host}:${endpoint.port}` : host;
}

function networkModeText(endpoint: ExitEndpointSummary) {
  const config = endpoint.streamConfig || {};
  const raw = config.network_mode ?? config.network ?? '';
  const mode = String(raw || '').trim();
  return mode || '-';
}

function enabledType(endpoint: ExitEndpointSummary) {
  return endpoint.enabled && endpoint.exitResourceEnabled ? 'success' : 'info';
}
</script>

<template>
  <el-card shadow="never" class="line-endpoints-card">
    <template #header>
      <div class="line-endpoints-header">
        <div>
          <strong>出口管理（统一列表）</strong>
          <p>所有来源的线路都在这里维护。</p>
        </div>
        <div class="line-endpoints-actions">
          <el-input v-model="keyword" clearable placeholder="搜索名称、地址、协议、供应商" />
          <el-tag effect="plain">{{ filteredEndpoints.length }} / {{ endpoints.length }} 条</el-tag>
        </div>
      </div>
    </template>

    <el-table v-loading="loading" :data="filteredEndpoints" stripe>
      <el-table-column label="线路/供应商" min-width="220">
        <template #default="{ row }: { row: ExitEndpointSummary }">
          <strong>{{ resourceText(row) }}</strong>
          <div class="muted">{{ row.name || '-' }}</div>
          <div class="row-crud-actions">
            <el-button link type="primary" @click="emit('edit', row)">查看/编辑</el-button>
            <el-button link type="primary" @click="emit('probe', row)">探测</el-button>
            <el-button link type="danger" @click="emit('delete', row)">删除</el-button>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="协议" min-width="130">
        <template #default="{ row }: { row: ExitEndpointSummary }">
          <el-tag effect="plain">{{ protocolLabel(row.outboundType) }}</el-tag>
          <div class="muted">网络：{{ networkModeText(row) }}</div>
        </template>
      </el-table-column>
      <el-table-column label="地址" min-width="220">
        <template #default="{ row }: { row: ExitEndpointSummary }">{{ endpointText(row) }}</template>
      </el-table-column>
      <el-table-column label="账号/UUID" min-width="190">
        <template #default="{ row }: { row: ExitEndpointSummary }"><pre>{{ lineAccount(row) }}</pre></template>
      </el-table-column>
      <el-table-column label="密码/密钥" min-width="220">
        <template #default="{ row }: { row: ExitEndpointSummary }"><pre>{{ lineSecret(row) }}</pre></template>
      </el-table-column>
      <el-table-column label="来源/所属服务器" min-width="180">
        <template #default="{ row }: { row: ExitEndpointSummary }">{{ ownershipText(row) }}</template>
      </el-table-column>
      <el-table-column label="状态" min-width="120">
        <template #default="{ row }: { row: ExitEndpointSummary }">
          <el-tag :type="enabledType(row)" effect="plain">{{ row.enabled && row.exitResourceEnabled ? '启用' : '停用' }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="配置 JSON" min-width="260">
        <template #default="{ row }: { row: ExitEndpointSummary }">
          <pre class="config-json">{{ formatJson(row.outboundConfig) }}</pre>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="210" fixed="right">
        <template #default="{ row }: { row: ExitEndpointSummary }">
          <el-button size="small" @click="emit('edit', row)">查看/编辑</el-button>
          <el-button size="small" @click="emit('probe', row)">探测</el-button>
          <el-button size="small" type="danger" @click="emit('delete', row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>
</template>

<style scoped>
.line-endpoints-card {
  margin-bottom: 16px;
}

.line-endpoints-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.line-endpoints-header p,
.muted,
.row-crud-actions {
  margin: 4px 0 0;
  color: #6b7280;
}

.row-crud-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.line-endpoints-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 360px;
}

pre {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
}

.config-json {
  max-height: 120px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.5;
}

@media (max-width: 720px) {
  .line-endpoints-header,
  .line-endpoints-actions {
    align-items: stretch;
    flex-direction: column;
    min-width: 0;
  }
}
</style>
