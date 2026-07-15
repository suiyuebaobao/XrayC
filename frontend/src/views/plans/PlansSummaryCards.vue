<!--
  本文件展示套餐分组页面顶部统计卡片。
  它只接收已计算好的套餐数量、基础套餐和分组绑定统计。
  统计规则放在 usePlansPage 中，组件不直接请求接口。
  拆出后 PlansPage 只负责页面编排，便于控制主文件行数。
-->
<script setup lang="ts">
import type { PlanInfo } from '@/services/api';

defineProps<{
  plans: PlanInfo[];
  defaultPlan?: PlanInfo;
  authorizedLineGroupCount: number;
  lineGroupBindingCount: number;
}>();
</script>

<template>
  <div class="metric-grid">
    <el-card shadow="never" class="metric-card">
      <span>套餐数量</span>
      <strong>{{ plans.length }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>基础套餐</span>
      <strong class="metric-card__text">{{ defaultPlan?.name ?? '未配置' }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>分组</span>
      <strong>{{ authorizedLineGroupCount }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>套餐绑定</span>
      <strong>{{ lineGroupBindingCount }}</strong>
    </el-card>
  </div>
</template>

<style scoped>
.metric-card__text {
  overflow: hidden;
  max-width: 100%;
  font-size: 28px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
