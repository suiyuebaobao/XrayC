<!--
  本文件展示中转节点健康小卡片。
  组件只展示节点心跳、配置同步、Agent 版本和健康原因。
  健康判断由 useHealthCheck 完成，避免展示层包含业务逻辑。
-->
<script setup lang="ts">
import { formatAge, healthLabel, healthType } from '@/views/health/format';
import type { NodeCard } from '@/views/health/useHealthCheck';

defineProps<{ nodes: NodeCard[] }>();
</script>

<template>
  <el-card shadow="never" class="health-section">
    <template #header>
      <div class="health-section__header">
        <span>中转节点健康</span>
        <el-tag effect="plain">自动刷新 15 秒</el-tag>
      </div>
    </template>
    <el-empty v-if="nodes.length === 0" description="暂无中转节点" />
    <div v-else class="node-card-grid">
      <article v-for="item in nodes" :key="item.node.id" class="node-card">
        <div class="node-card__title">
          <strong>{{ item.node.name || '未命名节点' }}</strong>
          <el-tag :type="healthType(item.status)" effect="dark">{{ healthLabel(item.status) }}</el-tag>
        </div>
        <p>{{ item.reason }}</p>
        <dl>
          <div>
            <dt>入口地址</dt>
            <dd>{{ item.node.publicHost || '未配置' }}</dd>
          </div>
          <div>
            <dt>心跳</dt>
            <dd>{{ formatAge(item.node.lastHeartbeatAt, item.node.heartbeatAgeSeconds) }}</dd>
          </div>
          <div>
            <dt>配置</dt>
            <dd>{{ item.node.configSynced ? '已同步' : '待同步' }}</dd>
          </div>
          <div>
            <dt>Agent</dt>
            <dd>{{ item.node.agentVersion || item.node.status || '未上报' }}</dd>
          </div>
        </dl>
      </article>
    </div>
  </el-card>
</template>

<style scoped>
.health-section {
  margin-bottom: 18px;
  border-radius: 22px;
}

.health-section__header,
.node-card__title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.node-card-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
}

.node-card {
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background:
    radial-gradient(circle at top right, rgba(33, 150, 243, 0.12), transparent 36%),
    rgba(255, 255, 255, 0.62);
}

.node-card p,
dt {
  color: var(--ink-soft);
}

.node-card p {
  margin: 10px 0 0;
  line-height: 1.6;
}

.node-card dl {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
  margin: 14px 0 0;
}

.node-card dl > div {
  padding: 10px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.62);
}

dt {
  font-size: 12px;
}

dd {
  margin: 4px 0 0;
  font-weight: 800;
  color: var(--ink);
  word-break: break-all;
}

@media (max-width: 1100px) {
  .node-card-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 760px) {
  .node-card-grid,
  .node-card dl {
    grid-template-columns: 1fr;
  }
}
</style>
