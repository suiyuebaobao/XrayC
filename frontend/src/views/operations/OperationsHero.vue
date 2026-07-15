<!--
  本组件展示运营中心顶部概览横幅。
  它只负责 summary 时间、运行态标签和资源计数的展示。
  数据加载和状态推导由 useOperationsPage 提供。
  样式类由 OperationsPage 中的运营中心共享样式维护。
-->
<script setup lang="ts">
import type { OperationsSummary, RuntimeMetricStatus } from '@/services/api';
import { formatKnownNumber, formatTime, runtimeMetricTagType } from '@/views/operations/operationsFormatters';

defineProps<{
  summary: OperationsSummary | undefined;
  runtimeMetricStatus: RuntimeMetricStatus;
}>();
</script>

<template>
  <el-card shadow="never" class="operations-hero">
    <div>
      <p class="eyebrow">Operations Summary</p>
      <h2>中转运营态</h2>
      <p>Summary 生成：{{ formatTime(summary?.generatedAt, '未知') }}</p>
      <p>指标上报：{{ formatTime(summary?.latestMetricAt, '未上报') }}</p>
    </div>
    <div class="operations-hero__tags">
      <el-tag :type="runtimeMetricTagType(runtimeMetricStatus)" effect="dark">
        运行态 {{ runtimeMetricStatus }}
      </el-tag>
      <el-tag effect="plain">中转节点 {{ formatKnownNumber(summary?.accessNodeCount) }}</el-tag>
      <el-tag effect="plain">线路 {{ formatKnownNumber(summary?.accessLineCount) }}</el-tag>
      <el-tag effect="plain">分组 {{ formatKnownNumber(summary?.lineGroupCount) }}</el-tag>
      <el-tag effect="plain">账本记录 {{ formatKnownNumber(summary?.ledgerCount) }}</el-tag>
    </div>
  </el-card>
</template>
