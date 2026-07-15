<!--
  本页面用于普通用户查看和生成自己的邀请码。
  它展示邀请码数量、使用状态、被邀请账号和创建时间。
  邀请码生成权限由后台安全设置控制。
-->
<script setup lang="ts">
import { DocumentCopy, Plus, Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type UserInviteCodesInfo } from '@/services/api';

const loading = ref(true);
const creating = ref(false);
const inviteInfo = ref<UserInviteCodesInfo>();

const canCreate = computed(() => Boolean(inviteInfo.value?.enabled && inviteInfo.value.remainingCount > 0));
const usageText = computed(() => {
  const info = inviteInfo.value;
  if (!info) {
    return '0 / 0';
  }

  return `${info.generatedCount} / ${info.maxCount}`;
});

onMounted(load);

async function load() {
  loading.value = true;
  try {
    inviteInfo.value = await apiClient.getUserInviteCodes();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载邀请码失败');
  } finally {
    loading.value = false;
  }
}

async function createInviteCode() {
  creating.value = true;
  try {
    await apiClient.createUserInviteCode();
    inviteInfo.value = await apiClient.getUserInviteCodes();
    ElMessage.success('邀请码已生成');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '生成邀请码失败');
  } finally {
    creating.value = false;
  }
}

async function copyCode(code: string) {
  await navigator.clipboard.writeText(code);
  ElMessage.success('邀请码已复制');
}

function formatDate(value: string) {
  return value || '-';
}
</script>

<template>
  <PageHeader title="邀请码" description="查看和生成当前账号的邀请码，用于邀请来源追责。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :icon="Plus" :loading="creating" :disabled="!canCreate" @click="createInviteCode">
      生成邀请码
    </el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="8" animated />
  <template v-else-if="inviteInfo">
    <el-alert
      v-if="!inviteInfo.enabled"
      type="warning"
      show-icon
      :closable="false"
      title="管理员未开启用户自助生成邀请码"
      class="section-row"
    />

    <div class="metric-grid">
      <el-card shadow="never" class="metric-card">
        <span>自助生成</span>
        <strong>{{ inviteInfo.enabled ? '已开启' : '已关闭' }}</strong>
      </el-card>
      <el-card shadow="never" class="metric-card">
        <span>生成额度</span>
        <strong>{{ usageText }}</strong>
      </el-card>
      <el-card shadow="never" class="metric-card">
        <span>剩余额度</span>
        <strong>{{ inviteInfo.remainingCount }}</strong>
      </el-card>
    </div>

    <el-card shadow="never" class="section-row">
      <template #header>我的邀请码</template>
      <el-empty v-if="inviteInfo.items.length === 0" description="暂无邀请码" />
      <el-table v-else :data="inviteInfo.items" stripe>
        <el-table-column prop="code" label="邀请码" min-width="220">
          <template #default="{ row }">
            <el-space>
              <code>{{ row.code }}</code>
              <el-button text :icon="DocumentCopy" @click="copyCode(row.code)">复制</el-button>
            </el-space>
          </template>
        </el-table-column>
        <el-table-column label="状态" min-width="100">
          <template #default="{ row }">
            <el-tag :type="row.isUsed ? 'info' : 'success'" effect="plain">
              {{ row.isUsed ? '已使用' : '未使用' }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column prop="usedByEmail" label="使用人邮箱" min-width="180">
          <template #default="{ row }">
            {{ row.usedByEmail || '-' }}
          </template>
        </el-table-column>
        <el-table-column prop="usedAt" label="使用时间" min-width="180">
          <template #default="{ row }">
            {{ formatDate(row.usedAt) }}
          </template>
        </el-table-column>
        <el-table-column prop="createdAt" label="创建时间" min-width="180">
          <template #default="{ row }">
            {{ formatDate(row.createdAt) }}
          </template>
        </el-table-column>
      </el-table>
    </el-card>
  </template>
</template>

<style scoped>
code {
  color: #0f172a;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}
</style>
