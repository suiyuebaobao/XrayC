<!--
  本文件展示中转节点页面顶部的统计卡片。
  组件只接收 useAccessLinesPage 已计算好的数量。
  它负责视觉布局，不请求接口也不维护表单状态。
  拆出后 AccessLinesPage 只负责页面编排。
-->
<script setup lang="ts">
defineProps<{
  nodeCount: number;
  totalLineCount: number;
  availableLineCount: number;
  linePoolCount: number;
}>();
</script>

<template>
  <el-row :gutter="16" class="summary-row">
    <el-col :span="8">
      <el-card shadow="never">
        <span>中转节点</span>
        <strong>{{ nodeCount }}</strong>
      </el-card>
    </el-col>
    <el-col :span="8">
      <el-card shadow="never">
        <span>运行入口数</span>
        <strong>{{ totalLineCount }}</strong>
      </el-card>
    </el-col>
    <el-col :span="8">
      <el-card shadow="never">
        <span>可用线路</span>
        <strong>{{ availableLineCount }} / {{ linePoolCount }}</strong>
      </el-card>
    </el-col>
  </el-row>
</template>

<style scoped>
.summary-row {
  margin-bottom: 16px;
}

.summary-row :deep(.el-card__body) {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.summary-row span {
  color: #667085;
  font-size: 13px;
}

.summary-row strong {
  color: #111827;
  font-size: 26px;
}
</style>
