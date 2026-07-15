<!--
  本页面用于后台查询订单列表。
  它支持按状态、用户、邮箱、关键字和订单号筛选。
  支付处理暂未完整实现，页面只展示现有订单数据。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type AdminOrderQuery, type OrderInfo } from '@/services/api';

const loading = ref(true);
const orders = ref<OrderInfo[]>([]);
const total = ref(0);
const filters = reactive({
  status: '',
  keyword: '',
  orderNo: '',
  page: 1,
  pageSize: 20,
});

const paidOrders = computed(() => orders.value.filter((order) => normalizedStatus(order.status) === 'paid').length);
const pendingOrders = computed(() => orders.value.filter((order) => normalizedStatus(order.status) === 'pending').length);
const paidAmount = computed(() =>
  orders.value
    .filter((order) => normalizedStatus(order.status) === 'paid')
    .reduce((sum, order) => sum + (order.amount ?? 0), 0),
);

onMounted(load);

async function load() {
  loading.value = true;
  try {
    const data = await apiClient.getAdminOrders(adminOrderQuery());
    orders.value = data.items;
    total.value = data.total;
    filters.page = data.page;
    filters.pageSize = data.pageSize;
  } catch (error) {
    orders.value = [];
    total.value = 0;
    ElMessage.error(error instanceof Error ? error.message : '加载订单列表失败');
  } finally {
    loading.value = false;
  }
}

function adminOrderQuery(): AdminOrderQuery {
  return {
    page: filters.page,
    pageSize: filters.pageSize,
    status: filters.status || undefined,
    keyword: filters.keyword.trim() || undefined,
    orderNo: filters.orderNo.trim() || undefined,
  };
}

function search() {
  filters.page = 1;
  load();
}

function resetFilters() {
  filters.status = '';
  filters.keyword = '';
  filters.orderNo = '';
  filters.page = 1;
  load();
}

function changePage(page: number) {
  filters.page = page;
  load();
}

function changePageSize(pageSize: number) {
  filters.pageSize = pageSize;
  filters.page = 1;
  load();
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

  const currency = order.currency === 'CNY' ? '¥' : `${order.currency} `;
  return `${currency}${Number(order.amount.toFixed(2))}`;
}

function formatAmount(value: number) {
  return `¥${Number(value.toFixed(2))}`;
}

function formatDate(value: string) {
  return value || '-';
}
</script>

<template>
  <PageHeader title="订单管理" description="支付功能当前暂停；本页用于查看历史订单、占位接口和金额汇总。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
  </PageHeader>

  <el-alert
    title="支付功能暂停中：后台只保留订单查询、接口占位和后续对账入口，不发起真实收款。"
    type="info"
    show-icon
    :closable="false"
    class="order-paused-alert"
  />

  <div class="metric-grid">
    <el-card shadow="never" class="metric-card">
      <span>订单总数</span>
      <strong>{{ total }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>已支付</span>
      <strong>{{ paidOrders }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>待支付</span>
      <strong>{{ pendingOrders }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>历史已支付金额</span>
      <strong class="metric-card__text">{{ formatAmount(paidAmount) }}</strong>
    </el-card>
  </div>

  <el-card shadow="never">
    <template #header>
      <span>订单列表</span>
    </template>
    <el-form :inline="true" class="order-filter" @submit.prevent>
      <el-form-item label="状态">
        <el-select v-model="filters.status" clearable placeholder="全部状态" style="width: 140px" @change="search">
          <el-option label="待支付" value="pending" />
          <el-option label="已支付" value="paid" />
          <el-option label="已关闭" value="failed" />
        </el-select>
      </el-form-item>
      <el-form-item label="用户/邮箱">
        <el-input
          v-model="filters.keyword"
          clearable
          placeholder="输入用户邮箱或关键词"
          style="width: 220px"
          @keyup.enter="search"
          @clear="search"
        />
      </el-form-item>
      <el-form-item label="订单号">
        <el-input
          v-model="filters.orderNo"
          clearable
          placeholder="输入订单号"
          style="width: 220px"
          @keyup.enter="search"
          @clear="search"
        />
      </el-form-item>
      <el-form-item>
        <el-button type="primary" :loading="loading" @click="search">筛选</el-button>
        <el-button @click="resetFilters">重置</el-button>
      </el-form-item>
    </el-form>
    <el-table v-loading="loading" :data="orders" row-key="id" stripe>
      <el-table-column prop="orderNo" label="订单号" min-width="180" />
      <el-table-column prop="account" label="用户" min-width="180" />
      <el-table-column prop="planName" label="套餐" min-width="140" />
      <el-table-column label="金额" min-width="120">
        <template #default="{ row }: { row: OrderInfo }">
          {{ amountLabel(row) }}
        </template>
      </el-table-column>
      <el-table-column label="状态" min-width="120">
        <template #default="{ row }: { row: OrderInfo }">
          <el-tag :type="statusType(row.status)" effect="plain">{{ statusLabel(row.status) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="有效期" min-width="110">
        <template #default="{ row }: { row: OrderInfo }">
          {{ row.durationDays ? `${row.durationDays} 天` : '-' }}
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
    <el-pagination
      v-model:current-page="filters.page"
      v-model:page-size="filters.pageSize"
      class="order-pagination"
      :total="total"
      :page-sizes="[10, 20, 50, 100, 200]"
      layout="total, sizes, prev, pager, next, jumper"
      @current-change="changePage"
      @size-change="changePageSize"
    />
  </el-card>
</template>

<style scoped>
.order-filter {
  margin-bottom: 16px;
}

.order-paused-alert {
  margin-bottom: 16px;
}

.order-pagination {
  margin-top: 16px;
  justify-content: flex-end;
}
</style>
