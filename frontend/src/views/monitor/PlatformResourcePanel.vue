<!--
  本文件是监控中心「平台资源」面板，展示控制面（运行 api/worker 的中心机）资源占用。
  数据来自 GET /api/admin/access-operations/platform-metrics，全部真实只读。
  CPU 用环形、内存/磁盘用进度卡同时展示使用率与剩余率，数据库展示库总量 + 前几大表。
  disk.hostMounted=false 时提示磁盘为容器视图，需升级面板挂 host 盘。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type { PlatformMetrics } from '@/services/api';
import { formatBytes, formatTime } from '@/views/operations/operationsFormatters';
import {
  formatPctMilli,
  pctMilliToPercent,
  usageColor,
} from '@/views/monitor/format';

const props = defineProps<{
  metrics: PlatformMetrics | null;
  loading: boolean;
  error: string;
}>();

const cpuPercent = computed(() => pctMilliToPercent(props.metrics?.cpu.pctMilli));

const memory = computed(() => props.metrics?.memory ?? null);
const disk = computed(() => props.metrics?.disk ?? null);
const database = computed(() => props.metrics?.database ?? null);

const memoryPercent = computed(() => pctMilliToPercent(memory.value?.usedPctMilli));
const diskPercent = computed(() => pctMilliToPercent(disk.value?.usedPctMilli));

// 前几大表占库比例：用于在表格里画一条占比条，库为空时按 0。
function tableShare(bytes: number): number {
  const total = database.value?.totalBytes ?? 0;
  if (!total) {
    return 0;
  }
  return Math.min(100, Math.max(0, (bytes / total) * 100));
}
</script>

<template>
  <el-alert
    v-if="error"
    class="monitor-alert"
    :title="error"
    type="warning"
    show-icon
    :closable="false"
  />

  <el-skeleton v-if="loading && !metrics" :rows="6" animated />

  <el-empty v-else-if="!metrics" description="平台资源指标未上报，待中心机采集后展示。" />

  <template v-else>
    <p class="monitor-panel__hint">
      控制面（运行 API/Worker 的中心机）资源占用，最近采集：{{ formatTime(metrics.generatedAt) }}
    </p>

    <div class="monitor-resource-grid">
      <el-card shadow="never" class="monitor-card monitor-card--ring">
        <template #header><strong>CPU 使用率</strong></template>
        <el-progress
          type="dashboard"
          :percentage="Number(cpuPercent.toFixed(1))"
          :color="usageColor(cpuPercent)"
          :width="148"
        >
          <template #default>
            <div class="monitor-ring__value">{{ formatPctMilli(metrics.cpu.pctMilli) }}</div>
            <div class="monitor-ring__label">CPU 占用</div>
          </template>
        </el-progress>
      </el-card>

      <el-card shadow="never" class="monitor-card">
        <template #header>
          <div class="monitor-card__header">
            <strong>内存使用率</strong>
            <span>{{ formatBytes(memory?.usedBytes) }} / {{ formatBytes(memory?.totalBytes) }}</span>
          </div>
        </template>
        <el-progress
          :percentage="Number(memoryPercent.toFixed(1))"
          :color="usageColor(memoryPercent)"
          :stroke-width="16"
          :text-inside="true"
        />
        <dl class="monitor-usage-stats">
          <div>
            <dt>已用</dt>
            <dd>{{ formatPctMilli(memory?.usedPctMilli) }}</dd>
          </div>
          <div>
            <dt>剩余</dt>
            <dd>{{ formatPctMilli(memory?.freePctMilli) }}</dd>
          </div>
        </dl>
      </el-card>

      <el-card shadow="never" class="monitor-card">
        <template #header>
          <div class="monitor-card__header">
            <strong>磁盘使用率</strong>
            <span>{{ formatBytes(disk?.usedBytes) }} / {{ formatBytes(disk?.totalBytes) }}</span>
          </div>
        </template>
        <el-progress
          :percentage="Number(diskPercent.toFixed(1))"
          :color="usageColor(diskPercent)"
          :stroke-width="16"
          :text-inside="true"
        />
        <dl class="monitor-usage-stats">
          <div>
            <dt>已用</dt>
            <dd>{{ formatPctMilli(disk?.usedPctMilli) }}</dd>
          </div>
          <div>
            <dt>剩余</dt>
            <dd>{{ formatPctMilli(disk?.freePctMilli) }}</dd>
          </div>
          <div>
            <dt>挂载点</dt>
            <dd>{{ disk?.mountPoint || '未上报' }}</dd>
          </div>
        </dl>
        <el-alert
          v-if="disk && !disk.hostMounted"
          class="monitor-disk-alert"
          title="磁盘为容器视图，需升级面板挂 host 盘"
          type="info"
          show-icon
          :closable="false"
        />
      </el-card>
    </div>

    <el-card shadow="never" class="monitor-card monitor-db-card">
      <template #header>
        <div class="monitor-card__header">
          <strong>数据库存储量</strong>
          <el-tag effect="plain" type="info">总量 {{ formatBytes(database?.totalBytes) }}</el-tag>
        </div>
      </template>
      <el-empty
        v-if="!database || database.tables.length === 0"
        description="暂无表级存储明细"
      />
      <el-table v-else :data="database.tables" class="monitor-db-table">
        <el-table-column label="表" min-width="220">
          <template #default="{ row }">
            <strong>{{ row.name }}</strong>
          </template>
        </el-table-column>
        <el-table-column label="占用" width="150">
          <template #default="{ row }">{{ formatBytes(row.bytes) }}</template>
        </el-table-column>
        <el-table-column label="占库比例" min-width="200">
          <template #default="{ row }">
            <el-progress
              :percentage="Number(tableShare(row.bytes).toFixed(1))"
              :stroke-width="10"
              color="#909399"
            />
          </template>
        </el-table-column>
      </el-table>
    </el-card>
  </template>
</template>

<style scoped>
.monitor-alert,
.monitor-disk-alert {
  margin-bottom: 14px;
}

.monitor-panel__hint {
  margin: 0 0 16px;
  color: var(--ink-soft);
  line-height: 1.7;
}

.monitor-resource-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 16px;
  margin-bottom: 18px;
}

.monitor-card {
  border-radius: 18px;
}

.monitor-card--ring :deep(.el-card__body) {
  display: flex;
  justify-content: center;
}

.monitor-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.monitor-card__header span {
  color: var(--ink-soft);
  font-size: 13px;
}

.monitor-ring__value {
  font-size: 22px;
  font-weight: 900;
  letter-spacing: -0.03em;
}

.monitor-ring__label {
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 12px;
}

.monitor-usage-stats {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  margin: 16px 0 0;
}

.monitor-usage-stats > div {
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.56);
}

.monitor-usage-stats dt {
  color: var(--ink-soft);
  font-size: 12px;
}

.monitor-usage-stats dd {
  margin: 3px 0 0;
  font-weight: 800;
  color: var(--ink);
}

.monitor-db-card {
  margin-bottom: 18px;
}

.monitor-db-table strong {
  color: var(--ink);
}

@media (max-width: 1000px) {
  .monitor-resource-grid {
    grid-template-columns: 1fr;
  }

  .monitor-usage-stats {
    grid-template-columns: 1fr;
  }
}
</style>
