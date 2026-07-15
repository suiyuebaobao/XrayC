<!--
  本组件展示管理员用户级设备/IP 观察列表。
  设备管理只合并订阅拉取 IP 和真实节点使用 IP。
  组件不做设备绑定、不生成设备 token、不拼接订阅链接。
  数据按当前用户分页加载，去重逻辑由后端完成。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, reactive, ref, watch } from 'vue';
import { apiClient, type AdminUser, type AdminUserDeviceIp } from '@/services/api';

const props = defineProps<{
  modelValue: boolean;
  user?: AdminUser;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: boolean];
}>();

const loading = ref(false);
const devices = ref<AdminUserDeviceIp[]>([]);
const total = ref(0);
const filters = reactive({
  page: 1,
  pageSize: 20,
});

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value),
});

watch(
  () => [props.modelValue, props.user?.id],
  () => {
    if (props.modelValue && props.user?.id) {
      filters.page = 1;
      void loadDevices();
    }
  },
);

async function loadDevices() {
  if (!props.user?.id) {
    devices.value = [];
    total.value = 0;
    return;
  }

  loading.value = true;
  try {
    const page = await apiClient.getAdminUserDevices(props.user.id, {
      page: filters.page,
      pageSize: filters.pageSize,
    });
    devices.value = page.items;
    total.value = page.total;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载用户设备管理失败');
  } finally {
    loading.value = false;
  }
}

function sourceLabel(source: AdminUserDeviceIp['source']) {
  if (source === 'both') {
    return '订阅拉取 + 节点使用';
  }
  if (source === 'subscription_pull') {
    return '订阅拉取';
  }
  if (source === 'node_use') {
    return '节点使用';
  }
  return '未知';
}

function sourceTagType(source: AdminUserDeviceIp['source']) {
  if (source === 'both') {
    return 'success';
  }
  if (source === 'subscription_pull') {
    return 'warning';
  }
  if (source === 'node_use') {
    return 'primary';
  }
  return 'info';
}

function ipLabel(row: AdminUserDeviceIp) {
  return row.clientIp || row.clientIpHash || '-';
}

function lineLabel(row: AdminUserDeviceIp) {
  const line = row.lastAccessLine;
  if (!line.name && !line.id) {
    return '-';
  }
  return `${line.name || line.id} / ${line.protocol || '-'} / ${line.transport || '-'}`;
}

function nodeLabel(row: AdminUserDeviceIp) {
  const node = row.lastAccessNode;
  if (!node.name && !node.id) {
    return '-';
  }
  return `${node.name || node.id} / ${node.publicHost || '-'}`;
}

function statusLabel(row: AdminUserDeviceIp) {
  return `${row.lastSessionStatus || 'unknown'} / 连接 ${row.lastActiveConnectionCount || 0}`;
}

function formatDate(value: string) {
  return value || '-';
}
</script>

<template>
  <el-drawer v-model="visible" :title="`设备管理：${user?.account || user?.email || ''}`" size="68%">
    <div class="device-toolbar">
      <div>
        <strong>{{ user?.email || '-' }}</strong>
        <p class="muted">合并展示订阅拉取 IP 和真实节点使用 IP；重复 IP 只显示一条。</p>
      </div>
      <el-button :icon="Refresh" :loading="loading" @click="loadDevices">刷新</el-button>
    </div>

    <el-table v-loading="loading" :data="devices" row-key="clientIpHash" stripe>
      <el-table-column label="IP" min-width="190">
        <template #default="{ row }: { row: AdminUserDeviceIp }">
          <strong>{{ ipLabel(row) }}</strong>
          <p v-if="row.clientIp" class="muted">{{ row.clientIpHash || '-' }}</p>
        </template>
      </el-table-column>
      <el-table-column label="来源" min-width="150">
        <template #default="{ row }: { row: AdminUserDeviceIp }">
          <el-tag :type="sourceTagType(row.source)" effect="plain">{{ sourceLabel(row.source) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="首次出现" min-width="170">
        <template #default="{ row }: { row: AdminUserDeviceIp }">{{ formatDate(row.firstSeenAt) }}</template>
      </el-table-column>
      <el-table-column label="最近出现" min-width="170">
        <template #default="{ row }: { row: AdminUserDeviceIp }">{{ formatDate(row.lastSeenAt) }}</template>
      </el-table-column>
      <el-table-column label="次数" min-width="150">
        <template #default="{ row }: { row: AdminUserDeviceIp }">
          订阅 {{ row.subscriptionPullCount }}
          <p class="muted">使用 {{ row.nodeUseCount }}</p>
        </template>
      </el-table-column>
      <el-table-column label="最近入口" min-width="260">
        <template #default="{ row }: { row: AdminUserDeviceIp }">
          {{ lineLabel(row) }}
          <p class="muted">{{ nodeLabel(row) }}</p>
        </template>
      </el-table-column>
      <el-table-column label="最近状态" min-width="150">
        <template #default="{ row }: { row: AdminUserDeviceIp }">{{ statusLabel(row) }}</template>
      </el-table-column>
      <template #empty>
        <el-empty description="暂无该用户的设备/IP 记录" />
      </template>
    </el-table>

    <el-pagination
      class="device-pagination"
      background
      layout="total, sizes, prev, pager, next"
      :total="total"
      :page-size="filters.pageSize"
      :current-page="filters.page"
      :page-sizes="[20, 50, 100, 200]"
      @update:current-page="(page: number) => { filters.page = page; void loadDevices(); }"
      @update:page-size="(size: number) => { filters.pageSize = size; filters.page = 1; void loadDevices(); }"
    />
  </el-drawer>
</template>

<style scoped>
.device-toolbar {
  align-items: flex-start;
  display: flex;
  gap: 16px;
  justify-content: space-between;
  margin-bottom: 16px;
}

.device-pagination {
  justify-content: flex-end;
  margin-top: 16px;
}
</style>
