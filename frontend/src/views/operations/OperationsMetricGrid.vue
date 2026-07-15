<!--
  本组件展示运营中心顶部指标卡片网格。
  卡片文案、数值和提示由 useOperationsPage 预先格式化。
  组件不读取接口，也不自行推导运行态含义。
  样式复用全局 metric-grid 与运营中心 hint 类。
-->
<script setup lang="ts">
import type { OperationsMetricCard } from '@/views/operations/useOperationsPage';

defineProps<{
  cards: OperationsMetricCard[];
}>();
</script>

<template>
  <div class="metric-grid">
    <el-card v-for="card in cards" :key="card.label" shadow="never" class="metric-card">
      <span>{{ card.label }}</span>
      <strong>{{ card.value }}</strong>
      <p class="operations-metric__hint">{{ card.hint }}</p>
    </el-card>
  </div>
</template>
