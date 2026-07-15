<!--
  本文件展示健康检查页面顶部汇总小卡片。
  组件只接收 useHealthCheck 计算好的 HealthCard 列表。
  它负责视觉布局，不负责接口请求或健康规则判断。
-->
<script setup lang="ts">
import { healthLabel, healthType } from '@/views/health/format';
import type { HealthCard } from '@/views/health/useHealthCheck';

defineProps<{ cards: HealthCard[] }>();
</script>

<template>
  <div class="health-grid">
    <el-card v-for="card in cards" :key="card.title" shadow="never" class="health-card">
      <div class="health-card__head">
        <span>{{ card.title }}</span>
        <el-tag :type="healthType(card.status)" effect="plain">{{ healthLabel(card.status) }}</el-tag>
      </div>
      <strong>{{ card.value }}</strong>
      <p>{{ card.hint }}</p>
    </el-card>
  </div>
</template>

<style scoped>
.health-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 16px;
  margin-bottom: 18px;
}

.health-card {
  border-radius: 20px;
}

.health-card :deep(.el-card__body) {
  min-height: 138px;
}

.health-card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.health-card span,
.health-card p {
  color: var(--ink-soft);
}

.health-card strong {
  display: block;
  margin-top: 18px;
  font-size: 34px;
  line-height: 1;
  letter-spacing: -0.05em;
}

.health-card p {
  margin: 10px 0 0;
  line-height: 1.6;
}

@media (max-width: 1100px) {
  .health-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 760px) {
  .health-grid {
    grid-template-columns: 1fr;
  }
}
</style>
