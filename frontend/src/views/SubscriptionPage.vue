<!--
  本页面用于普通用户查看订阅链接、套餐状态和流量用量。
  它支持复制订阅地址、重置订阅 token 和查看授权线路。
  页面不得展示第三方出口真实地址或上游凭据。
-->
<script setup lang="ts">
import { DocumentCopy, RefreshRight, Refresh } from '@element-plus/icons-vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import { usePortalFeaturesStore } from '@/stores/portalFeatures';
import { useSessionStore } from '@/stores/session';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type SubscriptionInfo, type UserUsageSummary, type OrderInfo } from '@/services/api';

const portal = usePortalFeaturesStore();
const session = useSessionStore();
const orders = ref<OrderInfo[]>([]);
const ordersError = ref('');
const loading = ref(true);
const resetting = ref(false);
const subscription = ref<SubscriptionInfo>();
const usage = ref<UserUsageSummary>();
const usageError = ref('');

const totalTraffic = computed(() => ({
  used: subscription.value?.trafficGb.used ?? 0,
  total: subscription.value?.trafficGb.total ?? 0,
}));
const totalPercent = computed(() => percent(totalTraffic.value.used, totalTraffic.value.total));
const usageCards = computed(() => [
  {
    label: '累计真实流量',
    value: formatBytes(usage.value?.realBytes),
    hint: `${formatGb(usage.value?.realGb)} GB`,
  },
  {
    label: '累计扣费流量',
    value: formatBytes(usage.value?.billedBytes),
    hint: `${formatGb(usage.value?.billedGb)} GB`,
  },
  {
    label: '套餐总流量已用',
    value: `${formatGb(totalTraffic.value.used)} GB`,
    hint: totalTraffic.value.total === -1 ? '不限流量' : `配额 ${formatGb(totalTraffic.value.total)} GB`,
  },
]);

onMounted(async () => { await portal.load(); await loadSubscription(); });

async function loadSubscription() {
  loading.value = true;
  usageError.value = '';
  ordersError.value = '';
  try {
    const [subscriptionResult, usageResult, ordersResult] = await Promise.allSettled([
      apiClient.getSubscription(),
      apiClient.getUserUsage(),
      portal.features.orders ? apiClient.getUserOrders() : Promise.resolve([]),
    ]);

    if (subscriptionResult.status === 'fulfilled') {
      subscription.value = subscriptionResult.value;
    } else {
      ElMessage.error(errorText(subscriptionResult.reason, '加载订阅信息失败'));
    }

    if (ordersResult.status === 'fulfilled') orders.value = ordersResult.value;
    else { orders.value = []; ordersError.value = '订单暂时无法加载'; }

    if (usageResult.status === 'fulfilled') {
      usage.value = usageResult.value;
    } else {
      usage.value = undefined;
      usageError.value = errorText(usageResult.reason, '加载用量统计失败');
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载订阅信息失败');
  } finally {
    loading.value = false;
  }
}

async function copyUrl() {
  if (!subscription.value) {
    return;
  }

  await navigator.clipboard.writeText(subscription.value.subscriptionUrl);
  ElMessage.success('订阅链接已复制');
}

async function resetToken() {
  if (!subscription.value) {
    return;
  }

  try {
    await ElMessageBox.confirm('重置后旧订阅链接会立即失效，是否继续？', '重置订阅链接', {
      type: 'warning',
      confirmButtonText: '重置',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  resetting.value = true;
  try {
    subscription.value = await apiClient.resetSubscriptionToken();
    ElMessage.success('订阅链接已重置');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '重置订阅链接失败');
  } finally {
    resetting.value = false;
  }
}

function percent(used = 0, total = 1) {
  if (!Number.isFinite(total) || total <= 0) {
    return 0;
  }

  return Math.min(100, Math.round((used / total) * 100));
}

function formatGb(value: number | null | undefined) {
  if (!Number.isFinite(value)) {
    return '0.00';
  }

  return Number(value).toFixed(2);
}

function formatBytes(value: number | null | undefined) {
  if (!Number.isFinite(value) || value === undefined || value === null) {
    return '0 B';
  }

  if (value < 1024) {
    return `${Math.round(value)} B`;
  }

  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let scaled = value;
  let unitIndex = -1;
  do {
    scaled /= 1024;
    unitIndex += 1;
  } while (scaled >= 1024 && unitIndex < units.length - 1);

  return `${scaled.toFixed(scaled >= 10 ? 1 : 2)} ${units[unitIndex]}`;
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}
</script>

<template>
  <PageHeader title="我的服务" :description="`${session.user?.name || session.user?.account || '欢迎回来'}，在这里查看套餐、用量及可用节点。`">
    <el-button :icon="Refresh" :loading="loading" @click="loadSubscription">刷新</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="8" animated />
  <template v-else-if="subscription">
    <el-row :gutter="18">
      <el-col :xs="24" :lg="9">
        <el-card shadow="never" class="subscription-card">
          <template #header>
            <span>当前套餐</span> · <strong>{{ subscription.planName }}</strong>
          </template>
          <p>到期时间：{{ subscription.expiresAt }}</p>
          <el-divider />
          <span>总流量</span>
          <el-progress :percentage="totalPercent" />
          <p>{{ totalTraffic.total === -1 ? `${totalTraffic.used} GB / 不限流量` : `${totalTraffic.used} / ${totalTraffic.total} GB` }}</p>
          <p>剩余流量：{{ totalTraffic.total === -1 ? '不限流量' : `${Math.max(0, totalTraffic.total - totalTraffic.used).toFixed(2)} GB` }}</p>
        </el-card>
      </el-col>

      <el-col :xs="24" :lg="15">
        <el-card shadow="never">
          <template #header>订阅链接</template>
          <el-input :model-value="subscription.subscriptionUrl" readonly size="large">
            <template #append>
              <el-button :icon="DocumentCopy" @click="copyUrl">复制</el-button>
            </template>
          </el-input>
          <div class="subscription-actions">
            <el-button :icon="RefreshRight" :loading="resetting" type="warning" plain @click="resetToken">
              重置订阅链接
            </el-button>
          </div>
          <p class="muted">
            复制订阅链接后，在 Clash 或 mihomo 兼容客户端导入即可使用。
          </p>
        </el-card>
      </el-col>
    </el-row>

    <el-card shadow="never" class="section-row">
      <template #header><span>可用节点</span> <el-tag size="small" effect="plain">{{ subscription.visibleLines.length }} 条</el-tag></template>
      <el-table :data="subscription.visibleLines" stripe>
        <el-table-column label="名称" min-width="160">
          <template #default="{ row }">
            <strong>{{ row.name }}</strong>
            <small v-if="row.lineGroupName" class="node-group-meta">
              {{ row.lineGroupName }}
            </small>
          </template>
        </el-table-column>
        <el-table-column prop="region" label="地区" min-width="120" />
        <el-table-column label="客户端连接地址" min-width="200">
          <template #default="{ row }">
            {{ row.server }}:{{ row.port }}
          </template>
        </el-table-column>
        <el-table-column prop="protocol" label="协议" min-width="150" />
      </el-table>
    </el-card>

    <el-card shadow="never" class="section-row usage-history-card">
      <template #header>
        <div class="usage-history-card__header">
          <span>订阅用量统计</span>
          <el-tag effect="plain">累计用量</el-tag>
        </div>
      </template>

      <p class="muted">
        真实流量是实际传输用量；扣费流量按套餐及分组倍率计算。
      </p>

      <el-alert
        v-if="usageError"
        :title="usageError"
        type="warning"
        show-icon
        :closable="false"
      />
      <div v-else class="usage-metric-grid">
        <article v-for="card in usageCards" :key="card.label" class="usage-metric">
          <span>{{ card.label }}</span>
          <strong>{{ card.value }}</strong>
          <p>{{ card.hint }}</p>
        </article>
      </div>


    </el-card>
    <el-card v-if="portal.features.plans || portal.features.orders || portal.features.redeem || portal.features.invites" shadow="never" class="section-row">
      <template #header>账户服务</template>
      <el-space wrap>
        <RouterLink v-if="portal.features.plans" to="/plans"><el-button>选择套餐</el-button></RouterLink>
        <RouterLink v-if="portal.features.orders" to="/orders"><el-button>我的订单</el-button></RouterLink>
        <RouterLink v-if="portal.features.redeem" to="/redeem"><el-button>兑换套餐</el-button></RouterLink>
        <RouterLink v-if="portal.features.invites" to="/invite-codes"><el-button>邀请注册</el-button></RouterLink>
      </el-space>
      <p v-if="portal.features.orders && orders[0]" class="muted">最近订单：{{ orders[0].planName || orders[0].orderNo }} · {{ orders[0].status }} · {{ orders[0].createdAt }}</p>
      <p v-else-if="ordersError" class="muted">{{ ordersError }}</p>
    </el-card>
  </template>
</template>

<style scoped>
.subscription-actions {
  margin-top: 12px;
}

.node-group-meta {
  display: block;
  margin-top: 4px;
  color: var(--ink-soft);
  font-size: 12px;
  font-weight: 500;
}

.usage-history-card {
  overflow: hidden;
  background: var(--surface);
}

.usage-history-card__header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.usage-metric-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
  margin: 16px 0 18px;
}

.usage-metric {
  min-height: 132px;
  padding: 18px;
  border: 1px solid rgba(15, 118, 110, 0.16);
  border-radius: 22px;
  background:
    linear-gradient(160deg, rgba(255, 255, 255, 0.82), rgba(15, 118, 110, 0.08)),
    rgba(255, 255, 255, 0.58);
}

.usage-metric span,
.usage-metric p {
  color: var(--ink-soft);
}

.usage-metric strong {
  display: block;
  margin: 12px 0 8px;
  color: var(--accent-dark);
  font-size: clamp(24px, 3vw, 36px);
  line-height: 1;
  letter-spacing: -0.05em;
}

.usage-metric p {
  margin: 0;
}

.usage-history-table {
  margin-top: 8px;
}

.usage-empty {
  padding-top: 8px;
}

@media (max-width: 900px) {
  .usage-metric-grid {
    grid-template-columns: 1fr;
  }
}
</style>
