<!--
  后台「数据库备份」管理页，以「运行状态」表格为中心：
  - 顶部一张「异地备份服务器」共享连接卡（rsync 目标 + 测试并装公钥）；异地是共享目标、不再是独立模式，
    全量/WAL 各自的「存储位置」决定是否推异地。
  - 主体一张运行状态表：每行一种备份方式（全量/WAL/邮件），列含启用开关、状态、
    最近成功时间与「编辑 / 详情 / 立即备份」操作。
  - 「编辑」弹窗改该模式调度与字段并保存（写时保留：密码/口令留空=不改）；「详情」弹窗只读展示完整运行状态。
  本页只做编排、加载/保存与事件处理；表格、两种弹窗、字段片段、服务器卡均拆子组件（守 550 行上限）。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import {
  apiClient,
  type BackupActiveMode,
  type BackupConfig,
  type BackupModeState,
  type BackupState,
} from '@/services/api';
import { runModeLabel } from './backupSchedule';
import BackupDetailDialog from './BackupDetailDialog.vue';
import BackupEditDialog from './BackupEditDialog.vue';
import BackupModeTable from './BackupModeTable.vue';
import BackupOffsiteServerCard from './BackupOffsiteServerCard.vue';

const loading = ref(true);
const saving = ref(false);
const testing = ref(false);
const running = reactive<Record<BackupActiveMode, boolean>>({
  full: false,
  wal: false,
  email: false,
});

const config = reactive<BackupConfig>({
  offsite: {
    sshHost: '',
    sshPort: 22,
    sshUser: 'root',
    sshPassword: '',
    sshPasswordSet: false,
    remoteDir: '/var/backups/xrayc',
    pubkeyInstalled: false,
    pubkeyFingerprint: '',
    retentionDays: 30,
  },
  full: {
    enabled: true,
    excludeTrafficLogs: false,
    schedule: { kind: 'interval', intervalMinutes: 1440, cron: '' },
    retentionDays: 30,
    destination: 'local',
  },
  wal: {
    enabled: false,
    configured: false,
    fullBackupSchedule: { kind: 'interval', intervalMinutes: 10080, cron: '' },
    retentionFull: 4,
    destination: 'local',
  },
  email: {
    enabled: false,
    recipients: [],
    // 邮件独立调度默认每天（1440 分钟），与全量同格式；后端返回会覆盖。
    schedule: { kind: 'interval', intervalMinutes: 1440, cron: '' },
    attachEncrypted: true,
    attachPassphrase: '',
    attachPassphraseSet: false,
    maxAttachMb: 20,
    notifyOnSuccess: true,
    notifyOnFailure: true,
    oversizeMode: 'notify',
  },
});

function emptyModeState(): BackupModeState {
  return {
    status: 'idle',
    lastStartedAt: '',
    lastSuccessAt: '',
    lastFailedAt: '',
    errorSummary: '',
    lastFileName: '',
    offsiteSynced: false,
    offsiteError: '',
    offsiteLastSyncAt: '',
    emailLastSentAt: '',
    emailAttachmentName: '',
    walArchiveOk: null,
    history: [],
  };
}

const state = reactive<BackupState>({
  full: emptyModeState(),
  wal: emptyModeState(),
  email: emptyModeState(),
});

// 编辑弹窗状态：操作的是 config 的深拷贝草稿，保存才落库、取消丢弃不污染主表。
const editVisible = ref(false);
const editMode = ref<BackupActiveMode | null>(null);
const editDraft = ref<BackupConfig | null>(null);

// 详情弹窗状态：只读展示，直接引用线上 state/config。
const detailVisible = ref(false);
const detailMode = ref<BackupActiveMode | null>(null);

onMounted(load);

async function load() {
  loading.value = true;
  try {
    const [nextConfig, nextState] = await Promise.all([
      apiClient.getBackupConfig(),
      apiClient.getBackupState(),
    ]);
    applyConfig(nextConfig);
    applyState(nextState);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载备份配置失败');
  } finally {
    loading.value = false;
  }
}

// 统一保存入口：source 可为线上 config（服务器卡/开关）或编辑草稿（弹窗）；成功回填脱敏配置。
async function persist(source: BackupConfig): Promise<boolean> {
  saving.value = true;
  try {
    applyConfig(await apiClient.updateBackupConfig({ ...source }));
    ElMessage.success('备份配置已保存');
    return true;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存备份配置失败');
    return false;
  } finally {
    saving.value = false;
  }
}

// 顶部「保存配置」：保存异地服务器共享连接等线上 config 改动。
function saveConfig() {
  void persist(config);
}

// 表格启用开关：切换即改该模式 enabled 并保存整份配置；保存失败则回滚开关。
async function toggleMode(mode: BackupActiveMode, enabled: boolean) {
  config[mode].enabled = enabled;
  if (!(await persist(config))) {
    config[mode].enabled = !enabled;
  }
}

function openEdit(mode: BackupActiveMode) {
  editMode.value = mode;
  // 深拷贝出草稿，弹窗改草稿不动线上 config；密码/口令为空副本，留空=不改。
  editDraft.value = JSON.parse(JSON.stringify(config)) as BackupConfig;
  editVisible.value = true;
}

async function saveEdit() {
  if (!editDraft.value) {
    return;
  }
  if (await persist(editDraft.value)) {
    editVisible.value = false;
  }
}

function openDetail(mode: BackupActiveMode) {
  detailMode.value = mode;
  detailVisible.value = true;
}

async function testAndProvision() {
  testing.value = true;
  try {
    const result = await apiClient.testAndProvisionBackup({ ...config.offsite });
    config.offsite.pubkeyInstalled = result.pubkeyInstalled || config.offsite.pubkeyInstalled;
    if (result.fingerprint) {
      config.offsite.pubkeyFingerprint = result.fingerprint;
    }
    if (result.ok) {
      // 装成功且本次填了密码：清空输入框并标记已设置（后端已落库作兜底）。
      if (config.offsite.sshPassword) {
        config.offsite.sshPasswordSet = true;
        config.offsite.sshPassword = '';
      }
      ElMessage.success(result.message || '测试通过，公钥已装');
    } else {
      ElMessage.error(result.message || '测试失败，请检查 IP/端口/密码');
    }
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '测试并装公钥失败');
  } finally {
    testing.value = false;
  }
}

async function runNow(mode: BackupActiveMode) {
  running[mode] = true;
  try {
    const result = await apiClient.runBackupNow(mode);
    if (result.ok) {
      ElMessage.success(result.message || `${runModeLabel(mode)}已触发`);
    } else {
      ElMessage.warning(result.message || `${runModeLabel(mode)}触发失败`);
    }
    // 触发后刷新状态，反映最新运行态。
    applyState(await apiClient.getBackupState());
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '立即备份失败');
  } finally {
    running[mode] = false;
  }
}

// 就地回填配置（保持子对象引用不变，密码字段永远清空）。
function applyConfig(next: BackupConfig) {
  // 异地段只保留共享连接 + 保留天数（无 schedule/enabled），密码字段永远清空。
  Object.assign(config.offsite, next.offsite, { sshPassword: '' });
  Object.assign(config.full, next.full, { schedule: config.full.schedule });
  Object.assign(config.full.schedule, next.full.schedule);
  Object.assign(config.wal, next.wal, { fullBackupSchedule: config.wal.fullBackupSchedule });
  Object.assign(config.wal.fullBackupSchedule, next.wal.fullBackupSchedule);
  // 邮件与 full/wal 同款:保留 schedule 子对象引用不变、再就地更新内容,
  // 否则替换引用会让 BackupScheduleFields 的绑定/回填失效;密码字段永远清空。
  Object.assign(config.email, next.email, {
    attachPassphrase: '',
    schedule: config.email.schedule,
  });
  Object.assign(config.email.schedule, next.email.schedule);
}

function applyState(next: BackupState) {
  Object.assign(state.full, next.full);
  Object.assign(state.wal, next.wal);
  Object.assign(state.email, next.email);
}
</script>

<template>
  <PageHeader
    title="数据库备份"
    description="以运行状态表为中心管理全量 / WAL / 邮件三类备份；全量与 WAL 可选存储到本机 / 异地 / 本机+异地。每行可开关、编辑、看详情、立即备份。密码与口令留空表示保留原值。"
  >
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :loading="saving" @click="saveConfig">保存配置</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="10" animated />
  <div v-else class="backup-page">
    <BackupOffsiteServerCard :config="config.offsite" :testing="testing" @test="testAndProvision" />
    <BackupModeTable
      :config="config"
      :state="state"
      :running="running"
      :saving="saving"
      @toggle="toggleMode"
      @edit="openEdit"
      @detail="openDetail"
      @run="runNow"
    />
  </div>

  <BackupEditDialog
    v-model="editVisible"
    :mode="editMode"
    :draft="editDraft"
    :saving="saving"
    @save="saveEdit"
  />
  <BackupDetailDialog
    v-model="detailVisible"
    :mode="detailMode"
    :state="state"
    :config="config"
  />
</template>

<style scoped>
.backup-page {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
</style>
