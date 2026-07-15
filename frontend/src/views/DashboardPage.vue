<!--
  本页面用于普通用户进入后台后的仪表盘。
  它展示当前订阅、最近订单和常用操作入口。
  页面只读取用户自己的数据，不访问管理员接口。
-->
<script setup lang="ts">
import { Refresh, Tickets } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type OrderInfo, type SubscriptionInfo } from '@/services/api';
import { useSessionStore } from '@/stores/session';

const session = useSessionStore();
const loading = ref(true);
const subscription = ref<SubscriptionInfo>();
const orders = ref<OrderInfo[]>([]);

const totalRemaining = computed(() => remaining(
  subscription.value?.trafficGb.used ?? 0,
  subscription.value?.trafficGb.total ?? 0,
));
const latestOrder = computed(() => orders.value[0]);
const visibleNodeCount = computed(() => {
  const lines = subscription.value?.visibleLines ?? [];
  const groupIds = new Set(lines.map((line) => line.lineGroupId || line.name).filter(Boolean));
  return groupIds.size || lines.length;
});

onMounted(loadDashboard);

async function loadDashboard() {
  loading.value = true;
  try {
    const [subscriptionResult, ordersResult] = await Promise.allSettled([
      apiClient.getSubscription(),
      apiClient.getUserOrders(),
    ]);

    if (subscriptionResult.status === 'fulfilled') {
      subscription.value = subscriptionResult.value;
    } else {
      ElMessage.error(errorText(subscriptionResult.reason, '加载订阅失败'));
    }

    if (ordersResult.status === 'fulfilled') {
      orders.value = ordersResult.value;
    }
  } finally {
    loading.value = false;
  }
}

function remaining(used = 0, total = 0) {
  return Math.max(0, Number((total - used).toFixed(2)));
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}
</script>

<template>
  <PageHeader title="首页" description="查看当前账号的套餐、订阅和用量概览。">
    <el-button :icon="Refresh" :loading="loading" @click="loadDashboard">刷新</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="8" animated />
  <template v-else>
    <section class="dashboard-hero">
      <div>
        <p class="eyebrow">User dashboard</p>
        <h2>{{ session.user?.name || session.user?.account }}</h2>
        <p>普通用户登录后默认进入这里，可继续查看套餐、复制订阅或提交兑换码。</p>
      </div>
      <RouterLink to="/subscription">
        <el-button type="primary" size="large" :icon="Tickets">查看我的订阅</el-button>
      </RouterLink>
    </section>

    <div class="metric-grid">
      <el-card shadow="never" class="metric-card">
        <span>当前套餐</span>
        <strong class="metric-card__text">{{ subscription?.planName || '未开通' }}</strong>
      </el-card>
      <el-card shadow="never" class="metric-card">
        <span>剩余流量</span>
        <strong>{{ totalRemaining }} GB</strong>
      </el-card>
      <el-card shadow="never" class="metric-card">
        <span>可用节点</span>
        <strong>{{ visibleNodeCount }}</strong>
      </el-card>
    </div>

    <el-row :gutter="18">
      <el-col :xs="24" :lg="14">
        <el-card shadow="never">
          <template #header>快速入口</template>
          <div class="quick-grid">
            <RouterLink to="/plans">选择套餐</RouterLink>
            <RouterLink to="/subscription">复制订阅</RouterLink>
            <RouterLink to="/orders">我的订单</RouterLink>
            <RouterLink to="/redeem">兑换套餐</RouterLink>
          </div>
        </el-card>
      </el-col>
      <el-col :xs="24" :lg="10">
        <el-card shadow="never">
          <template #header>最近订单</template>
          <template v-if="latestOrder">
            <strong>{{ latestOrder.planName || latestOrder.orderNo }}</strong>
            <p class="muted">{{ latestOrder.status }} · {{ latestOrder.createdAt || '暂无时间' }}</p>
          </template>
          <el-empty v-else description="暂无订单数据" />
        </el-card>
      </el-col>
    </el-row>
  </template>
</template>

<style scoped>
.dashboard-hero {
  display: flex;
  justify-content: space-between;
  gap: 18px;
  align-items: center;
  margin-bottom: 18px;
  padding: 28px;
  border: 1px solid var(--border);
  border-radius: 28px;
  background:
    radial-gradient(circle at 86% 20%, rgba(15, 118, 110, 0.18), transparent 18rem),
    rgba(255, 255, 255, 0.72);
}

.dashboard-hero h2 {
  margin: 0;
  color: var(--accent-dark);
  font-size: clamp(32px, 5vw, 56px);
  letter-spacing: -0.05em;
}

.dashboard-hero p:not(.eyebrow) {
  color: var(--ink-soft);
}

.quick-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

.quick-grid a {
  padding: 18px;
  border: 1px solid var(--border);
  border-radius: 18px;
  color: var(--accent-dark);
  font-weight: 800;
  background: rgba(255, 255, 255, 0.64);
}

@media (max-width: 900px) {
  .dashboard-hero {
    display: grid;
  }

  .quick-grid {
    grid-template-columns: 1fr;
  }
}
</style>
