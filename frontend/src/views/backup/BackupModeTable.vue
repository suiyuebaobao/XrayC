<!--
  数据库备份「运行状态」主表：每行一种备份方式（全量/WAL/邮件）。异地已并入全量·WAL，不再单列一行。
  列：方式名 · 启用开关（切换即改 enabled 并保存）· 状态（中文）· 最近成功时间 · 操作（编辑/详情/立即备份）。
  自身不发请求：config/state/running 由父级传入，切换/编辑/详情/立即备份经事件交父级处理。
-->
<script setup lang="ts">
import { Edit, InfoFilled, VideoPlay } from '@element-plus/icons-vue';
import type { BackupActiveMode, BackupConfig, BackupState } from '@/services/api';
import { backupStatusLabel, backupStatusTagType, formatBackupTime, runModeLabel } from './backupSchedule';

const props = defineProps<{
  config: BackupConfig;
  state: BackupState;
  running: Record<BackupActiveMode, boolean>;
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'toggle', mode: BackupActiveMode, enabled: boolean): void;
  (event: 'edit', mode: BackupActiveMode): void;
  (event: 'detail', mode: BackupActiveMode): void;
  (event: 'run', mode: BackupActiveMode): void;
}>();

const MODES: BackupActiveMode[] = ['full', 'wal', 'email'];

type ModeRow = { mode: BackupActiveMode; label: string };

// 表格数据源只需模式与中文名，具体启用/状态用 config/state 现取（保持响应式）。
const rows = (): ModeRow[] => MODES.map((mode) => ({ mode, label: runModeLabel(mode) }));
</script>

<template>
  <el-card shadow="never">
    <template #header>运行状态</template>
    <el-table :data="rows()" class="mode-table">
      <el-table-column label="备份方式" min-width="130">
        <template #default="{ row }">
          <span class="mode-name">{{ row.label }}</span>
        </template>
      </el-table-column>
      <el-table-column label="启用" width="90">
        <template #default="{ row }">
          <el-switch
            :model-value="props.config[row.mode as BackupActiveMode].enabled"
            :loading="props.saving"
            @change="emit('toggle', row.mode as BackupActiveMode, $event as boolean)"
          />
        </template>
      </el-table-column>
      <el-table-column label="状态" width="100">
        <template #default="{ row }">
          <el-tag
            :type="backupStatusTagType(props.state[row.mode as BackupActiveMode].status)"
            effect="plain"
          >
            {{ backupStatusLabel(props.state[row.mode as BackupActiveMode].status) }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="最近成功" min-width="170">
        <template #default="{ row }">
          {{ formatBackupTime(props.state[row.mode as BackupActiveMode].lastSuccessAt) }}
        </template>
      </el-table-column>
      <el-table-column label="操作" width="270">
        <template #default="{ row }">
          <el-button size="small" :icon="Edit" @click="emit('edit', row.mode as BackupActiveMode)">
            编辑
          </el-button>
          <el-button size="small" :icon="InfoFilled" @click="emit('detail', row.mode as BackupActiveMode)">
            详情
          </el-button>
          <el-button
            size="small"
            type="primary"
            plain
            :icon="VideoPlay"
            :loading="props.running[row.mode as BackupActiveMode]"
            @click="emit('run', row.mode as BackupActiveMode)"
          >
            立即备份
          </el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>
</template>

<style scoped>
.mode-table {
  width: 100%;
}

.mode-name {
  font-weight: 500;
}
</style>
