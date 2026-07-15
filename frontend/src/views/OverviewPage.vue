<!--
  本页面用于后台总览关键运营指标。
  它展示活跃用户、线路、可用线路和配置同步概况。
  页面只读取 summary API，不直接修改任何运行配置。
-->
<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type OverviewSummary } from '@/services/api';

const loading = ref(true);
const summary = ref<OverviewSummary>();
const configSyncPercentage = computed(() => {
  const total = summary.value?.accessNodeCount ?? 0;
  if (total <= 0) {
    return 0;
  }
  const dirty = Math.min(summary.value?.configDirtyNodes ?? 0, total);
  return Math.round(((total - dirty) / total) * 100);
});

onMounted(async () => {
  summary.value = await apiClient.getOverview();
  loading.value = false;
});
</script>

<template>
  <PageHeader title="概览" description="先维护出口管理，再通过入口管理创建入口出口绑定节点，分组用于套餐授权和归类。" />

  <el-skeleton v-if="loading" :rows="8" animated />
  <template v-else-if="summary">
    <div class="metric-grid">
      <el-card shadow="never" class="metric-card">
        <span>活跃用户</span>
        <strong>{{ summary.activeUsers }}</strong>
      </el-card>
      <el-card shadow="never" class="metric-card">
        <span>启用线路</span>
        <strong>{{ summary.activeAccessLines }}</strong>
      </el-card>
      <el-card shadow="never" class="metric-card">
        <span>可用线路</span>
        <strong>{{ summary.healthyExitPools }}</strong>
      </el-card>
    </div>

    <el-card shadow="never" class="section-row">
      <template #header>核心配置路径</template>
      <div class="core-flow">
        <RouterLink to="/admin/line-pool">
          <strong>出口管理</strong>
          <span>先维护线路地址、端口和协议凭据。</span>
        </RouterLink>
        <RouterLink to="/admin/line-groups">
          <strong>分组</strong>
          <span>从入口出口绑定节点中选择成员，按 AI、游戏、GPT、视频等用途归类。</span>
        </RouterLink>
        <RouterLink to="/admin/transit-nodes">
          <strong>中转节点</strong>
          <span>部署中转节点、查看运行状态，并批量添加本机出口服务。</span>
        </RouterLink>
        <RouterLink to="/admin/access-entries">
          <strong>入口管理</strong>
          <span>创建用户入口，并绑定一条或多条出口线路形成订阅节点。</span>
        </RouterLink>
        <RouterLink to="/admin/plans">
          <strong>套餐授权</strong>
          <span>按套餐授权用户可访问的分组。</span>
        </RouterLink>
      </div>
    </el-card>

    <el-row :gutter="18" class="section-row">
      <el-col :xs="24" :lg="16">
        <el-card shadow="never">
          <template #header>中转节点运行态</template>
          <div class="status-panel">
            <div>
              <span>待同步节点</span>
              <strong>{{ summary.configDirtyNodes }}</strong>
              <p>配置变化后等待节点运行组件拉取并应用。</p>
            </div>
            <el-progress :percentage="configSyncPercentage" :stroke-width="14" striped />
          </div>
        </el-card>
      </el-col>
      <el-col :xs="24" :lg="8">
        <el-card shadow="never">
          <template #header>近期事件</template>
          <el-timeline>
            <el-timeline-item
              v-for="event in summary.recentEvents"
              :key="event.id"
              :type="event.level === 'danger' ? 'danger' : event.level"
              :timestamp="event.time"
            >
              {{ event.title }}
            </el-timeline-item>
          </el-timeline>
        </el-card>
      </el-col>
    </el-row>
  </template>
</template>

<style scoped>
.core-flow {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
}

.core-flow a {
  display: grid;
  gap: 8px;
  padding: 18px;
  border: 1px solid var(--border);
  border-radius: 18px;
  color: var(--accent-dark);
  background: rgba(255, 255, 255, 0.7);
}

.core-flow span {
  color: var(--ink-soft);
  line-height: 1.6;
}

@media (max-width: 900px) {
  .core-flow {
    grid-template-columns: 1fr;
  }
}
</style>
