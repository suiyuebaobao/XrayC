<!--
  本页面用于后台查看审计日志。
  它展示操作者、动作、资源、结果和请求摘要。
  页面只负责展示 API 标准化后的审计日志，不额外改写请求摘要。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type AdminAuditLog } from '@/services/api';

const loading = ref(true);
const rows = ref<AdminAuditLog[]>([]);
const errorMessage = ref('');

onMounted(load);

async function load() {
  loading.value = true;
  errorMessage.value = '';

  try {
    rows.value = await apiClient.getAdminAuditLogs();
  } catch {
    rows.value = [];
    errorMessage.value = '无法加载审计日志，请确认后端已提供 /api/admin/audit-logs 只读接口。';
  } finally {
    loading.value = false;
  }
}

function formatTime(value: string) {
  if (!value) {
    return '-';
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return date.toLocaleString('zh-CN', { hour12: false });
}

function resultType(value: string): 'success' | 'danger' | 'warning' | 'info' {
  const result = value.toLowerCase();
  if (result === 'succeeded' || result === 'success' || result === 'ok') {
    return 'success';
  }
  if (result === 'failed' || result === 'failure' || result === 'error') {
    return 'danger';
  }
  if (result === 'unknown') {
    return 'info';
  }
  return 'warning';
}

function resultLabel(value: string) {
  const result = value.toLowerCase();
  if (result === 'succeeded' || result === 'success' || result === 'ok') {
    return '成功';
  }
  if (result === 'failed' || result === 'failure' || result === 'error') {
    return '失败';
  }
  if (result === 'unknown') {
    return '未知';
  }
  return value || '未知';
}
</script>

<template>
  <PageHeader title="审计日志" description="只读查看管理员操作记录，展示请求摘要。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
  </PageHeader>

  <el-alert
    v-if="errorMessage"
    class="audit-alert"
    title="审计日志接口不可用"
    :description="errorMessage"
    type="warning"
    show-icon
    :closable="false"
  />

  <el-card shadow="never" class="audit-card">
    <template #header>
      <div class="audit-card__header">
        <span>管理员审计日志</span>
        <el-tag type="info" effect="plain">只读</el-tag>
      </div>
    </template>

    <el-table
      v-loading="loading"
      :data="rows"
      stripe
      empty-text="暂无审计日志"
      class="audit-table"
    >
      <el-table-column label="时间" min-width="180">
        <template #default="{ row }">
          {{ formatTime(row.createdAt) }}
        </template>
      </el-table-column>
      <el-table-column prop="actor" label="操作者" min-width="140" />
      <el-table-column prop="action" label="动作" min-width="180" />
      <el-table-column prop="resource" label="资源" min-width="180" />
      <el-table-column label="结果" min-width="100">
        <template #default="{ row }">
          <el-tag :type="resultType(row.result)" effect="plain">
            {{ resultLabel(row.result) }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="summary" label="请求摘要" min-width="280" show-overflow-tooltip />
    </el-table>
  </el-card>
</template>

<style scoped>
.audit-alert {
  margin-bottom: 18px;
}

.audit-card {
  border-radius: 22px;
}

.audit-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.audit-table {
  width: 100%;
}
</style>
