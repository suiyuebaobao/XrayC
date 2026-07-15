<!--
  WAL 增量 / PITR「编辑」弹窗的表单字段片段（不含卡片/启用开关，供 BackupEditDialog 内嵌）。
  已就绪(configured=true)显示「WAL 归档已就绪」(success)，否则提示「首次启用需重启一次数据库」；
  含存储位置（本机/异地/本机+异地）、base 调度（间隔/cron）、保留 base 个数。不含重启按钮。
  config 是父级 reactive WAL 配置对象，直接就地改写（与其它备份字段片段同口径）。
-->
<script setup lang="ts">
import type { BackupWalConfig } from '@/services/api';
import BackupScheduleFields from './BackupScheduleFields.vue';

defineProps<{
  config: BackupWalConfig;
}>();
</script>

<template>
  <el-form label-position="top" class="fields-form">
    <el-alert v-if="config.configured" type="success" :closable="false" show-icon class="wal-warn" title="WAL 归档已就绪">
      archive_mode / 归档链路已配置就绪，无需再次重启数据库。
    </el-alert>
    <el-alert v-else type="warning" :closable="false" show-icon class="wal-warn" title="首次启用需重启一次数据库">
      开启 WAL 归档需改 archive_mode，首次须重启一次 PostgreSQL 才生效（会短暂停机），请在受控时段上线。
    </el-alert>

    <el-form-item label="存储位置">
      <el-radio-group v-model="config.destination">
        <el-radio-button value="local">本机</el-radio-button>
        <el-radio-button value="offsite">异地</el-radio-button>
        <el-radio-button value="both">本机+异地</el-radio-button>
      </el-radio-group>
      <small v-if="config.destination === 'offsite'" class="field-hint">仅异地：恢复时需从异地取回，步骤稍多。</small>
      <small v-else class="field-hint">选「异地/本机+异地」需先在页面顶部「异地备份服务器」卡装好公钥。</small>
    </el-form-item>
    <BackupScheduleFields :schedule="config.fullBackupSchedule" />
    <el-form-item label="保留 base 备份个数">
      <el-input-number v-model="config.retentionFull" :min="1" :max="365" controls-position="right" />
    </el-form-item>
  </el-form>
</template>

<style scoped>
.fields-form {
  max-width: 100%;
}

.wal-warn {
  margin-bottom: 16px;
}

.field-hint {
  display: block;
  margin-top: 4px;
  color: var(--el-text-color-secondary);
}
</style>
