<!--
  本文件是监控中心「节点资源」面板，展示每个中转节点的 CPU/内存/磁盘占用与今日业务流量。
  资源指标消费控制面读模型 access_nodes[].runtimeMetrics；今日上/下/总流量由父层 traffic 字典传入（按节点 id 取，无数据按 0）。
  runtimeMetrics 为 null 时显示「未上报/待 agent 升级」，区分真实 0 与缺数据。
  每行「详情」按钮打开 NodeTrafficDetailDialog 看该节点各时间段趋势；本面板保持纯 props 可测。
-->
<script setup lang="ts">
import { computed, ref } from 'vue';
import type { AccessNodeSummary, NodeTrafficSummary } from '@/services/api';
import { formatBytes, formatTime } from '@/views/operations/operationsFormatters';
import {
  bytesUsagePercent,
  formatPctMilli,
  pctMilliToPercent,
  usageColor,
} from '@/views/monitor/format';
import NodeTrafficDetailDialog from '@/views/monitor/NodeTrafficDetailDialog.vue';

const props = defineProps<{
  nodes: AccessNodeSummary[] | null;
  error: string;
  // 节点 id → 今日业务流量汇总；缺省字典时所有节点按 0 展示（未接父层数据也不崩）。
  traffic?: Record<string, NodeTrafficSummary>;
  // 今日流量加载报错（与资源指标 error 分开展示）。
  trafficError?: string;
}>();

const rows = computed(() => props.nodes ?? []);
const reportedCount = computed(() => rows.value.filter((node) => node.runtimeMetrics !== null).length);

// 详情弹窗状态：选中的节点 id/名与可见性，交给 NodeTrafficDetailDialog 自取趋势。
const detailVisible = ref(false);
const detailNodeId = ref('');
const detailNodeName = ref('');

function cpuPercent(node: AccessNodeSummary): number {
  return pctMilliToPercent(node.runtimeMetrics?.cpuPctMilli);
}

function memPercent(node: AccessNodeSummary): number {
  const metrics = node.runtimeMetrics;
  return metrics ? bytesUsagePercent(metrics.memUsedBytes, metrics.memTotalBytes) : 0;
}

function diskPercent(node: AccessNodeSummary): number {
  const metrics = node.runtimeMetrics;
  return metrics ? bytesUsagePercent(metrics.diskUsedBytes, metrics.diskTotalBytes) : 0;
}

// 取某节点今日流量某字段；父层未传字典或该节点无数据时按 0（展示为「0 B」）。
function trafficBytes(node: AccessNodeSummary, field: 'uplinkBytes' | 'downlinkBytes' | 'totalBytes'): number {
  return props.traffic?.[node.id]?.[field] ?? 0;
}

function openDetail(node: AccessNodeSummary) {
  detailNodeId.value = node.id;
  detailNodeName.value = node.name || node.id;
  detailVisible.value = true;
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

  <el-empty v-else-if="rows.length === 0" description="暂无中转节点。" />

  <template v-else>
    <p class="monitor-panel__hint">
      每个中转节点的运行态资源占用与今日业务流量（当天本地 0 点至今），已上报 {{ reportedCount }} / {{ rows.length }} 个节点（null 为待 agent 升级上报）。
    </p>

    <el-alert
      v-if="trafficError"
      class="monitor-alert"
      :title="trafficError"
      type="info"
      show-icon
      :closable="false"
    />

    <el-table :data="rows" class="monitor-node-table">
      <el-table-column label="节点" min-width="180">
        <template #default="{ row }">
          <strong>{{ row.name || row.id }}</strong>
          <div class="monitor-node__meta">{{ row.publicHost || row.ipDirectAddress || '地址未上报' }}</div>
        </template>
      </el-table-column>
      <el-table-column label="CPU" min-width="200">
        <template #default="{ row }">
          <template v-if="row.runtimeMetrics">
            <el-progress
              :percentage="Number(cpuPercent(row).toFixed(1))"
              :color="usageColor(cpuPercent(row))"
              :stroke-width="12"
            />
            <div class="monitor-node__sub">{{ formatPctMilli(row.runtimeMetrics.cpuPctMilli) }}</div>
          </template>
          <el-tag v-else type="info" effect="plain">未上报 / 待 agent 升级</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="内存" min-width="220">
        <template #default="{ row }">
          <template v-if="row.runtimeMetrics">
            <el-progress
              :percentage="Number(memPercent(row).toFixed(1))"
              :color="usageColor(memPercent(row))"
              :stroke-width="12"
            />
            <div class="monitor-node__sub">
              {{ formatBytes(row.runtimeMetrics.memUsedBytes) }} / {{ formatBytes(row.runtimeMetrics.memTotalBytes) }}
            </div>
          </template>
          <el-tag v-else type="info" effect="plain">未上报</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="磁盘" min-width="220">
        <template #default="{ row }">
          <template v-if="row.runtimeMetrics">
            <el-progress
              :percentage="Number(diskPercent(row).toFixed(1))"
              :color="usageColor(diskPercent(row))"
              :stroke-width="12"
            />
            <div class="monitor-node__sub">
              {{ formatBytes(row.runtimeMetrics.diskUsedBytes) }} / {{ formatBytes(row.runtimeMetrics.diskTotalBytes) }}
            </div>
          </template>
          <el-tag v-else type="info" effect="plain">未上报</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="采集时间" min-width="170">
        <template #default="{ row }">
          {{ row.runtimeMetrics ? formatTime(row.runtimeMetrics.collectedAt) : '未上报' }}
        </template>
      </el-table-column>
      <el-table-column label="今日上行" min-width="110" align="right">
        <template #default="{ row }">
          {{ formatBytes(trafficBytes(row, 'uplinkBytes')) }}
        </template>
      </el-table-column>
      <el-table-column label="今日下行" min-width="110" align="right">
        <template #default="{ row }">
          {{ formatBytes(trafficBytes(row, 'downlinkBytes')) }}
        </template>
      </el-table-column>
      <el-table-column label="今日总流量" min-width="120" align="right">
        <template #default="{ row }">
          <strong>{{ formatBytes(trafficBytes(row, 'totalBytes')) }}</strong>
        </template>
      </el-table-column>
      <el-table-column label="操作" min-width="90" fixed="right">
        <template #default="{ row }">
          <el-button type="primary" link @click="openDetail(row)">详情</el-button>
        </template>
      </el-table-column>
    </el-table>

    <NodeTrafficDetailDialog
      v-model="detailVisible"
      :node-id="detailNodeId"
      :node-name="detailNodeName"
    />
  </template>
</template>

<style scoped>
.monitor-alert {
  margin-bottom: 14px;
}

.monitor-panel__hint {
  margin: 0 0 16px;
  color: var(--ink-soft);
  line-height: 1.7;
}

.monitor-node-table {
  width: 100%;
}

.monitor-node-table strong {
  color: var(--ink);
}

.monitor-node__meta {
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 13px;
}

.monitor-node__sub {
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 12px;
}
</style>
