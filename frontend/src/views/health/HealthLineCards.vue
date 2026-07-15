<!--
  本文件展示运行入口健康小卡片。
  组件只负责展示运行入口、速率、在线数、探针和快照时间。
  线路健康结论由 useHealthCheck 根据真实运行数据计算。
-->
<script setup lang="ts">
import {
  formatNumber,
  formatRate,
  formatTime,
  healthLabel,
  healthType,
  lineEndpoint,
  probeStatusLabel,
} from '@/views/health/format';
import type { LineCard } from '@/views/health/useHealthCheck';

defineProps<{
  lines: LineCard[];
  refreshedLabel: string;
}>();
</script>

<template>
  <el-card shadow="never" class="health-section">
    <template #header>
      <div class="health-section__header">
        <span>线路健康</span>
        <span>最后刷新：{{ refreshedLabel }}</span>
      </div>
    </template>
    <el-empty v-if="lines.length === 0" description="暂无中转入口" />
    <div v-else class="line-card-grid">
      <article v-for="item in lines" :key="item.line.uuid" class="line-card">
        <div class="line-card__title">
          <strong>{{ item.line.name || '未命名线路' }}</strong>
          <el-tag :type="healthType(item.status)" effect="plain">{{ healthLabel(item.status) }}</el-tag>
        </div>
        <p>{{ item.reason }}</p>
        <div class="line-card__endpoint">{{ lineEndpoint(item.line) }}</div>
        <dl>
          <div>
            <dt>下行</dt>
            <dd>{{ formatRate(item.line.downlinkRateBps) }}</dd>
          </div>
          <div>
            <dt>上行</dt>
            <dd>{{ formatRate(item.line.uplinkRateBps) }}</dd>
          </div>
          <div>
            <dt>在线</dt>
            <dd>{{ formatNumber(item.line.runtimeOnlineUsers) }}</dd>
          </div>
          <div>
            <dt>探针</dt>
            <dd>{{ probeStatusLabel(item.line.probeStatus) }}</dd>
          </div>
        </dl>
        <p class="line-card__time">
          指标 {{ formatTime(item.line.metricCollectedAt) }} · 探针 {{ formatTime(item.line.lastProbeAt) }}
        </p>
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
.line-card__title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.line-card-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
}

.line-card {
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background:
    radial-gradient(circle at top right, rgba(33, 150, 243, 0.12), transparent 36%),
    rgba(255, 255, 255, 0.62);
}

.line-card p,
.line-card__endpoint,
.line-card__time,
dt {
  color: var(--ink-soft);
}

.line-card p {
  margin: 10px 0 0;
  line-height: 1.6;
}

.line-card dl {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
  margin: 14px 0 0;
}

.line-card dl > div {
  padding: 10px;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.62);
}

.line-card__endpoint,
.line-card__time {
  margin-top: 10px;
  font-size: 13px;
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
  .line-card-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 760px) {
  .line-card-grid,
  .line-card dl {
    grid-template-columns: 1fr;
  }
}
</style>
