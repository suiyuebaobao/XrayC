<!--
  本文件是「监控中心」装配入口（阶段E 前端合并）。
  它把原运营中心与健康检查两页合并到一个五 tab 页面，并新增平台/节点资源与流量 tab。
  数据复用双 composable：useOperationsPage（运营 summary/控制面/账本）+ useHealthCheck（健康），
  外加新 usePlatformMetrics（控制面资源）；刷新按钮一次性触发三者重新加载。
  本文件只做装配与刷新编排，所有卡片/列表/资源面板都拆到 operations/health/monitor 子组件。
  共享样式沿用运营中心非 scoped 约定，以便覆盖拆出的局部组件结构类。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import OperationsActivitySection from '@/views/operations/OperationsActivitySection.vue';
import OperationsHero from '@/views/operations/OperationsHero.vue';
import OperationsLedgerRankingCard from '@/views/operations/OperationsLedgerRankingCard.vue';
import OperationsMetricGrid from '@/views/operations/OperationsMetricGrid.vue';
import OperationsProbeCard from '@/views/operations/OperationsProbeCard.vue';
import OperationsRuntimeCard from '@/views/operations/OperationsRuntimeCard.vue';
import OperationsStatusSettingsSection from '@/views/operations/OperationsStatusSettingsSection.vue';
import { useOperationsPage } from '@/views/operations/useOperationsPage';
import HealthLineCards from '@/views/health/HealthLineCards.vue';
import HealthNodeCards from '@/views/health/HealthNodeCards.vue';
import HealthPoolList from '@/views/health/HealthPoolList.vue';
import HealthSummaryCards from '@/views/health/HealthSummaryCards.vue';
import HealthTrafficPanel from '@/views/health/HealthTrafficPanel.vue';
import { formatTime } from '@/views/health/format';
import { useHealthCheck } from '@/views/health/useHealthCheck';
import PlatformResourcePanel from '@/views/monitor/PlatformResourcePanel.vue';
import NodeResourcePanel from '@/views/monitor/NodeResourcePanel.vue';
import MonitorTrafficPanel from '@/views/monitor/MonitorTrafficPanel.vue';
import { usePlatformMetrics } from '@/views/monitor/usePlatformMetrics';
import { useNodeTraffic } from '@/views/monitor/useNodeTraffic';

const ops = useOperationsPage();
const health = useHealthCheck();
const platform = usePlatformMetrics();
const nodeTraffic = useNodeTraffic();

const activeTab = ref('overview');
const healthBoardTab = ref('node');

// 平台资源与节点今日流量走手动加载（其它两个 composable 自身 onMounted 已自动加载）。
onMounted(() => {
  void platform.load();
  void nodeTraffic.load();
});

function refreshAll() {
  void ops.load();
  void health.load();
  void platform.load();
  void nodeTraffic.load();
}

function alertType(severity: string) {
  return severity === 'danger' ? 'danger' : severity === 'warning' ? 'warning' : 'info';
}
</script>

<template>
  <PageHeader title="监控中心" description="运行总览、平台/节点资源占用、业务流量与健康检查统一入口，全部来自真实运行上报。">
    <el-button :icon="Refresh" :loading="ops.loading.value || platform.loading.value || nodeTraffic.loading.value" @click="refreshAll">刷新</el-button>
  </PageHeader>

  <el-tabs v-model="activeTab" class="monitor-tabs">
    <!-- 运行总览：原运营中心全部内容 -->
    <el-tab-pane label="运行总览" name="overview">
      <el-alert
        v-if="ops.summaryError.value"
        class="operations-alert"
        :title="ops.summaryError.value"
        type="warning"
        show-icon
        :closable="false"
      />

      <el-skeleton v-if="ops.loading.value && !ops.summary.value" :rows="8" animated />
      <template v-else>
        <OperationsHero
          :summary="ops.summary.value"
          :runtime-metric-status="ops.runtimeMetricStatus.value"
        />

        <el-alert
          v-if="ops.summary.value && ops.runtimeMetricStatus.value !== 'fresh'"
          class="operations-alert"
          :title="ops.runtimeMetricAlertTitle.value"
          :type="ops.runtimeMetricAlertType.value"
          show-icon
          :closable="false"
        />

        <OperationsMetricGrid :cards="ops.metricCards.value" />

        <el-card v-if="ops.summary.value" shadow="never" class="operations-card production-alerts-card">
          <template #header>
            <div class="operations-card__header">
              <strong>生产告警</strong>
              <el-tag
                :type="ops.alertSummary.value.dangerCount > 0 ? 'danger' : ops.alertSummary.value.warningCount > 0 ? 'warning' : 'success'"
                effect="plain"
              >
                {{ ops.alertSummary.value.totalCount }} 条
              </el-tag>
            </div>
          </template>
          <el-empty v-if="ops.productionAlerts.value.length === 0" description="暂无生产告警" />
          <el-table v-else :data="ops.productionAlerts.value" class="production-alerts-table">
            <el-table-column label="级别" width="100">
              <template #default="{ row }">
                <el-tag :type="alertType(row.severity)" effect="dark">{{ row.severity }}</el-tag>
              </template>
            </el-table-column>
            <el-table-column label="告警" min-width="260">
              <template #default="{ row }">
                <strong>{{ row.title }}</strong>
                <div class="production-alerts-table__meta">{{ row.message }}</div>
              </template>
            </el-table-column>
            <el-table-column label="资源" min-width="180">
              <template #default="{ row }">
                {{ row.resourceName || row.resourceId || row.resourceType }}
              </template>
            </el-table-column>
            <el-table-column label="检测时间" prop="detectedAt" min-width="180" />
          </el-table>
        </el-card>

        <OperationsLedgerRankingCard
          :error="ops.ledgerRankingError.value"
          :hint="ops.ledgerRankingHint.value"
          :rows="ops.ledgerRankingRows.value"
        />
        <OperationsRuntimeCard
          :error="ops.nodeDetailError.value"
          :lines="ops.runtimeLines.value"
          :runtime-metric-status="ops.runtimeMetricStatus.value"
        />
        <OperationsProbeCard
          :error="ops.probeError.value"
          :message="ops.probeMessage.value"
          :node-detail-error="ops.nodeDetailError.value"
          :probing-exit-endpoint-id="ops.probingExitEndpointId.value"
          :targets="ops.operationProbeTargets.value"
          @trigger="ops.triggerOperationsProbe"
        />
        <OperationsActivitySection
          :dirty-node-count="ops.dirtyNodeCount.value"
          :dirty-nodes="ops.dirtyNodes.value"
          :node-detail-error="ops.nodeDetailError.value"
          :recent-events="ops.recentEvents.value"
        />
        <OperationsStatusSettingsSection
          :saving-settings="ops.savingSettings.value"
          :settings-error="ops.settingsError.value"
          :settings-form="ops.settingsForm.value"
          :settings-message="ops.settingsMessage.value"
          :summary="ops.summary.value"
          @save="ops.saveSettings"
        />
      </template>
    </el-tab-pane>

    <!-- 平台资源：控制面 CPU/内存/磁盘 + 数据库存储 -->
    <el-tab-pane label="平台资源" name="platform">
      <PlatformResourcePanel
        :metrics="platform.metrics.value"
        :loading="platform.loading.value"
        :error="platform.error.value"
      />
    </el-tab-pane>

    <!-- 节点资源：每节点 CPU/内存/磁盘（消费 access_nodes[].runtimeMetrics） -->
    <el-tab-pane label="节点资源" name="node-resource">
      <NodeResourcePanel
        :nodes="ops.accessNodes.value"
        :error="ops.nodeDetailError.value"
        :traffic="nodeTraffic.trafficByNode.value"
        :traffic-error="nodeTraffic.error.value"
      />
    </el-tab-pane>

    <!-- 流量：节点/线路业务流量 + 线路实时态 -->
    <el-tab-pane label="流量" name="traffic">
      <MonitorTrafficPanel
        :traffic-health="ops.summary.value?.trafficHealth ?? null"
        :lines="ops.runtimeLines.value"
      />
    </el-tab-pane>

    <!-- 健康检查：原健康检查页全部内容 -->
    <el-tab-pane label="健康检查" name="health">
      <el-alert
        v-if="health.errorMessage.value"
        class="health-alert"
        :title="health.errorMessage.value"
        type="warning"
        show-icon
        :closable="false"
      />

      <el-skeleton v-if="health.loading.value && !health.summary.value" :rows="8" animated />
      <template v-else>
        <HealthSummaryCards :cards="health.overviewCards.value" />
        <el-card v-if="health.summary.value?.trafficHealth" shadow="never" class="health-board">
          <el-tabs v-model="healthBoardTab" class="health-board__tabs">
            <el-tab-pane label="中转节点健康" name="node">
              <HealthTrafficPanel
                kind="node"
                title="中转节点健康"
                description="按中转服务器聚合真实业务流量。"
                :items="health.summary.value.trafficHealth.nodeItems"
                :generated-at="health.summary.value.trafficHealth.generatedAt"
              />
            </el-tab-pane>
            <el-tab-pane label="出口线路健康" name="exit">
              <HealthTrafficPanel
                kind="exit"
                title="出口线路健康"
                description="按线路聚合流量，便于发现消耗和异常。"
                :items="health.summary.value.trafficHealth.exitItems"
                :generated-at="health.summary.value.trafficHealth.generatedAt"
              />
            </el-tab-pane>
            <el-tab-pane label="分组健康" name="group">
              <HealthTrafficPanel
                kind="group"
                title="分组健康"
                description="按分组聚合真实流量。"
                :items="health.summary.value.trafficHealth.groupItems"
                :generated-at="health.summary.value.trafficHealth.generatedAt"
              />
            </el-tab-pane>
          </el-tabs>
        </el-card>

        <el-collapse class="health-details">
          <el-collapse-item title="高级健康详情" name="advanced">
            <HealthNodeCards :nodes="health.nodeCards.value" />
            <HealthLineCards :lines="health.lineCards.value" :refreshed-label="formatTime(health.refreshedAt.value, '刚刚')" />
            <HealthPoolList :pools="health.exitPools.value" />
          </el-collapse-item>
        </el-collapse>
      </template>
    </el-tab-pane>
  </el-tabs>
</template>

<style>
.monitor-tabs > .el-tabs__header {
  margin-bottom: 18px;
}

.monitor-tabs > .el-tabs__header .el-tabs__item {
  height: 46px;
  font-weight: 800;
}

.operations-alert,
.health-alert {
  margin-bottom: 18px;
}

.operations-hero {
  margin-bottom: 18px;
  border-radius: 22px;
}

.operations-hero .el-card__body {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 18px;
}

.operations-hero h2 {
  margin: 0;
  font-size: 30px;
  letter-spacing: -0.04em;
}

.operations-hero p {
  margin: 8px 0 0;
  color: var(--ink-soft);
}

.operations-hero__tags {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 10px;
}

.operations-metric__hint {
  margin: 8px 0 0;
  color: var(--ink-soft);
}

.operations-card {
  min-height: 100%;
  border-radius: 22px;
}

.runtime-card,
.ledger-ranking-card,
.production-alerts-card,
.operations-probe-card {
  margin-bottom: 18px;
}

.production-alerts-table {
  width: 100%;
}

.production-alerts-table strong {
  color: var(--ink);
}

.production-alerts-table__meta {
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 13px;
  line-height: 1.5;
}

.operations-card__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.runtime-intro {
  margin: 0 0 14px;
  color: var(--ink-soft);
  line-height: 1.7;
}

.runtime-table {
  width: 100%;
}

.ledger-ranking-table strong {
  color: var(--ink);
}

.ledger-rank {
  font-weight: 900;
  color: var(--accent);
}

.ledger-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.runtime-line__name {
  font-weight: 700;
  color: var(--ink);
}

.runtime-line__meta {
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 13px;
  line-height: 1.5;
}

.runtime-stats {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  margin: 0;
}

.runtime-stats > div {
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.56);
}

.runtime-stats dt {
  color: var(--ink-soft);
  font-size: 12px;
}

.runtime-stats dd {
  margin: 3px 0 0;
  font-weight: 800;
  color: var(--ink);
}

.rate-pair {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.rate-pair + .rate-pair {
  margin-top: 8px;
}

.rate-pair span {
  color: var(--ink-soft);
}

.rate-pair strong {
  font-size: 15px;
}

.probe-message {
  margin-top: 6px;
  color: #b42318;
  font-size: 13px;
  line-height: 1.5;
}

.dirty-summary {
  display: grid;
  grid-template-columns: 160px minmax(0, 1fr);
  gap: 20px;
  align-items: center;
  margin-bottom: 18px;
}

.dirty-summary span,
.operations-list span {
  color: var(--ink-soft);
}

.dirty-summary strong {
  display: block;
  margin-top: 8px;
  font-size: 42px;
  line-height: 1;
  letter-spacing: -0.04em;
}

.dirty-summary p {
  margin: 0;
  color: var(--ink-soft);
  line-height: 1.8;
}

.operations-list {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}

.operations-list > div {
  padding: 18px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.54);
}

.operations-list strong {
  display: block;
  margin-top: 10px;
  font-size: 28px;
  letter-spacing: -0.03em;
}

.settings-alert {
  margin-bottom: 14px;
}

.probe-settings {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 0 14px;
}

.probe-settings__hint {
  margin: 0 0 14px;
  color: var(--ink-soft);
  line-height: 1.7;
}

.health-board {
  margin-bottom: 18px;
  border-radius: 24px;
}

.health-board__tabs .el-tabs__header {
  margin-bottom: 18px;
}

.health-board__tabs .el-tabs__item {
  height: 44px;
  font-weight: 800;
}

.health-details {
  margin-bottom: 18px;
}

.health-details .el-collapse-item__header {
  padding: 0 18px;
  border-radius: 18px;
  font-weight: 900;
}

.health-details .el-collapse-item__content {
  padding-top: 18px;
}

@media (max-width: 900px) {
  .operations-hero .el-card__body {
    align-items: flex-start;
    flex-direction: column;
  }

  .operations-hero__tags {
    justify-content: flex-start;
  }

  .dirty-summary,
  .operations-list,
  .probe-settings,
  .runtime-stats {
    grid-template-columns: 1fr;
  }
}
</style>
