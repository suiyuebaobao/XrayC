<!--
  本页面用于普通用户查看自己的订单记录。
  它展示订单编号、套餐、金额、状态和支付地址。
  页面只读取当前登录用户的数据。
-->
<script setup lang="ts">
import { DocumentCopy, Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type OrderInfo } from '@/services/api';

const loading = ref(true);
const orders = ref<OrderInfo[]>([]);

const paidOrders = computed(() => orders.value.filter((order) => normalizedStatus(order.status) === 'paid').length);
const pendingOrders = computed(() => orders.value.filter((order) => normalizedStatus(order.status) === 'pending').length);

onMounted(load);

async function load() {
  loading.value = true;
  try {
    orders.value = await apiClient.getUserOrders();
  } catch (error) {
    orders.value = [];
    ElMessage.error(errorText(error, '加载订单记录失败'));
  } finally {
    loading.value = false;
  }
}

function normalizedStatus(status: string) {
  const value = status.toLowerCase();
  if (value === 'paid' || value === 'success' || value === 'completed') {
    return 'paid';
  }

  if (value === 'pending' || value === 'created' || value === 'unpaid') {
    return 'pending';
  }

  if (value === 'cancelled' || value === 'canceled' || value === 'failed') {
    return 'failed';
  }

  return value || 'unknown';
}

function statusLabel(status: string) {
  const value = normalizedStatus(status);
  if (value === 'paid') {
    return '已支付';
  }

  if (value === 'pending') {
    return '待支付';
  }

  if (value === 'failed') {
    return '已关闭';
  }

  return status || '未知';
}

function statusType(status: string) {
  const value = normalizedStatus(status);
  if (value === 'paid') {
    return 'success';
  }

  if (value === 'pending') {
    return 'warning';
  }

  return value === 'failed' ? 'info' : 'primary';
}

function amountLabel(order: OrderInfo) {
  if (order.amount === null) {
    return '-';
  }

  return Number(order.amount.toFixed(2)).toString();
}

function currencyLabel(order: OrderInfo) {
  return order.currency || '-';
}

function formatDate(value: string) {
  return value || '-';
}

async function copyPaymentAddress(address: string) {
  if (!address) {
    return;
  }

  try {
    await navigator.clipboard.writeText(address);
    ElMessage.success('收款地址已复制');
  } catch {
    ElMessage.error('复制收款地址失败');
  }
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}
</script>

<template>
  <PageHeader title="我的订单" description="查看当前账号历史订单。支付功能当前暂停，接口保留待后续启用。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
  </PageHeader>

  <el-row :gutter="18">
    <el-col :xs="24" :lg="8">
      <el-card shadow="never">
        <template #header>支付功能暂停</template>
        <el-alert
          type="info"
          :closable="false"
          show-icon
          title="当前版本先暂停在线支付和订单创建，只保留订单查询与支付接口占位。"
        />
      </el-card>
    </el-col>

    <el-col :xs="24" :lg="16">
      <div class="metric-grid order-metrics">
        <el-card shadow="never" class="metric-card">
          <span>订单总数</span>
          <strong>{{ orders.length }}</strong>
        </el-card>
        <el-card shadow="never" class="metric-card">
          <span>已支付</span>
          <strong>{{ paidOrders }}</strong>
        </el-card>
        <el-card shadow="never" class="metric-card">
          <span>待支付</span>
          <strong>{{ pendingOrders }}</strong>
        </el-card>
      </div>
    </el-col>
  </el-row>

  <el-card shadow="never" class="section-row">
    <template #header>订单记录</template>
    <el-table v-loading="loading" :data="orders" row-key="id" stripe>
      <el-table-column prop="orderNo" label="订单号" min-width="180" />
      <el-table-column prop="planName" label="套餐" min-width="140" />
      <el-table-column label="金额" min-width="120">
        <template #default="{ row }: { row: OrderInfo }">
          {{ amountLabel(row) }}
        </template>
      </el-table-column>
      <el-table-column label="币种" min-width="90">
        <template #default="{ row }: { row: OrderInfo }">
          {{ currencyLabel(row) }}
        </template>
      </el-table-column>
      <el-table-column label="收款地址" min-width="300">
        <template #default="{ row }: { row: OrderInfo }">
          <el-space v-if="row.paymentAddress" wrap>
            <code class="address-text">{{ row.paymentAddress }}</code>
            <el-button text size="small" :icon="DocumentCopy" @click="copyPaymentAddress(row.paymentAddress)">
              复制
            </el-button>
          </el-space>
          <span v-else>-</span>
        </template>
      </el-table-column>
      <el-table-column label="状态" min-width="120">
        <template #default="{ row }: { row: OrderInfo }">
          <el-tag :type="statusType(row.status)" effect="plain">{{ statusLabel(row.status) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="套餐有效期" min-width="110">
        <template #default="{ row }: { row: OrderInfo }">
          {{ row.durationDays ? `${row.durationDays} 天` : '-' }}
        </template>
      </el-table-column>
      <el-table-column label="订单过期时间" min-width="180">
        <template #default="{ row }: { row: OrderInfo }">
          {{ formatDate(row.expiresAt) }}
        </template>
      </el-table-column>
      <el-table-column label="创建时间" min-width="180">
        <template #default="{ row }: { row: OrderInfo }">
          {{ formatDate(row.createdAt) }}
        </template>
      </el-table-column>
      <el-table-column label="支付时间" min-width="180">
        <template #default="{ row }: { row: OrderInfo }">
          {{ formatDate(row.paidAt) }}
        </template>
      </el-table-column>
      <template #empty>
        <el-empty description="暂无订单数据" />
      </template>
    </el-table>
  </el-card>
</template>

<style scoped>
.form-field {
  width: 100%;
}

.field-suffix {
  margin-left: 8px;
  color: var(--ink-soft);
}

.payment-card {
  margin-top: 18px;
}

.payment-descriptions {
  margin-top: 14px;
}

.address-text {
  word-break: break-all;
}

.order-metrics {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}
</style>
