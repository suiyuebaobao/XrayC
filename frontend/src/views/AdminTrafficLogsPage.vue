<!--
  本页面是管理员后台独立流量日志入口。
  管理员先选择用户，再查看该用户级别的访问 IP 和流量明细。
  页面只调用管理员 API，不向普通用户页面暴露明文访问 IP。
  表格渲染复用用户日志组件，避免两个后台入口展示不一致。
-->
<script setup lang="ts">
import { Refresh, Search } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type AdminUser, type AdminUserTrafficLog } from '@/services/api';
import UserTrafficLogsTable from '@/views/users/UserTrafficLogsTable.vue';

const route = useRoute();
const router = useRouter();
const loadingUsers = ref(true);
const loadingLogs = ref(false);
const users = ref<AdminUser[]>([]);
const logs = ref<AdminUserTrafficLog[]>([]);
const selectedUserId = ref('');
const total = ref(0);
const filters = reactive({
  keyword: '',
  page: 1,
  pageSize: 20,
  range: [] as string[],
});

const selectedUser = computed(() => users.value.find((user) => String(user.id) === selectedUserId.value));
const filteredUsers = computed(() => {
  const keyword = filters.keyword.trim().toLowerCase();
  if (!keyword) {
    return users.value;
  }
  return users.value.filter((user) => (
    user.email.toLowerCase().includes(keyword)
    || user.account.toLowerCase().includes(keyword)
    || user.name.toLowerCase().includes(keyword)
  ));
});

onMounted(loadUsers);

watch(
  () => route.query.user_id,
  (value) => {
    const next = typeof value === 'string' ? value : '';
    if (next && next !== selectedUserId.value) {
      selectedUserId.value = next;
      filters.page = 1;
      void loadLogs();
    }
  },
);

async function loadUsers() {
  loadingUsers.value = true;
  try {
    users.value = await apiClient.getAdminUsers();
    selectedUserId.value = initialUserId();
    if (selectedUserId.value) {
      await loadLogs();
    }
  } catch (error) {
    users.value = [];
    ElMessage.error(error instanceof Error ? error.message : '加载用户列表失败');
  } finally {
    loadingUsers.value = false;
  }
}

function initialUserId() {
  const routeUserId = typeof route.query.user_id === 'string' ? route.query.user_id : '';
  if (routeUserId && users.value.some((user) => String(user.id) === routeUserId)) {
    return routeUserId;
  }
  return users.value[0] ? String(users.value[0].id) : '';
}

async function selectUser(user: AdminUser) {
  selectedUserId.value = String(user.id);
  filters.page = 1;
  await router.replace({ path: '/admin/traffic-logs', query: { user_id: selectedUserId.value } });
  await loadLogs();
}

async function loadLogs() {
  if (!selectedUserId.value) {
    logs.value = [];
    total.value = 0;
    return;
  }

  loadingLogs.value = true;
  try {
    const page = await apiClient.getAdminUserTrafficLogs(selectedUserId.value, {
      page: filters.page,
      pageSize: filters.pageSize,
      from: filters.range[0],
      to: filters.range[1],
    });
    logs.value = page.items;
    total.value = page.total;
  } catch (error) {
    logs.value = [];
    total.value = 0;
    ElMessage.error(error instanceof Error ? error.message : '加载用户流量日志失败');
  } finally {
    loadingLogs.value = false;
  }
}

function searchLogs() {
  filters.page = 1;
  void loadLogs();
}
</script>

<template>
  <PageHeader title="用户流量日志" description="管理员后台按用户查看访问 IP、入口、出口端点、真实流量和扣费流量；超过运营设置保留期的详细日志会汇总后清理。">
    <el-button :icon="Refresh" :loading="loadingUsers || loadingLogs" @click="loadUsers">刷新</el-button>
  </PageHeader>

  <div class="traffic-layout">
    <el-card shadow="never" class="user-card">
      <template #header>
        <div class="card-header">
          <span>选择用户</span>
          <el-tag type="info" effect="plain">{{ users.length }} 个</el-tag>
        </div>
      </template>
      <el-input v-model="filters.keyword" :prefix-icon="Search" clearable placeholder="搜索邮箱或账号" />
      <el-scrollbar class="user-list" v-loading="loadingUsers">
        <button
          v-for="user in filteredUsers"
          :key="user.id"
          class="user-row"
          :class="{ 'user-row--active': String(user.id) === selectedUserId }"
          type="button"
          @click="selectUser(user)"
        >
          <strong>{{ user.account || user.email || '-' }}</strong>
          <span>{{ user.planName || '未绑定套餐' }} / {{ user.subscriptionStatus || '无订阅' }}</span>
        </button>
        <el-empty v-if="!loadingUsers && filteredUsers.length === 0" description="暂无匹配用户" />
      </el-scrollbar>
    </el-card>

    <el-card shadow="never" class="log-card">
      <template #header>
        <div class="card-header">
          <span>{{ selectedUser?.email || selectedUser?.account || '请选择用户' }}</span>
          <el-tag type="success" effect="plain">用户级明细</el-tag>
        </div>
      </template>

      <div class="log-toolbar">
        <el-date-picker
          v-model="filters.range"
          type="datetimerange"
          value-format="YYYY-MM-DDTHH:mm:ss[Z]"
          start-placeholder="开始时间"
          end-placeholder="结束时间"
          range-separator="至"
        />
        <el-button :icon="Search" type="primary" plain :disabled="!selectedUserId" @click="searchLogs">
          查询
        </el-button>
      </div>

      <UserTrafficLogsTable
        :logs="logs"
        :loading="loadingLogs"
        empty-text="请选择用户或等待该用户产生流量日志"
      />

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
    </el-card>
  </div>
</template>

<style scoped>
.traffic-layout {
  align-items: flex-start;
  display: grid;
  gap: 18px;
  grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
}

.user-card,
.log-card {
  border-radius: 22px;
}

.card-header,
.log-toolbar {
  align-items: center;
  display: flex;
  gap: 12px;
  justify-content: space-between;
}

.log-toolbar {
  justify-content: flex-start;
  margin-bottom: 16px;
}

.user-list {
  margin-top: 14px;
  max-height: 640px;
}

.user-row {
  background: transparent;
  border: 1px solid transparent;
  border-radius: 14px;
  color: inherit;
  cursor: pointer;
  display: block;
  margin-bottom: 8px;
  padding: 12px;
  text-align: left;
  width: 100%;
}

.user-row span {
  color: var(--el-text-color-secondary);
  display: block;
  font-size: 12px;
  margin-top: 4px;
}

.user-row--active {
  background: var(--el-color-primary-light-9);
  border-color: var(--el-color-primary-light-5);
}

.traffic-log-pagination {
  justify-content: flex-end;
  margin-top: 16px;
}

@media (max-width: 960px) {
  .traffic-layout {
    grid-template-columns: 1fr;
  }
}
</style>
