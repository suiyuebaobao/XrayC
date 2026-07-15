<!--
  本页面用于后台生成和查看兑换码。
  它支持按套餐批量生成兑换码并查看使用状态。
  兑换码明细只展示必要运营字段，不包含用户敏感凭据。
-->
<script setup lang="ts">
import { Plus, Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type PlanInfo, type RedeemCodeInfo } from '@/services/api';

const loading = ref(true);
const saving = ref(false);
const codes = ref<RedeemCodeInfo[]>([]);
const plans = ref<PlanInfo[]>([]);
const createDialogOpen = ref(false);
const redeemCodeBatchLimit = 100;

const createForm = reactive({
  planId: '',
  count: 10,
  durationDays: 30,
  expiresAt: '',
});

const unusedCount = computed(() => codes.value.filter((code) => normalizedStatus(code.status) === 'unused').length);
const usedCount = computed(() => codes.value.filter((code) => normalizedStatus(code.status) === 'used').length);
const expiredCount = computed(() => codes.value.filter((code) => normalizedStatus(code.status) === 'expired').length);

onMounted(load);

async function load() {
  loading.value = true;
  try {
    const [codeResult, planResult] = await Promise.allSettled([
      apiClient.getAdminRedeemCodes(),
      apiClient.getPlans(),
    ]);

    if (codeResult.status === 'fulfilled') {
      codes.value = codeResult.value;
    } else {
      codes.value = [];
      ElMessage.error(errorText(codeResult.reason, '加载兑换码失败'));
    }

    if (planResult.status === 'fulfilled') {
      plans.value = planResult.value;
      createForm.planId ||= planResult.value[0]?.id ?? '';
    } else {
      plans.value = [];
      ElMessage.error(errorText(planResult.reason, '加载套餐列表失败'));
    }
  } finally {
    loading.value = false;
  }
}

async function createCodes() {
  if (!createForm.planId) {
    ElMessage.warning('请选择套餐');
    return;
  }

  if (createForm.count < 1 || createForm.count > redeemCodeBatchLimit) {
    ElMessage.warning(`生成数量必须在 1 到 ${redeemCodeBatchLimit} 之间`);
    return;
  }

  saving.value = true;
  try {
    await apiClient.createAdminRedeemCodes({
      planId: createForm.planId,
      count: createForm.count,
      durationDays: createForm.durationDays,
      expiresAt: createForm.expiresAt,
    });
    createDialogOpen.value = false;
    await load();
    ElMessage.success('兑换码已生成');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '生成兑换码失败');
  } finally {
    saving.value = false;
  }
}

function normalizedStatus(status: string) {
  const value = status.toLowerCase();
  if (value === 'unused' || value === 'active' || value === 'available') {
    return 'unused';
  }

  if (value === 'used' || value === 'redeemed') {
    return 'used';
  }

  if (value === 'expired') {
    return 'expired';
  }

  return value || 'unknown';
}

function statusLabel(status: string) {
  const value = normalizedStatus(status);
  if (value === 'unused') {
    return '未使用';
  }

  if (value === 'used') {
    return '已使用';
  }

  if (value === 'expired') {
    return '已过期';
  }

  return status || '未知';
}

function statusType(status: string) {
  const value = normalizedStatus(status);
  if (value === 'unused') {
    return 'success';
  }

  if (value === 'expired') {
    return 'warning';
  }

  return value === 'used' ? 'info' : 'primary';
}

function durationLabel(days: number | null) {
  return days ? `${days} 天` : '-';
}

function trafficLabel(code: RedeemCodeInfo) {
  const value = code.trafficGb;
  if (value === null) {
    return '-';
  }
  return `${Number(value.toFixed(value >= 10 ? 0 : 2))} GB`;
}

function formatDate(value: string) {
  return value || '-';
}

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}
</script>

<template>
  <PageHeader title="兑换码管理" description="批量生成兑换码，并查看使用状态。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :icon="Plus" @click="createDialogOpen = true">批量生成</el-button>
  </PageHeader>

  <div class="metric-grid">
    <el-card shadow="never" class="metric-card">
      <span>兑换码总数</span>
      <strong>{{ codes.length }}</strong>
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
      <span>已过期</span>
      <strong>{{ expiredCount }}</strong>
    </el-card>
  </div>

  <el-card shadow="never">
    <template #header>兑换码列表</template>
    <el-table v-loading="loading" :data="codes" row-key="id" stripe>
      <el-table-column prop="code" label="兑换码" min-width="190" />
      <el-table-column prop="planName" label="套餐" min-width="140" />
      <el-table-column label="兑换流量" min-width="150">
        <template #default="{ row }: { row: RedeemCodeInfo }">
          {{ trafficLabel(row) }}
        </template>
      </el-table-column>
      <el-table-column label="有效期" min-width="110">
        <template #default="{ row }: { row: RedeemCodeInfo }">
          {{ durationLabel(row.durationDays) }}
        </template>
      </el-table-column>
      <el-table-column label="状态" min-width="120">
        <template #default="{ row }: { row: RedeemCodeInfo }">
          <el-tag :type="statusType(row.status)" effect="plain">{{ statusLabel(row.status) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="使用人" min-width="180">
        <template #default="{ row }: { row: RedeemCodeInfo }">
          {{ row.usedBy || '-' }}
        </template>
      </el-table-column>
      <el-table-column label="使用时间" min-width="180">
        <template #default="{ row }: { row: RedeemCodeInfo }">
          {{ formatDate(row.usedAt) }}
        </template>
      </el-table-column>
      <el-table-column label="过期时间" min-width="180">
        <template #default="{ row }: { row: RedeemCodeInfo }">
          {{ formatDate(row.expiresAt) }}
        </template>
      </el-table-column>
      <template #empty>
        <el-empty description="暂无兑换码数据" />
      </template>
    </el-table>
  </el-card>

  <el-dialog v-model="createDialogOpen" title="批量生成兑换码" width="560px">
    <el-form label-position="top">
      <el-form-item label="套餐">
        <el-select v-model="createForm.planId" filterable placeholder="选择套餐" class="form-field">
          <el-option v-for="plan in plans" :key="plan.id" :label="plan.name" :value="plan.id" />
        </el-select>
      </el-form-item>
      <el-form-item label="生成数量">
        <el-input-number v-model="createForm.count" :min="1" :max="redeemCodeBatchLimit" controls-position="right" />
        <span class="field-suffix">每次最多 {{ redeemCodeBatchLimit }} 个，与后端限制一致</span>
      </el-form-item>
      <el-form-item label="兑换后有效期">
        <el-input-number v-model="createForm.durationDays" :min="1" :max="3650" controls-position="right" />
        <span class="field-suffix">天</span>
      </el-form-item>
      <el-form-item label="兑换码过期时间">
        <el-date-picker
          v-model="createForm.expiresAt"
          type="datetime"
          value-format="YYYY-MM-DDTHH:mm:ssZ"
          placeholder="不限制"
          class="form-field"
        />
      </el-form-item>
    </el-form>
    <template #footer>
      <el-button @click="createDialogOpen = false">取消</el-button>
      <el-button type="primary" :loading="saving" @click="createCodes">生成</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.form-field {
  width: 100%;
}

.traffic-stack {
  display: grid;
  gap: 4px;
}

.field-suffix {
  margin-left: 8px;
  color: var(--ink-soft);
}
</style>
