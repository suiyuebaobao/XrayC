<!--
  本组件展示线路主动探测列表和触发按钮。
  它只发出 trigger 事件，由父页面 composable 调用后端接口。
  表格不展示线路地址或凭据，只展示资源名、协议和健康状态。
  探测按钮的 loading/disabled 状态由父级传入的线路 ID 控制。
-->
<script setup lang="ts">
import {
  exitMemberStatusLabel,
  exitMemberStatusType,
  formatKnownNumber,
} from '@/views/operations/operationsFormatters';
import type { ProbeTarget } from '@/views/operations/useOperationsPage';

defineProps<{
  error: string;
  message: string;
  nodeDetailError: string;
  probingExitEndpointId: string;
  targets: ProbeTarget[];
}>();

const emit = defineEmits<{
  (event: 'trigger', target: ProbeTarget): void;
}>();
</script>

<template>
  <el-card shadow="never" class="operations-card operations-probe-card">
    <template #header>
      <div class="operations-card__header">
        <span>主动探测</span>
        <el-tag effect="plain">线路 {{ formatKnownNumber(targets.length) }}</el-tag>
      </div>
    </template>

    <p class="runtime-intro">
      手动登记一次线路探测任务，由节点运行组件拉取后执行；页面只展示资源名、协议和状态，不展示线路地址或凭据。
    </p>
    <el-alert
      v-if="error"
      class="settings-alert"
      :title="error"
      type="warning"
      show-icon
      :closable="false"
    />
    <el-alert
      v-else-if="message"
      class="settings-alert"
      :title="message"
      type="success"
      show-icon
      :closable="false"
    />
    <el-alert
      v-if="nodeDetailError"
      :title="nodeDetailError"
      type="info"
      show-icon
      :closable="false"
    />
    <el-empty v-else-if="targets.length === 0" description="暂无可探测线路" />
    <el-table v-else :data="targets" class="runtime-table" stripe>
      <el-table-column label="线路" min-width="220">
        <template #default="{ row }: { row: ProbeTarget }">
          <div class="runtime-line__name">{{ row.member.resourceName || '未命名线路' }}</div>
          <div class="runtime-line__meta">{{ row.poolName || '分组未上报' }}</div>
        </template>
      </el-table-column>
      <el-table-column label="协议" min-width="110">
        <template #default="{ row }: { row: ProbeTarget }">
          {{ row.member.outboundType }}
        </template>
      </el-table-column>
      <el-table-column label="线路状态" min-width="150">
        <template #default="{ row }: { row: ProbeTarget }">
          <el-tag :type="exitMemberStatusType(row.member)" effect="plain">
            {{ exitMemberStatusLabel(row.member) }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="140" fixed="right">
        <template #default="{ row }: { row: ProbeTarget }">
          <el-button
            text
            type="primary"
            :loading="probingExitEndpointId === row.member.id"
            :disabled="!!probingExitEndpointId && probingExitEndpointId !== row.member.id"
            @click="emit('trigger', row)"
          >
            主动探测
          </el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>
</template>
