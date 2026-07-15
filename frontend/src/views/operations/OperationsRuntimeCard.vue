<!--
  本组件展示中转入口运行快照表格。
  它接收控制面线路列表、运行指标状态和加载错误。
  速率、探针状态和快照时间格式化统一来自 operationsFormatters。
  组件不修改线路数据，也不访问后端接口。
-->
<script setup lang="ts">
import type { AccessLine, RuntimeMetricStatus } from '@/services/api';
import {
  formatLineRate,
  formatProbeLatency,
  formatReportedNumber,
  formatTime,
  lineEndpoint,
  probeStatusLabel,
  probeTagType,
  runtimeMetricTagType,
  runtimeStatusLabel,
} from '@/views/operations/operationsFormatters';

defineProps<{
  error: string;
  lines: AccessLine[];
  runtimeMetricStatus: RuntimeMetricStatus;
}>();
</script>

<template>
  <el-card shadow="never" class="operations-card runtime-card">
    <template #header>
      <div class="operations-card__header">
        <span>中转入口运行态</span>
        <el-tag :type="runtimeMetricTagType(runtimeMetricStatus)" effect="plain">
          {{ runtimeMetricStatus }} · {{ runtimeStatusLabel(runtimeMetricStatus) }}
        </el-tag>
      </div>
    </template>

    <p class="runtime-intro">
      展示中转入口运行快照；速率、在线、连接、独立 IP、探测延迟和状态缺失时按未上报处理。
    </p>
    <el-alert
      v-if="error"
      :title="error"
      type="info"
      show-icon
      :closable="false"
    />
    <el-empty v-else-if="lines.length === 0" description="暂无中转入口 / 未上报" />
    <el-table v-else :data="lines" class="runtime-table" stripe>
      <el-table-column label="中转入口" min-width="220">
        <template #default="{ row }: { row: AccessLine }">
          <div class="runtime-line__name">{{ row.name || row.uuid || '未命名中转入口' }}</div>
          <div class="runtime-line__meta">{{ row.accessNode || '中转节点未上报' }} · {{ lineEndpoint(row) }}</div>
        </template>
      </el-table-column>
      <el-table-column label="在线 / 连接 / IP" min-width="230">
        <template #default="{ row }: { row: AccessLine }">
          <dl class="runtime-stats">
            <div>
              <dt>在线</dt>
              <dd>{{ formatReportedNumber(row.runtimeOnlineUsers) }}</dd>
            </div>
            <div>
              <dt>连接</dt>
              <dd>{{ formatReportedNumber(row.runtimeActiveConnections) }}</dd>
            </div>
            <div>
              <dt>独立 IP</dt>
              <dd>{{ formatReportedNumber(row.runtimeUniqueClientIpCount) }}</dd>
            </div>
          </dl>
        </template>
      </el-table-column>
      <el-table-column label="中转入口速率" min-width="170">
        <template #default="{ row }: { row: AccessLine }">
          <div class="rate-pair">
            <span>上行</span>
            <strong>{{ formatLineRate(row.uplinkRateBps) }}</strong>
          </div>
          <div class="rate-pair">
            <span>下行</span>
            <strong>{{ formatLineRate(row.downlinkRateBps) }}</strong>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="探测" min-width="170">
        <template #default="{ row }: { row: AccessLine }">
          <el-tag :type="probeTagType(row.probeStatus)" effect="plain">
            {{ probeStatusLabel(row.probeStatus) }}
          </el-tag>
          <div class="runtime-line__meta">{{ formatProbeLatency(row.probeLatencyMs) }}</div>
          <div v-if="row.probeErrorSummary" class="probe-message">{{ row.probeErrorSummary }}</div>
        </template>
      </el-table-column>
      <el-table-column label="快照时间" min-width="190">
        <template #default="{ row }: { row: AccessLine }">
          <div class="runtime-line__meta">指标 {{ formatTime(row.metricCollectedAt) }}</div>
          <div class="runtime-line__meta">探测 {{ formatTime(row.lastProbeAt) }}</div>
        </template>
      </el-table-column>
    </el-table>
  </el-card>
</template>
