<!--
  本组件展示流量账本按中转入口汇总的流量排行。
  组件只消费已加载的排行行、错误信息和顶部提示。
  字节、时间和入口地址格式化统一复用 operationsFormatters。
  它不产生模拟数据，也不直接访问 API。
-->
<script setup lang="ts">
import type { OperationsLedgerRankingItem } from '@/services/api';
import {
  formatBytes,
  formatKnownNumber,
  formatTime,
  rankingEndpoint,
} from '@/views/operations/operationsFormatters';

defineProps<{
  error: string;
  hint: string;
  rows: OperationsLedgerRankingItem[];
}>();
</script>

<template>
  <el-card shadow="never" class="operations-card ledger-ranking-card">
    <template #header>
      <div class="operations-card__header">
        <span>账本流量排行</span>
        <el-tag effect="plain">{{ hint }}</el-tag>
      </div>
    </template>

    <p class="runtime-intro">
      按中转入口汇总保留明细和历史日汇总的真实增量与扣费字节，默认按扣费流量降序展示，不使用模拟数据。
    </p>
    <el-alert
      v-if="error"
      :title="error"
      type="warning"
      show-icon
      :closable="false"
    />
    <el-empty v-else-if="rows.length === 0" description="暂无流量账本记录" />
    <el-table v-else :data="rows" class="runtime-table ledger-ranking-table" stripe>
      <el-table-column label="#" width="72">
        <template #default="{ row }: { row: OperationsLedgerRankingItem }">
          <span class="ledger-rank">#{{ row.rank }}</span>
        </template>
      </el-table-column>
      <el-table-column label="中转入口" min-width="240">
        <template #default="{ row }: { row: OperationsLedgerRankingItem }">
          <div class="runtime-line__name">{{ row.accessLineName || row.accessLineId || '未命名中转入口' }}</div>
          <div class="runtime-line__meta">{{ row.accessNodeName || '中转节点未上报' }} · {{ rankingEndpoint(row) }}</div>
          <div class="ledger-tags">
            <el-tag v-if="row.region" size="small" effect="plain">{{ row.region }}</el-tag>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="真实流量" min-width="190" sortable prop="realBytes">
        <template #default="{ row }: { row: OperationsLedgerRankingItem }">
          <strong>{{ formatBytes(row.realBytes) }}</strong>
          <div class="runtime-line__meta">
            上行 {{ formatBytes(row.deltaUplink) }} / 下行 {{ formatBytes(row.deltaDownlink) }}
          </div>
        </template>
      </el-table-column>
      <el-table-column label="扣费流量" min-width="150" sortable prop="billedBytes">
        <template #default="{ row }: { row: OperationsLedgerRankingItem }">
          <strong>{{ formatBytes(row.billedBytes) }}</strong>
        </template>
      </el-table-column>
      <el-table-column label="账本 / 最后入账" min-width="190">
        <template #default="{ row }: { row: OperationsLedgerRankingItem }">
          <div>账本 {{ formatKnownNumber(row.ledgerCount) }}</div>
          <div class="runtime-line__meta">{{ formatTime(row.latestCollectedAt, '未入账') }}</div>
        </template>
      </el-table-column>
    </el-table>
  </el-card>
</template>
