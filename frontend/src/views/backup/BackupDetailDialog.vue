<!--
  备份「详情」弹窗：只读展示某模式完整运行状态 —— 通用字段（状态/最近开始·成功·失败/最近文件/
  脱敏错误摘要）+ 该模式特有 state 字段（全量·WAL 的存储位置 + 异地同步结果 offsite_last_sync_at 等、
  WAL 归档成功标记、邮件 email_last_sent_at + 附件名）与相关配置摘要，末尾附「历史记录」表格
  （每次备份的时间/结果/大小/文件/异地/错误，最新在前、最多 30 条）。数据全部由父级传入，本组件不发请求。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type {
  BackupActiveMode,
  BackupConfig,
  BackupDestination,
  BackupHistoryEntry,
  BackupModeState,
  BackupState,
} from '@/services/api';
import { backupStatusLabel, formatBackupTime, formatBytes, runModeLabel } from './backupSchedule';

const props = defineProps<{
  modelValue: boolean;
  mode: BackupActiveMode | null;
  state: BackupState;
  config: BackupConfig;
}>();

const emit = defineEmits<{
  (event: 'update:modelValue', value: boolean): void;
}>();

const title = computed(() => (props.mode ? `详情 · ${runModeLabel(props.mode)}` : '详情'));

type DetailRow = { label: string; value: string };

// 三态归档标记：成功/失败/无记录。
function archiveOkLabel(ok: boolean | null): string {
  if (ok === null) {
    return '—';
  }
  return ok ? '成功' : '失败';
}

// 存储位置中文名。
function destinationLabel(destination: BackupDestination): string {
  if (destination === 'offsite') {
    return '异地';
  }
  if (destination === 'both') {
    return '本机+异地';
  }
  return '本机';
}

// 全量·WAL 详情追加「存储位置 + 异地同步结果」行；仅 destination 含异地时展示同步结果。
function pushOffsiteRows(base: DetailRow[], destination: BackupDestination, s: BackupModeState) {
  base.push({ label: '存储位置', value: destinationLabel(destination) });
  if (destination === 'local') {
    return;
  }
  base.push({ label: '最近异地同步', value: formatBackupTime(s.offsiteLastSyncAt) });
  base.push({ label: '异地同步结果', value: s.offsiteSynced ? '成功' : (s.offsiteError ? '失败' : '—') });
  if (s.offsiteError) {
    base.push({ label: '异地同步错误', value: s.offsiteError });
  }
}

// 按模式拼出通用 + 特有字段行；缺失值统一走 formatBackupTime 的占位破折号。
const rows = computed<DetailRow[]>(() => {
  const mode = props.mode;
  if (!mode) {
    return [];
  }
  const s = props.state[mode];
  const base: DetailRow[] = [
    { label: '当前状态', value: backupStatusLabel(s.status) },
    { label: '最近开始', value: formatBackupTime(s.lastStartedAt) },
    { label: '最近成功', value: formatBackupTime(s.lastSuccessAt) },
    { label: '最近失败', value: formatBackupTime(s.lastFailedAt) },
    { label: '最近文件', value: s.lastFileName || '—' },
    { label: '错误摘要', value: s.errorSummary || '—' },
  ];
  if (mode === 'full') {
    base.push({ label: '排除流量日志大表', value: props.config.full.excludeTrafficLogs ? '排除' : '保留' });
    base.push({ label: '本地保留天数', value: String(props.config.full.retentionDays) });
    pushOffsiteRows(base, props.config.full.destination, s);
  } else if (mode === 'wal') {
    base.push({ label: '归档就绪', value: props.config.wal.configured ? '已就绪' : '未就绪' });
    base.push({ label: '最近归档', value: archiveOkLabel(s.walArchiveOk) });
    base.push({ label: '保留 base 个数', value: String(props.config.wal.retentionFull) });
    pushOffsiteRows(base, props.config.wal.destination, s);
  } else if (mode === 'email') {
    base.push({ label: '最近发送', value: formatBackupTime(s.emailLastSentAt) });
    base.push({ label: '最近附件名', value: s.emailAttachmentName || '—' });
    base.push({ label: '收件人数量', value: String(props.config.email.recipients.length) });
    base.push({ label: '加密附件', value: props.config.email.attachEncrypted ? '加密' : '明文' });
  }
  return base;
});

// 当前模式的历史记录（父级已归一，最新在前）。
const history = computed<BackupHistoryEntry[]>(() => (props.mode ? props.state[props.mode].history : []));

// 异地推送三态展示：已推 / 未推 / 无异地记录。
function offsiteLabel(synced: boolean | null): string {
  if (synced === true) {
    return '已推';
  }
  if (synced === false) {
    return '未推';
  }
  return '—';
}
</script>

<template>
  <el-dialog
    :model-value="modelValue"
    :title="title"
    width="720px"
    @update:model-value="emit('update:modelValue', $event)"
  >
    <el-descriptions :column="1" border>
      <el-descriptions-item v-for="row in rows" :key="row.label" :label="row.label">
        <span class="detail-value">{{ row.value }}</span>
      </el-descriptions-item>
    </el-descriptions>

    <div class="history-block">
      <div class="history-title">历史记录</div>
      <el-table :data="history" size="small" max-height="320" empty-text="暂无记录" border>
        <el-table-column label="时间" min-width="150">
          <template #default="{ row }">{{ formatBackupTime(row.at) }}</template>
        </el-table-column>
        <el-table-column label="结果" width="76" align="center">
          <template #default="{ row }">
            <el-tag :type="row.ok ? 'success' : 'danger'" size="small">{{ row.ok ? '成功' : '失败' }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="大小" width="96" align="right">
          <template #default="{ row }">{{ formatBytes(row.sizeBytes) }}</template>
        </el-table-column>
        <el-table-column label="文件" min-width="130" show-overflow-tooltip>
          <template #default="{ row }">{{ row.file || '—' }}</template>
        </el-table-column>
        <el-table-column label="异地" width="70" align="center">
          <template #default="{ row }">{{ offsiteLabel(row.offsiteSynced) }}</template>
        </el-table-column>
        <el-table-column label="错误" min-width="140" show-overflow-tooltip>
          <template #default="{ row }">{{ row.error || '—' }}</template>
        </el-table-column>
      </el-table>
    </div>

    <template #footer>
      <el-button type="primary" @click="emit('update:modelValue', false)">关闭</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.detail-value {
  word-break: break-all;
}

.history-block {
  margin-top: 16px;
}

.history-title {
  margin-bottom: 8px;
  font-weight: 600;
  font-size: 14px;
}
</style>
