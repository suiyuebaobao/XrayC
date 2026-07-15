<!--
  本页面用于后台管理用户、角色、状态、套餐和订阅链接。
  它支持编辑用户基础信息、查看订阅和重置订阅 token。
  列表查询支持关键词、状态、角色、分页和刷新。
  真实订阅地址只从后端返回，页面不拼接出口上游信息。
-->
<script setup lang="ts">
import { CopyDocument, Delete, Edit, Lock, Plus, Refresh, RefreshRight, Search, View } from '@element-plus/icons-vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type AdminUser, type AdminUserQuery, type AdminUserSubscription, type PlanInfo } from '@/services/api';
import AdminResetPasswordDialog from '@/views/users/AdminResetPasswordDialog.vue';
import UserDevicesDrawer from '@/views/users/UserDevicesDrawer.vue';
import UserFormDialog from '@/views/users/UserFormDialog.vue';
import UserTrafficLogsDrawer from '@/views/users/UserTrafficLogsDrawer.vue';
const loading = ref(true);
const detailLoading = ref(false);
const resetTokenLoading = ref(false);
const users = ref<AdminUser[]>([]);
const total = ref(0);
const plans = ref<PlanInfo[]>([]);
const selectedUsers = ref<AdminUser[]>([]);
const selectedUser = ref<AdminUser>();
const selectedSubscription = ref<AdminUserSubscription>();
const subscriptionDialogOpen = ref(false);
const formDialogOpen = ref(false);
const formMode = ref<'create' | 'edit'>('create');
const editingUser = ref<AdminUser>();
const devicesDrawerOpen = ref(false);
const devicesUser = ref<AdminUser>();
const trafficLogsDrawerOpen = ref(false);
const trafficLogsUser = ref<AdminUser>();
const resetPasswordDialogOpen = ref(false);
const resetPasswordUser = ref<AdminUser>();
let subscriptionRequestSeq = 0;
const filters = reactive({
  keyword: '',
  status: '',
  role: '',
  page: 1,
  pageSize: 20,
});
const activeSubscriptionCount = computed(
  () => users.value.filter((user) => user.subscriptionStatus === 'active' || user.subscriptionStatus === 'enabled').length,
);
const expiringSoonCount = computed(() => users.value.filter((user) => expiresSoon(user.subscriptionExpiresAt)).length);
const disabledUserCount = computed(() => users.value.filter((user) => user.status === 'disabled').length);
const totalUserCount = computed(() => total.value || users.value.length);
const selectedUserIds = computed(() => selectedUsers.value.map((user) => user.id));
onMounted(load);
async function load() {
  loading.value = true;
  try {
    const [userResult, planResult] = await Promise.allSettled([
      apiClient.getAdminUsersPage(adminUserQuery()),
      apiClient.getAdminPlans(),
    ]);
    if (userResult.status === 'fulfilled') {
      users.value = userResult.value.items;
      total.value = userResult.value.total;
      filters.page = userResult.value.page;
      filters.pageSize = userResult.value.pageSize;
    } else {
      users.value = [];
      total.value = 0;
      ElMessage.error(errorText(userResult.reason, '加载用户列表失败'));
    }
    if (planResult.status === 'fulfilled') {
      plans.value = planResult.value;
    } else {
      plans.value = [];
      ElMessage.error(errorText(planResult.reason, '加载套餐列表失败'));
    }
  } finally {
    loading.value = false;
  }
}

function adminUserQuery(): AdminUserQuery {
  return {
    page: filters.page,
    pageSize: filters.pageSize,
    keyword: filters.keyword.trim() || undefined,
    status: filters.status || undefined,
    role: filters.role || undefined,
  };
}
function searchUsers() {
  filters.page = 1;
  load();
}
function resetFilters() {
  filters.keyword = '';
  filters.status = '';
  filters.role = '';
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
function openCreate() {
  editingUser.value = undefined;
  formMode.value = 'create';
  formDialogOpen.value = true;
}
async function openEdit(user: AdminUser) {
  try {
    editingUser.value = await apiClient.getAdminUser(user.id);
    formMode.value = 'edit';
    formDialogOpen.value = true;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载用户详情失败');
  }
}

async function deleteUser(user: AdminUser) {
  try {
    await ElMessageBox.confirm(`确认删除用户 ${user.email || user.account}？删除后该用户不能再登录。`, '删除用户', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  try {
    await apiClient.deleteAdminUser(user.id);
    await load();
    ElMessage.success('用户已删除');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '删除用户失败');
  }
}
async function batchDeleteUsers() {
  if (selectedUserIds.value.length === 0) {
    ElMessage.warning('请选择要删除的用户');
    return;
  }
  try {
    await ElMessageBox.confirm(`确认删除选中的 ${selectedUserIds.value.length} 个用户？`, '批量删除用户', {
      type: 'warning',
      confirmButtonText: '批量删除',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  try {
    const result = await apiClient.deleteAdminUsers(selectedUserIds.value);
    selectedUsers.value = [];
    await load();
    ElMessage.success(`已删除 ${result.deletedCount} 个用户`);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '批量删除用户失败');
  }
}
function selectUsers(rows: AdminUser[]) {
  selectedUsers.value = rows;
}
async function openSubscription(user: AdminUser) {
  const requestSeq = ++subscriptionRequestSeq;
  selectedUser.value = user;
  selectedSubscription.value = undefined;
  subscriptionDialogOpen.value = true;
  detailLoading.value = true;
  try {
    const subscription = await apiClient.getAdminUserSubscription(user.id);
    if (requestSeq === subscriptionRequestSeq && selectedUser.value?.id === user.id) {
      selectedSubscription.value = subscription;
    }
  } catch (error) {
    if (requestSeq === subscriptionRequestSeq) {
      ElMessage.error(error instanceof Error ? error.message : '加载用户订阅失败');
    }
  } finally {
    if (requestSeq === subscriptionRequestSeq) {
      detailLoading.value = false;
    }
  }
}

async function resetSelectedSubscriptionToken() {
  if (!selectedUser.value) {
    return;
  }
  try {
    await ElMessageBox.confirm('重置后该用户旧订阅链接会立即失效，是否继续？', '重置用户订阅链接', {
      type: 'warning',
      confirmButtonText: '重置',
      cancelButtonText: '取消',
    });
  } catch {
    return;
  }
  resetTokenLoading.value = true;
  try {
    selectedSubscription.value = await apiClient.resetAdminUserSubscriptionToken(selectedUser.value.id);
    ElMessage.success('订阅链接已重置');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '重置订阅链接失败');
  } finally {
    resetTokenLoading.value = false;
  }
}
function openTrafficLogs(user: AdminUser) {
  // 只记录当前用户，抽屉组件按用户 ID 加载用户级流量日志。
  trafficLogsUser.value = user;
  trafficLogsDrawerOpen.value = true;
}
function openDevices(user: AdminUser) {
  devicesUser.value = user;
  devicesDrawerOpen.value = true;
}
async function copyUserId(id: number | string) {
  // 复制用户专用 ID（UUID）到剪贴板，方便管理员引用/检索/工单沟通。
  try {
    await navigator.clipboard.writeText(String(id));
    ElMessage.success('用户 ID 已复制');
  } catch {
    ElMessage.warning('复制失败，请手动选择文本复制');
  }
}
function openResetPassword(user: AdminUser) {
  // admin 直接设新密码，弹窗自包含校验与提交，无需邮箱验证码。
  resetPasswordUser.value = user;
  resetPasswordDialogOpen.value = true;
}
function statusType(status: AdminUser['status']) {
  if (status === 'active') {
    return 'success';
  }
  if (status === 'locked' || status === 'pending') {
    return 'warning';
  }
  return status === 'disabled' ? 'info' : 'primary';
}
function roleLabel(role: AdminUser['role']) {
  return role === 'admin' ? '管理员' : '用户';
}
function statusLabel(status: AdminUser['status']) {
  const labels: Record<AdminUser['status'], string> = {
    active: '正常',
    disabled: '禁用',
    locked: '锁定',
    pending: '待确认',
    unknown: '未知',
  };
  return labels[status];
}
function subscriptionLabel(status: string) {
  const normalized = status.toLowerCase();
  if (normalized === 'active' || normalized === 'enabled') {
    return '有效';
  }

  if (normalized === 'expired') {
    return '已过期';
  }

  if (normalized === 'cancelled' || normalized === 'canceled') {
    return '已取消';
  }

  return status || '未开通';
}

function subscriptionTagType(status: string) {
  const normalized = status.toLowerCase();
  if (normalized === 'active' || normalized === 'enabled') {
    return 'success';
  }

  if (normalized === 'expired') {
    return 'warning';
  }

  return normalized ? 'info' : 'primary';
}

function trafficLabel(user: Pick<AdminUser, 'trafficGb'>) {
  return trafficPair(totalTraffic(user));
}

function trafficPair(value: { used: number; total: number }) {
  return `${formatGb(value.used)} / ${formatGb(value.total)}`;
}

function totalTraffic(user: Pick<AdminUser, 'trafficGb'>) {
  return {
    used: user.trafficGb.used,
    total: user.trafficGb.total,
  };
}

function formatGb(value: number) {
  return `${Number(value.toFixed(value >= 10 ? 0 : 2))} GB`;
}

function rateLimitLabel(user: AdminUser) {
  if (!Number.isFinite(user.effectiveRateLimitMbps) || user.effectiveRateLimitMbps <= 0) {
    return '不限速';
  }
  const suffix = user.userRateLimitBps === null ? '套餐' : '用户';
  return `${Number(user.effectiveRateLimitMbps.toFixed(user.effectiveRateLimitMbps >= 10 ? 0 : 2))} Mbps（${suffix}）`;
}

function formatDate(value: string) {
  return value || '-';
}

function expiresSoon(value: string) {
  if (!value) {
    return false;
  }

  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) {
    return false;
  }

  const now = Date.now();
  const sevenDays = 7 * 24 * 60 * 60 * 1000;
  return time >= now && time - now <= sevenDays;
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}
</script>

<template>
  <PageHeader title="用户管理" description="查看用户、订阅状态和流量摘要。">
    <el-button :icon="Plus" type="primary" @click="openCreate">新增用户</el-button>
    <el-button :icon="Delete" type="danger" :disabled="selectedUserIds.length === 0" @click="batchDeleteUsers">
      批量删除
    </el-button>
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
  </PageHeader>

  <div class="metric-grid">
    <el-card shadow="never" class="metric-card">
      <span>用户总数</span>
      <strong>{{ totalUserCount }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>有效订阅</span>
      <strong>{{ activeSubscriptionCount }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>7 日内到期</span>
      <strong>{{ expiringSoonCount }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>禁用用户</span>
      <strong>{{ disabledUserCount }}</strong>
    </el-card>
  </div>

  <el-card shadow="never" class="section-row">
    <template #header>用户列表</template>
    <el-form class="user-filter" :inline="true" @submit.prevent>
      <el-form-item label="关键词">
        <el-input
          v-model="filters.keyword"
          clearable
          placeholder="邮箱、名称、套餐"
          style="width: 220px"
          @keyup.enter="searchUsers"
        />
      </el-form-item>
      <el-form-item label="状态">
        <el-select v-model="filters.status" clearable placeholder="全部状态" style="width: 140px">
          <el-option label="正常" value="active" />
          <el-option label="禁用" value="disabled" />
        </el-select>
      </el-form-item>
      <el-form-item label="角色">
        <el-select v-model="filters.role" clearable placeholder="全部角色" style="width: 140px">
          <el-option label="普通用户" value="user" />
          <el-option label="管理员" value="admin" />
        </el-select>
      </el-form-item>
      <el-form-item>
        <el-button :icon="Search" type="primary" @click="searchUsers">查询</el-button>
        <el-button @click="resetFilters">重置</el-button>
      </el-form-item>
    </el-form>
    <el-table v-loading="loading" :data="users" row-key="id" stripe @selection-change="selectUsers">
      <el-table-column type="selection" width="48" />
      <el-table-column label="账号" min-width="250">
        <template #default="{ row }: { row: AdminUser }">
          <strong>{{ row.account || row.email || '-' }}</strong>
          <p class="muted">{{ row.name || row.email || '-' }}</p>
          <p class="muted user-id-line" :title="`点击复制用户ID：${row.id}`" @click="copyUserId(row.id)">
            ID: {{ row.id }}
            <el-icon class="user-id-copy"><CopyDocument /></el-icon>
          </p>
        </template>
      </el-table-column>
      <el-table-column label="角色" min-width="100">
        <template #default="{ row }: { row: AdminUser }">
          {{ roleLabel(row.role) }}
        </template>
      </el-table-column>
      <el-table-column label="套餐" min-width="140">
        <template #default="{ row }: { row: AdminUser }">
          {{ row.planName || '-' }}
        </template>
      </el-table-column>
      <el-table-column label="订阅状态" min-width="130">
        <template #default="{ row }: { row: AdminUser }">
          <el-tag :type="subscriptionTagType(row.subscriptionStatus)" effect="plain">
            {{ subscriptionLabel(row.subscriptionStatus) }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="到期时间" min-width="170">
        <template #default="{ row }: { row: AdminUser }">
          {{ formatDate(row.subscriptionExpiresAt) }}
        </template>
      </el-table-column>
      <el-table-column label="流量用量" min-width="260">
        <template #default="{ row }: { row: AdminUser }">
          {{ trafficLabel(row) }}
        </template>
      </el-table-column>
      <el-table-column label="限速" min-width="150">
        <template #default="{ row }: { row: AdminUser }">
          {{ rateLimitLabel(row) }}
        </template>
      </el-table-column>
      <el-table-column label="用户状态" min-width="110">
        <template #default="{ row }: { row: AdminUser }">
          <el-tag :type="statusType(row.status)" effect="plain">{{ statusLabel(row.status) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="注册时间" min-width="170">
        <template #default="{ row }: { row: AdminUser }">
          {{ formatDate(row.createdAt) }}
        </template>
      </el-table-column>
      <el-table-column label="操作" width="380" fixed="right">
        <template #default="{ row }: { row: AdminUser }">
          <el-button :icon="Edit" text type="primary" @click="openEdit(row)">编辑用户</el-button>
          <el-button :icon="View" text type="primary" @click="openSubscription(row)">订阅</el-button>
          <el-button text type="primary" @click="openTrafficLogs(row)">流量日志</el-button>
          <el-button text type="primary" @click="openDevices(row)">设备管理</el-button>
          <el-button :icon="Lock" text type="warning" @click="openResetPassword(row)">重置密码</el-button>
          <el-button :icon="Delete" text type="danger" @click="deleteUser(row)">删除</el-button>
        </template>
      </el-table-column>
      <template #empty>
        <el-empty description="暂无用户数据" />
      </template>
    </el-table>
    <el-pagination
      v-model:current-page="filters.page"
      v-model:page-size="filters.pageSize"
      class="user-pagination"
      :total="total"
      :page-sizes="[10, 20, 50, 100, 200]"
      layout="total, sizes, prev, pager, next, jumper"
      @current-change="changePage"
      @size-change="changePageSize"
    />
  </el-card>

  <UserDevicesDrawer v-model="devicesDrawerOpen" :user="devicesUser" />
  <UserTrafficLogsDrawer v-model="trafficLogsDrawerOpen" :user="trafficLogsUser" />

  <el-dialog v-model="subscriptionDialogOpen" :title="`订阅详情：${selectedUser?.account ?? ''}`" width="680px">
    <el-skeleton v-if="detailLoading" :rows="5" animated />
    <el-descriptions v-else-if="selectedSubscription" :column="2" border>
      <el-descriptions-item label="套餐">{{ selectedSubscription.planName || '-' }}</el-descriptions-item>
      <el-descriptions-item label="状态">
        {{ subscriptionLabel(selectedSubscription.status) }}
      </el-descriptions-item>
      <el-descriptions-item label="到期时间">{{ formatDate(selectedSubscription.expiresAt) }}</el-descriptions-item>
      <el-descriptions-item label="总流量">
        {{ trafficPair(totalTraffic(selectedSubscription)) }}
      </el-descriptions-item>
      <el-descriptions-item label="订阅链接">
        <span class="subscription-url">{{ selectedSubscription.subscriptionUrl || '-' }}</span>
      </el-descriptions-item>
    </el-descriptions>
    <el-empty v-else description="暂无订阅详情" />
    <template #footer>
      <el-button
        v-if="selectedSubscription"
        :icon="RefreshRight"
        :loading="resetTokenLoading"
        type="warning"
        plain
        @click="resetSelectedSubscriptionToken"
      >
        重置订阅链接
      </el-button>
    </template>
  </el-dialog>

  <UserFormDialog
    v-model="formDialogOpen"
    :mode="formMode"
    :user="editingUser"
    :plans="plans"
    @saved="load"
  />
  <AdminResetPasswordDialog v-model="resetPasswordDialogOpen" :user="resetPasswordUser" />
</template>

<style scoped>
.user-filter {
  margin-bottom: 16px;
}

.user-pagination {
  margin-top: 16px;
  justify-content: flex-end;
}

.subscription-url {
  word-break: break-all;
}

.user-id-line {
  cursor: pointer;
  font-size: 12px;
  word-break: break-all;
}

.user-id-line:hover {
  color: var(--el-color-primary);
}

.user-id-copy {
  font-size: 12px;
  vertical-align: middle;
}

</style>
