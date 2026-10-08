<!-- 用户详情中的流量入口，与全站明细共用同一查询组件。 -->
<script setup lang="ts">
import { computed } from 'vue';
import type { AdminUser } from '@/services/api';
import UserTrafficLogsPanel from './UserTrafficLogsPanel.vue';
const props = defineProps<{ modelValue: boolean; user?: AdminUser }>();
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>();
const visible = computed({ get: () => props.modelValue, set: (value: boolean) => emit('update:modelValue', value) });
</script>
<template>
  <el-drawer v-model="visible" :title="`用户流量日志：${user?.account || user?.email || ''}`" size="min(1100px, 100vw)" destroy-on-close>
    <p class="muted">按时间查询访问 IP、线路与扣费明细。历史线路名称保留为记录当时的名称。</p>
    <RouterLink v-if="user" :to="{ path: '/admin/traffic-logs', query: { user_id: String(user.id) } }" class="traffic-full-page">在完整流量页面打开</RouterLink>
    <UserTrafficLogsPanel :user-id="user?.id" :active="visible" />
  </el-drawer>
</template>
<style scoped>
.traffic-full-page { display: inline-block; margin-bottom: 18px; color: var(--el-color-primary); }
</style>
