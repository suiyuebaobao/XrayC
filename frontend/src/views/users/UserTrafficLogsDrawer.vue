<!--
  本组件展示管理员用户级流量日志。
  数据只按当前用户加载，不混入全局运营排行。
  管理员页面按真实字段展示访问 IP、线路和出口信息。
  组件自身不拼接订阅链接、不保存 token、不做敏感日志输出。
-->
<script setup lang="ts">
import { Refresh, Search } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, reactive, ref, watch } from 'vue';
import { apiClient, type AdminUser, type AdminUserTrafficLog } from '@/services/api';
import UserTrafficLogsTable from '@/views/users/UserTrafficLogsTable.vue';

const props = defineProps<{
  modelValue: boolean;
  user?: AdminUser;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: boolean];
}>();

const loading = ref(false);
const logs = ref<AdminUserTrafficLog[]>([]);
const total = ref(0);
const filters = reactive({
  page: 1,
  pageSize: 20,
  range: [] as string[],
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
      void loadLogs();
    }
  },
);

async function loadLogs() {
  if (!props.user?.id) {
    logs.value = [];
    total.value = 0;
    return;
  }

  loading.value = true;
  try {
    const page = await apiClient.getAdminUserTrafficLogs(props.user.id, {
      page: filters.page,
      pageSize: filters.pageSize,
      from: filters.range[0],
      to: filters.range[1],
    });
    logs.value = page.items;
    total.value = page.total;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载用户流量日志失败');
  } finally {
    loading.value = false;
  }
}

function handleSearch() {
  filters.page = 1;
  void loadLogs();
}

</script>

<template>
  <el-drawer v-model="visible" :title="`用户流量日志：${user?.account || user?.email || ''}`" size="72%">
    <div class="traffic-log-toolbar">
      <div>
        <strong>{{ user?.email || '-' }}</strong>
        <p class="muted">只显示该用户保留期内的账本流水、访问 IP、入口和出口端点；更早数据保留为汇总统计。</p>
      </div>
      <el-space wrap>
        <el-date-picker
          v-model="filters.range"
          type="datetimerange"
          value-format="YYYY-MM-DDTHH:mm:ss[Z]"
          start-placeholder="开始时间"
          end-placeholder="结束时间"
          range-separator="至"
        />
        <el-button :icon="Search" type="primary" plain @click="handleSearch">查询</el-button>
        <el-button :icon="Refresh" :loading="loading" @click="loadLogs">刷新</el-button>
      </el-space>
    </div>

    <UserTrafficLogsTable :logs="logs" :loading="loading" />

    <el-pagination
      class="traffic-log-pagination"
      background
      layout="total, sizes, prev, pager, next"
      :total="total"
      :page-size="filters.pageSize"
      :current-page="filters.page"
      :page-sizes="[20, 50, 100, 200]"
      @update:current-page="(page: number) => { filters.page = page; void loadLogs(); }"
      @update:page-size="(size: number) => { filters.pageSize = size; filters.page = 1; void loadLogs(); }"
    />
  </el-drawer>
</template>

<style scoped>
.traffic-log-toolbar {
  align-items: flex-start;
  display: flex;
  gap: 16px;
  justify-content: space-between;
  margin-bottom: 16px;
}

.traffic-log-pagination {
  justify-content: flex-end;
  margin-top: 16px;
}
</style>
