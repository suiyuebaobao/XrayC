<!--
  本文件是监控中心「流量」面板，聚合每节点/每线路业务流量与线路实时态。
  业务流量复用 health/HealthTrafficPanel（node/line items，来自 trafficHealth）。
  线路实时速率/在线用户/活跃连接来自控制面 access_lines 的运行态字段。
  本面板只读真实聚合与实时上报，不生成模拟数据。
-->
<script setup lang="ts">
import { computed, ref } from 'vue';
import type { AccessLine, TrafficHealthSummary } from '@/services/api';
import HealthTrafficPanel from '@/views/health/HealthTrafficPanel.vue';
import {
  formatLineRate,
  formatReportedNumber,
  lineEndpoint,
} from '@/views/operations/operationsFormatters';
import { runtimeStatusLabel } from '@/views/health/format';

const props = defineProps<{
  trafficHealth: TrafficHealthSummary | null;
  lines: AccessLine[];
}>();

const activeTab = ref('node');

const sortedLines = computed(() => (
  [...props.lines].sort((left, right) => (
    Number(right.downlinkRateBps ?? 0) - Number(left.downlinkRateBps ?? 0)
    || left.name.localeCompare(right.name, 'zh-CN')
  ))
));

function metricTagType(line: AccessLine) {
  if (line.metricStatus === 'fresh') {
    return 'success';
  }
  return line.metricStatus === 'stale' ? 'warning' : 'info';
}
</script>

<template>
  <el-empty v-if="!trafficHealth" description="流量聚合未上报。" />

  <template v-else>
    <el-card shadow="never" class="monitor-traffic-card">
      <el-tabs v-model="activeTab" class="monitor-traffic-card__tabs">
        <el-tab-pane label="中转节点流量" name="node">
          <HealthTrafficPanel
            kind="node"
            title="中转节点流量"
            description="按中转服务器聚合真实业务流量。"
            :items="trafficHealth.nodeItems"
            :generated-at="trafficHealth.generatedAt"
          />
        </el-tab-pane>
        <el-tab-pane label="线路流量" name="line">
          <HealthTrafficPanel
            kind="line"
            title="线路流量"
            description="按中转入口线路聚合流量，便于发现消耗与异常。"
            :items="trafficHealth.lineItems"
            :generated-at="trafficHealth.generatedAt"
          />
        </el-tab-pane>
      </el-tabs>
    </el-card>

    <el-card shadow="never" class="monitor-traffic-card">
      <template #header><strong>线路实时态</strong></template>
      <el-empty v-if="sortedLines.length === 0" description="暂无线路实时上报。" />
      <el-table v-else :data="sortedLines" class="monitor-line-table">
        <el-table-column label="线路" min-width="200">
          <template #default="{ row }">
            <strong>{{ row.name || row.uuid }}</strong>
            <div class="monitor-line__meta">{{ lineEndpoint(row) }}</div>
          </template>
        </el-table-column>
        <el-table-column label="在线用户" width="110">
          <template #default="{ row }">{{ formatReportedNumber(row.runtimeOnlineUsers) }}</template>
        </el-table-column>
        <el-table-column label="活跃连接" width="110">
          <template #default="{ row }">{{ formatReportedNumber(row.runtimeActiveConnections) }}</template>
        </el-table-column>
        <el-table-column label="上行速率" min-width="130">
          <template #default="{ row }">{{ formatLineRate(row.uplinkRateBps) }}</template>
        </el-table-column>
        <el-table-column label="下行速率" min-width="130">
          <template #default="{ row }">{{ formatLineRate(row.downlinkRateBps) }}</template>
        </el-table-column>
        <el-table-column label="运行态" width="110">
          <template #default="{ row }">
            <el-tag :type="metricTagType(row)" effect="plain">{{ runtimeStatusLabel(row.metricStatus) }}</el-tag>
          </template>
        </el-table-column>
      </el-table>
    </el-card>
  </template>
</template>

<style scoped>
.monitor-traffic-card {
  margin-bottom: 18px;
  border-radius: 22px;
}

.monitor-traffic-card__tabs :deep(.el-tabs__item) {
  height: 44px;
  font-weight: 800;
}

.monitor-line-table {
  width: 100%;
}

.monitor-line-table strong {
  color: var(--ink);
}

.monitor-line__meta {
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 13px;
}
</style>
