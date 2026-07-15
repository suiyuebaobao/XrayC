<!--
  本组件只渲染管理员用户级流量日志表格。
  它接收父级已经按用户加载好的日志数据。
  表格展示访问 IP、入口、出口端点和流量扣费。
  组件不发起 API 请求，避免后台页面和抽屉重复请求逻辑。
-->
<script setup lang="ts">
import type { AdminUserTrafficLog } from '@/services/api';

defineProps<{
  logs: AdminUserTrafficLog[];
  loading: boolean;
  emptyText?: string;
}>();

function lineLabel(row: AdminUserTrafficLog) {
  const line = row.accessLine;
  const address = line.listenHost && line.listenPort ? `${line.listenHost}:${line.listenPort}` : '-';
  return `${line.name || line.id || '-'} / ${line.protocol || '-'} / ${line.transport || '-'} / ${address}`;
}

function nodeLabel(row: AdminUserTrafficLog) {
  const node = row.accessNode;
  return `${node.name || node.id || '-'} / ${node.publicHost || '-'}`;
}

function exitLabel(row: AdminUserTrafficLog) {
  const endpoint = row.exitEndpoint;
  const address = endpoint.host && endpoint.port ? `${endpoint.host}:${endpoint.port}` : '-';
  return `${endpoint.name || endpoint.id || '-'} / ${endpoint.outboundType || '-'} / ${address}`;
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return '0 B';
  }
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 || unitIndex === 0 ? 0 : 2)} ${units[unitIndex]}`;
}

function formatDate(value: string) {
  return value || '-';
}
</script>

<template>
  <el-table v-loading="loading" :data="logs" row-key="id" stripe class="traffic-log-table">
    <el-table-column label="记录时间" min-width="180">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        <strong>{{ formatDate(row.recordedAt || row.collectedAt) }}</strong>
        <p class="muted">采样 {{ formatDate(row.collectedAt) }}</p>
      </template>
    </el-table-column>
    <el-table-column label="访问 IP" min-width="180">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        <strong>{{ row.clientIp || '-' }}</strong>
        <p class="muted">{{ row.clientIpHash || '无历史 IP 事件' }}</p>
      </template>
    </el-table-column>
    <el-table-column label="入口" min-width="320">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        {{ lineLabel(row) }}
        <p class="muted">{{ nodeLabel(row) }}</p>
      </template>
    </el-table-column>
    <el-table-column label="出口端点" min-width="280">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        {{ exitLabel(row) }}
        <p class="muted">{{ row.exitEndpoint.resourceName || '-' }}</p>
      </template>
    </el-table-column>
    <el-table-column label="真实流量" min-width="170">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        ↑ {{ formatBytes(row.deltaUplink) }}
        <p class="muted">↓ {{ formatBytes(row.deltaDownlink) }} / 合计 {{ formatBytes(row.deltaTotal) }}</p>
      </template>
    </el-table-column>
    <el-table-column label="扣费流量" min-width="170">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        {{ formatBytes(row.billedBytes) }}
        <p class="muted">倍率 x{{ row.billingMultiplier || 1 }}</p>
      </template>
    </el-table-column>
    <el-table-column label="用户标识" min-width="260">
      <template #default="{ row }: { row: AdminUserTrafficLog }">
        {{ row.xrayUserKey || '-' }}
        <p class="muted">{{ row.trafficSource || '-' }} / {{ row.sessionStatus || '-' }}</p>
      </template>
    </el-table-column>
    <template #empty>
      <el-empty :description="emptyText || '暂无该用户的流量日志'" />
    </template>
  </el-table>
</template>

<style scoped>
.traffic-log-table {
  width: 100%;
}
</style>
