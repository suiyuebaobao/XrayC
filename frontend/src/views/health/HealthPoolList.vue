<!--
  本文件展示出口管理内部健康概览。
  组件只显示线路名称、地区和可用成员数量。
  不回显线路地址、账号、密码或任何敏感链路信息。
-->
<script setup lang="ts">
import type { ExitPool, HealthStatus } from '@/services/api';
import { healthLabel, healthType } from '@/views/health/format';

defineProps<{ pools: ExitPool[] }>();

function poolStatus(status: ExitPool['status']): HealthStatus {
  return status === 'healthy' || status === 'degraded' || status === 'offline' ? status : 'unknown';
}
</script>

<template>
  <el-card shadow="never" class="health-section">
    <template #header>出口管理可用性</template>
    <el-empty v-if="pools.length === 0" description="暂无线路" />
    <div v-else class="pool-list">
      <div v-for="pool in pools" :key="pool.uuid" class="pool-row">
        <div>
          <strong>{{ pool.name || pool.uuid }}</strong>
          <span>{{ pool.region || '未设置地区' }}</span>
        </div>
        <el-tag :type="healthType(poolStatus(pool.status))" effect="plain">
          {{ healthLabel(poolStatus(pool.status)) }}
        </el-tag>
        <span>{{ pool.healthyMembers }} / {{ pool.totalMembers }} 可用</span>
      </div>
    </div>
  </el-card>
</template>

<style scoped>
.health-section {
  margin-bottom: 18px;
  border-radius: 22px;
}

.pool-list {
  display: grid;
  gap: 10px;
}

.pool-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background:
    radial-gradient(circle at top right, rgba(33, 150, 243, 0.12), transparent 36%),
    rgba(255, 255, 255, 0.62);
}

.pool-row > div {
  display: grid;
  gap: 4px;
}

.pool-row span {
  color: var(--ink-soft);
}

@media (max-width: 760px) {
  .pool-row {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
