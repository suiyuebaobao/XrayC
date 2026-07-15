<!--
  全量 pg_dump 「编辑」弹窗的表单字段片段（不含卡片/启用开关，供 BackupEditDialog 内嵌）。
  含调度（间隔/cron）、存储位置（本机/异地/本机+异地）、排除流量日志大表、本地保留天数。
  config 是父级 reactive 全量配置对象，直接就地改写（与其它备份字段片段同口径）。
-->
<script setup lang="ts">
import type { BackupFullConfig } from '@/services/api';
import BackupScheduleFields from './BackupScheduleFields.vue';

defineProps<{
  config: BackupFullConfig;
}>();
</script>

<template>
  <el-form label-position="top" class="fields-form">
    <BackupScheduleFields :schedule="config.schedule" />
    <el-form-item label="存储位置">
      <el-radio-group v-model="config.destination">
        <el-radio-button value="local">本机</el-radio-button>
        <el-radio-button value="offsite">异地</el-radio-button>
        <el-radio-button value="both">本机+异地</el-radio-button>
      </el-radio-group>
      <small class="field-hint">选「异地/本机+异地」需先在页面顶部「异地备份服务器」卡装好公钥。</small>
    </el-form-item>
    <el-form-item label="排除流量日志大表">
      <el-switch v-model="config.excludeTrafficLogs" active-text="排除" inactive-text="保留" />
      <small class="field-hint">仅删观测大表数据、保结构；计费/账单表始终保留。</small>
    </el-form-item>
    <el-form-item label="本地保留天数">
      <el-input-number v-model="config.retentionDays" :min="1" :max="3650" controls-position="right" />
    </el-form-item>
  </el-form>
</template>

<style scoped>
.fields-form {
  max-width: 100%;
}

.field-hint {
  display: block;
  margin-top: 4px;
  color: var(--el-text-color-secondary);
}
</style>
