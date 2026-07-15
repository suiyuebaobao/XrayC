<!--
  本组件展示配置待同步节点和最近控制面事件。
  待同步节点列表来自控制面，最近事件来自运营 summary。
  组件只负责两列卡片布局与基础状态展示。
  事件类型、时间和待同步标签由共享格式化函数处理。
-->
<script setup lang="ts">
import type { AccessNodeSummary, OperationsEvent } from '@/services/api';
import {
  dirtyNodeHint,
  dirtyTagType,
  eventTitle,
  eventType,
  formatKnownNumber,
  formatTime,
} from '@/views/operations/operationsFormatters';

defineProps<{
  dirtyNodeCount: number | null;
  dirtyNodes: AccessNodeSummary[];
  nodeDetailError: string;
  recentEvents: OperationsEvent[];
}>();
</script>

<template>
  <el-row :gutter="18" class="section-row">
    <el-col :xs="24" :lg="14">
      <el-card shadow="never" class="operations-card">
        <template #header>
          <div class="operations-card__header">
            <span>配置待同步节点</span>
            <el-tag :type="dirtyTagType(dirtyNodeCount)" effect="plain">
              {{ dirtyNodeHint(dirtyNodeCount) }}
            </el-tag>
          </div>
        </template>

        <div class="dirty-summary">
          <div>
            <span>待同步节点数</span>
            <strong>{{ formatKnownNumber(dirtyNodeCount) }}</strong>
          </div>
          <p>配置变更后，节点运行组件拉取并应用成功前会保持待同步状态。</p>
        </div>

        <el-alert
          v-if="nodeDetailError"
          :title="nodeDetailError"
          type="info"
          show-icon
          :closable="false"
        />
        <el-empty v-else-if="dirtyNodes.length === 0" description="暂无待同步节点" />
        <el-table v-else :data="dirtyNodes" stripe>
          <el-table-column prop="name" label="节点" min-width="150" />
          <el-table-column prop="publicHost" label="访问地址" min-width="180" />
          <el-table-column prop="id" label="节点 ID" min-width="220" />
        </el-table>
      </el-card>
    </el-col>

    <el-col :xs="24" :lg="10">
      <el-card shadow="never" class="operations-card">
        <template #header>最近事件</template>
        <el-timeline v-if="recentEvents.length > 0">
          <el-timeline-item
            v-for="event in recentEvents"
            :key="event.id"
            :type="eventType(event.level)"
            :timestamp="formatTime(event.time)"
          >
            {{ eventTitle(event) }}
          </el-timeline-item>
        </el-timeline>
        <el-empty v-else description="暂无事件 / 未上报" />
      </el-card>
    </el-col>
  </el-row>
</template>
