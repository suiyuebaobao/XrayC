<!-- 用户侧栏与全站流量页共用查询、日期转换、分页和错误处理。只读取管理员授权的数据。 -->
<script setup lang="ts">
import { Refresh, Search } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { onBeforeUnmount, reactive, ref, watch } from 'vue';
import { apiClient, type AdminUserTrafficLog } from '@/services/api';
import UserTrafficLogsTable from './UserTrafficLogsTable.vue';

const props = withDefaults(defineProps<{ userId?: string | number; active?: boolean; emptyText?: string }>(), {
  active: true, emptyText: '请选择用户或等待该用户产生流量日志',
});
const loading = ref(false);
const logs = ref<AdminUserTrafficLog[]>([]);
const total = ref(0);
const filters = reactive({ page: 1, pageSize: 20, range: null as Date[] | null });
let requestGeneration = 0;

watch(() => [props.userId, props.active], () => {
  filters.page = 1;
  void loadLogs();
}, { immediate: true });
onBeforeUnmount(() => { requestGeneration += 1; });

async function loadLogs() {
  const generation = ++requestGeneration;
  if (!props.active || !props.userId) {
    logs.value = []; total.value = 0; loading.value = false;
    return;
  }
  loading.value = true;
  try {
    const result = await apiClient.getAdminUserTrafficLogs(props.userId, {
      page: filters.page, pageSize: filters.pageSize,
      // 日期选择器提供本地 Date；转换成真正的 UTC，不能直接给本地文本附加 Z。
      from: filters.range?.[0]?.toISOString(), to: filters.range?.[1]?.toISOString(),
    });
    if (generation !== requestGeneration) return;
    logs.value = result.items; total.value = result.total;
  } catch (error) {
    if (generation !== requestGeneration) return;
    logs.value = []; total.value = 0;
    ElMessage.error(error instanceof Error ? error.message : '加载用户流量日志失败');
  } finally {
    if (generation === requestGeneration) loading.value = false;
  }
}
function searchLogs() { filters.page = 1; void loadLogs(); }
defineExpose({ reload: loadLogs, loading });
</script>

<template>
  <div class="traffic-query">
    <div class="traffic-query__toolbar">
      <el-date-picker v-model="filters.range" type="datetimerange" start-placeholder="开始时间" end-placeholder="结束时间" range-separator="至" />
      <el-button :icon="Search" type="primary" plain :disabled="!userId" @click="searchLogs">查询</el-button>
      <el-button :icon="Refresh" :loading="loading" :disabled="!userId" @click="loadLogs">刷新</el-button>
    </div>
    <UserTrafficLogsTable :logs="logs" :loading="loading" :empty-text="emptyText" />
    <el-pagination class="traffic-log-pagination" background layout="total, sizes, prev, pager, next" :total="total" :page-size="filters.pageSize" :current-page="filters.page" :page-sizes="[20, 50, 100, 200]"
      @update:current-page="(page: number) => { filters.page = page; void loadLogs(); }"
      @update:page-size="(size: number) => { filters.pageSize = size; filters.page = 1; void loadLogs(); }" />
  </div>
</template>

<style scoped>
.traffic-query__toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; margin-bottom: 16px; }
.traffic-query__toolbar :deep(.el-date-editor) { max-width: 100%; flex-grow: 1; }
.traffic-log-pagination { justify-content: flex-end; margin-top: 16px; overflow-x: auto; }
</style>
