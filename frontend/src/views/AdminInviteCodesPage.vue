<!--
  本页面用于管理员生成和管理注册邀请码。
  管理员生成不受用户自助开关限制，可直接给新用户注册使用。
  已使用邀请码保留邀请来源和使用人信息，不允许删除以便追责。
  页面只调用管理端接口，不复用普通用户自助额度逻辑。
-->
<script setup lang="ts">
import { Delete, DocumentCopy, Plus, Refresh } from '@element-plus/icons-vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type AdminInviteCode } from '@/services/api';

const loading = ref(true);
const saving = ref(false);
const deletingCode = ref('');
const dialogOpen = ref(false);
const inviteCodes = ref<AdminInviteCode[]>([]);
const batchLimit = 100;

const form = reactive({
  count: 10,
});

const unusedCount = computed(() => inviteCodes.value.filter((item) => !item.isUsed).length);
const usedCount = computed(() => inviteCodes.value.filter((item) => item.isUsed).length);
const inviterCount = computed(() => new Set(inviteCodes.value.map((item) => item.inviterEmail).filter(Boolean)).size);

onMounted(load);

async function load() {
  loading.value = true;
  try {
    inviteCodes.value = await apiClient.getAdminInviteCodes();
  } catch (error) {
    ElMessage.error(errorText(error, '加载邀请码失败'));
  } finally {
    loading.value = false;
  }
}

async function createCodes() {
  if (form.count < 1 || form.count > batchLimit) {
    ElMessage.warning(`生成数量必须在 1 到 ${batchLimit} 之间`);
    return;
  }

  saving.value = true;
  try {
    await apiClient.createAdminInviteCodes({ count: form.count });
    dialogOpen.value = false;
    await load();
    ElMessage.success('邀请码已生成');
  } catch (error) {
    ElMessage.error(errorText(error, '生成邀请码失败'));
  } finally {
    saving.value = false;
  }
}

async function deleteCode(row: AdminInviteCode) {
  if (row.isUsed) {
    ElMessage.warning('已使用的邀请码需要保留追责记录，不能删除');
    return;
  }
  await ElMessageBox.confirm(`确认删除邀请码 ${row.code}？`, '删除邀请码', {
    confirmButtonText: '删除',
    cancelButtonText: '取消',
    type: 'warning',
  });

  deletingCode.value = row.code;
  try {
    await apiClient.deleteAdminInviteCode(row.code);
    await load();
    ElMessage.success('邀请码已删除');
  } catch (error) {
    ElMessage.error(errorText(error, '删除邀请码失败'));
  } finally {
    deletingCode.value = '';
  }
}

async function copyCode(code: string) {
  await navigator.clipboard.writeText(code);
  ElMessage.success('邀请码已复制');
}

function statusType(row: AdminInviteCode) {
  return row.isUsed ? 'info' : 'success';
}

function statusLabel(row: AdminInviteCode) {
  return row.isUsed ? '已使用' : '未使用';
}

function formatDate(value: string) {
  return value || '-';
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}
</script>

<template>
  <PageHeader title="邀请码管理" description="管理员生成注册邀请码，并查看邀请来源、使用人和使用时间。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :icon="Plus" @click="dialogOpen = true">批量生成</el-button>
  </PageHeader>

  <div class="metric-grid">
    <el-card shadow="never" class="metric-card">
      <span>邀请码总数</span>
      <strong>{{ inviteCodes.length }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>未使用</span>
      <strong>{{ unusedCount }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>已使用</span>
      <strong>{{ usedCount }}</strong>
    </el-card>
    <el-card shadow="never" class="metric-card">
      <span>邀请来源数</span>
      <strong>{{ inviterCount }}</strong>
    </el-card>
  </div>

  <el-card shadow="never">
    <template #header>邀请码列表</template>
    <el-table v-loading="loading" :data="inviteCodes" row-key="code" stripe>
      <el-table-column prop="code" label="邀请码" min-width="240">
        <template #default="{ row }: { row: AdminInviteCode }">
          <el-space>
            <code>{{ row.code }}</code>
            <el-button text :icon="DocumentCopy" @click="copyCode(row.code)">复制</el-button>
          </el-space>
        </template>
      </el-table-column>
      <el-table-column label="状态" min-width="110">
        <template #default="{ row }: { row: AdminInviteCode }">
          <el-tag :type="statusType(row)" effect="plain">{{ statusLabel(row) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="inviterEmail" label="邀请人邮箱" min-width="190">
        <template #default="{ row }: { row: AdminInviteCode }">{{ row.inviterEmail || '-' }}</template>
      </el-table-column>
      <el-table-column prop="usedByEmail" label="使用人邮箱" min-width="190">
        <template #default="{ row }: { row: AdminInviteCode }">{{ row.usedByEmail || '-' }}</template>
      </el-table-column>
      <el-table-column label="使用时间" min-width="180">
        <template #default="{ row }: { row: AdminInviteCode }">{{ formatDate(row.usedAt) }}</template>
      </el-table-column>
      <el-table-column label="创建时间" min-width="180">
        <template #default="{ row }: { row: AdminInviteCode }">{{ formatDate(row.createdAt) }}</template>
      </el-table-column>
      <el-table-column label="操作" width="120" fixed="right">
        <template #default="{ row }: { row: AdminInviteCode }">
          <el-button
            text
            type="danger"
            :icon="Delete"
            :disabled="row.isUsed"
            :loading="deletingCode === row.code"
            @click="deleteCode(row)"
          >
            删除
          </el-button>
        </template>
      </el-table-column>
      <template #empty>
        <el-empty description="暂无邀请码数据" />
      </template>
    </el-table>
  </el-card>

  <el-dialog v-model="dialogOpen" title="批量生成邀请码" width="480px">
    <el-form label-position="top">
      <el-form-item label="生成数量">
        <el-input-number v-model="form.count" :min="1" :max="batchLimit" controls-position="right" />
        <span class="field-suffix">每次最多 {{ batchLimit }} 个</span>
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="dialogOpen = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="createCodes">生成</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
code {
  color: #0f172a;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.field-suffix {
  color: var(--ink-soft);
  margin-left: 8px;
}
</style>
