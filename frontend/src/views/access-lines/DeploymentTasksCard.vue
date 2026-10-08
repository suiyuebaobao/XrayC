<!--
  本组件展示 Agent 安装任务的紧凑进度。
  列表按服务器地址聚合，一台服务器只占一行。
  详细步骤、历史任务、失败原因和安装结果放在详情弹窗中。
  SSH 凭据不持久化；失败重试只回填非敏感安装参数。
-->
<script setup lang="ts">
import { computed, ref } from 'vue';
import type { DeploymentTask } from '@/services/api';

const props = defineProps<{
  tasks: DeploymentTask[];
}>();

const emit = defineEmits<{
  (event: 'retry', task: DeploymentTask): void;
  // 删除/取消部署任务：未完成任务即取消，已结束任务即清理列表残留。
  (event: 'delete', task: DeploymentTask): void;
}>();

type DeploymentTaskRow = {
  key: string;
  targetName: string;
  targetAddress: string;
  latest: DeploymentTask;
  tasks: DeploymentTask[];
  stale: boolean;
};

const selectedRow = ref<DeploymentTaskRow | null>(null);
const detailDialogOpen = ref(false);

const taskRows = computed<DeploymentTaskRow[]>(() => {
  const grouped = new Map<string, DeploymentTask[]>();
  const sortedTasks = [...props.tasks]
    .sort((left, right) => timestamp(right.updatedAt) - timestamp(left.updatedAt));
  sortedTasks.forEach((task) => {
    const key = taskGroupKey(task);
    grouped.set(key, [...(grouped.get(key) ?? []), task]);
  });
  return Array.from(grouped.entries()).flatMap(([key, tasks]) => {
    const latest = tasks[0];
    if (latest.status === 'succeeded') {
      return [];
    }
    const visibleTasks = tasks.filter((task) => task.status !== 'succeeded');
    return {
      key,
      targetName: targetName(latest),
      targetAddress: targetAddress(latest),
      latest,
      tasks: visibleTasks,
      stale: isStaleRunning(latest),
    };
  });
});

function statusLabel(status: DeploymentTask['status'], task?: DeploymentTask) {
  if (status === 'waiting_for_server') {
    return task && metadataText(task, 'mode') === 'one_click' ? '准备安装' : '等待服务器上报';
  }
  const labels: Record<DeploymentTask['status'], string> = {
    waiting_for_server: '等待服务器上报',
    running: '安装中',
    succeeded: '已完成',
    failed: '失败',
    canceled: '已取消',
    unknown: '未知',
  };
  return labels[status] ?? '未知';
}

function rowStatusLabel(row: DeploymentTaskRow) {
  return row.stale ? '安装超时' : statusLabel(row.latest.status, row.latest);
}

function statusType(status: DeploymentTask['status'] | 'stale') {
  if (status === 'succeeded') {
    return 'success';
  }
  if (status === 'failed' || status === 'stale') {
    return 'danger';
  }
  if (status === 'running') {
    return 'warning';
  }
  return 'info';
}

function rowStatusType(row: DeploymentTaskRow) {
  return statusType(row.stale ? 'stale' : row.latest.status);
}

function progressStatus(row: DeploymentTaskRow) {
  if (row.stale || row.latest.status === 'failed') {
    return 'exception';
  }
  if (row.latest.status === 'succeeded') {
    return 'success';
  }
  return undefined;
}

function metadataText(task: DeploymentTask, key: string) {
  const value = task.safeMetadata[key];
  if (value === null || value === undefined) {
    return '';
  }
  return String(value);
}

function metadataBool(task: DeploymentTask, key: string) {
  return task.safeMetadata[key] === true;
}

function installModeLabel(task: DeploymentTask) {
  return metadataText(task, 'mode') === 'one_click' ? '一键 SSH 安装' : '手动安装说明';
}

function authMethodLabel(task: DeploymentTask) {
  const method = metadataText(task, 'ssh_auth_method');
  if (method === 'private_key') {
    return '私钥';
  }
  if (method === 'password') {
    return '密码';
  }
  return method || '-';
}

function targetName(task: DeploymentTask) {
  return metadataText(task, 'access_node_name') || task.title || 'Agent 安装';
}

function targetAddress(task: DeploymentTask) {
  const host = metadataText(task, 'public_host');
  const port = metadataText(task, 'public_port');
  if (!host) {
    return '-';
  }
  return port ? `${host}:${port}` : host;
}

function taskGroupKey(task: DeploymentTask) {
  const address = targetAddress(task);
  if (address !== '-') {
    return `address:${address}`;
  }
  const nodeId = metadataText(task, 'access_node_id') || task.targetId;
  return nodeId ? `node:${nodeId}` : `task:${task.id}`;
}

function currentStepTitle(task: DeploymentTask) {
  const step = task.steps.find((item) => item.key === task.currentStep);
  return step?.title || task.currentStep || '等待上报';
}

function stepTagType(status: string) {
  if (status === 'done' || status === 'succeeded') {
    return 'success';
  }
  if (status === 'current' || status === 'running') {
    return 'warning';
  }
  if (status === 'failed') {
    return 'danger';
  }
  if (status === 'canceled') {
    return 'info';
  }
  return 'info';
}

function stepStatusLabel(status: string) {
  const labels: Record<string, string> = {
    done: '已完成',
    current: '进行中',
    pending: '等待系统处理',
    failed: '失败',
    canceled: '已取消',
    running: '进行中',
    succeeded: '已完成',
  };
  return labels[status] ?? '未知';
}

function failedStepTitle(task: DeploymentTask) {
  return task.steps.find((step) => step.status === 'failed')?.title || currentStepTitle(task);
}

function failureSuggestions(task: DeploymentTask) {
  const step = task.currentStep;
  if (step.includes('ssh')) {
    return ['检查 SSH 地址、端口、用户名、密码或私钥', '确认服务器防火墙允许 SSH 连接'];
  }
  if (step.includes('artifact')) {
    return ['检查服务器能否访问管理平台下载地址', '确认 DEPLOY_ARTIFACT_TOKEN 未过期'];
  }
  if (step.includes('auth_code')) {
    return ['检查远端安装命令是否完整执行', '查看服务器 access-agent 容器是否启动'];
  }
  if (step.includes('node_register')) {
    return ['检查节点鉴权码是否有效', '确认中转节点地址和端口没有冲突'];
  }
  return ['先查看详细错误，修复原因后再重试；安装文件预检通过前保留现有服务'];
}

function resultRows(task: DeploymentTask) {
  const result = task.result;
  const rows: Array<{ label: string; value: string }> = [];
  const accessNodeId = result.access_node_id ?? result.accessNodeId;
  const authCodeReceived = result.auth_code_received ?? result.authCodeReceived;
  const stderrSummary = result.stderr_summary ?? result.stderrSummary;
  if (accessNodeId) {
    rows.push({ label: '中转节点 ID', value: String(accessNodeId) });
  }
  if (authCodeReceived !== undefined) {
    rows.push({ label: '鉴权码', value: authCodeReceived ? '已收到' : '未收到' });
  }
  if (stderrSummary) {
    rows.push({ label: '安装摘要', value: String(stderrSummary) });
  }
  return rows;
}

function timestamp(value: string) {
  const time = Date.parse(value);
  return Number.isFinite(time) ? time : 0;
}

function isStaleRunning(task: DeploymentTask) {
  if (task.status !== 'running') {
    return false;
  }
  const updated = timestamp(task.updatedAt);
  if (!updated) {
    return false;
  }
  return Date.now() - updated > 35 * 60 * 1000;
}

function canRetry(row: DeploymentTaskRow) {
  return row.latest.status === 'failed' || row.stale;
}

// 未结束（等待/安装中/卡死）的任务删除即「取消」，已结束的删除即「清理」。
function isUnfinished(task: DeploymentTask) {
  return task.status === 'waiting_for_server' || task.status === 'running';
}

function deleteActionLabel(row: DeploymentTaskRow) {
  return isUnfinished(row.latest) && !row.stale ? '取消' : '删除';
}

function removeTask(task: DeploymentTask) {
  detailDialogOpen.value = false;
  emit('delete', task);
}

function openDetails(row: DeploymentTaskRow) {
  selectedRow.value = row;
  detailDialogOpen.value = true;
}

function retryInstall(task: DeploymentTask) {
  detailDialogOpen.value = false;
  emit('retry', task);
}
</script>

<template>
  <el-card shadow="never" class="deployment-tasks-card">
    <template #header>
      <div class="deployment-tasks-card__header">
        <strong>部署任务</strong>
        <span>一台服务器一行，点击地址查看详情</span>
      </div>
    </template>
    <el-empty v-if="taskRows.length === 0" description="暂无部署任务" />
    <el-table v-else :data="taskRows" class="deployment-tasks-table">
      <el-table-column label="服务器地址" min-width="220">
        <template #default="{ row }">
          <el-button link type="primary" class="address-button" @click="openDetails(row)">
            {{ row.targetAddress }}
          </el-button>
          <div class="task-meta">{{ row.targetName }}</div>
        </template>
      </el-table-column>
      <el-table-column label="状态" width="130">
        <template #default="{ row }">
          <el-tag :type="rowStatusType(row)" effect="plain">{{ rowStatusLabel(row) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="当前进度" min-width="220">
        <template #default="{ row }">
          <div class="progress-cell">
            <el-progress :percentage="row.latest.progressPercent" :status="progressStatus(row)" />
            <div class="task-meta">{{ currentStepTitle(row.latest) }}</div>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="历史" width="110">
        <template #default="{ row }">
          <el-tag effect="plain">{{ row.tasks.length }} 条历史</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="更新时间" min-width="180">
        <template #default="{ row }">
          {{ row.latest.updatedAt }}
        </template>
      </el-table-column>
      <el-table-column label="操作" width="220" fixed="right">
        <template #default="{ row }">
          <el-button v-if="canRetry(row)" type="primary" link @click="retryInstall(row.latest)">重试安装</el-button>
          <el-button link @click="openDetails(row)">详情</el-button>
          <el-button type="danger" link @click="removeTask(row.latest)">{{ deleteActionLabel(row) }}</el-button>
        </template>
      </el-table-column>
    </el-table>
  </el-card>

  <el-dialog v-model="detailDialogOpen" title="部署详情" width="900px">
    <template v-if="selectedRow">
      <div class="task-detail">
        <div class="detail-head">
          <div>
            <strong>{{ selectedRow.targetAddress }}</strong>
            <div class="task-meta">{{ selectedRow.targetName }} · {{ selectedRow.tasks.length }} 条历史</div>
          </div>
          <div class="detail-actions">
            <el-button v-if="canRetry(selectedRow)" type="primary" @click="retryInstall(selectedRow.latest)">
              重试安装
            </el-button>
            <el-button type="danger" plain @click="removeTask(selectedRow.latest)">
              {{ deleteActionLabel(selectedRow) }}
            </el-button>
          </div>
        </div>

        <el-alert
          v-if="selectedRow.stale"
          type="error"
          show-icon
          :closable="false"
          title="该任务长时间没有更新，请先检查节点状态及错误详情，再决定是否重试。"
        />

        <section v-for="task in selectedRow.tasks" :key="task.id" class="task-history">
          <div class="task-history__head">
            <div>
              <strong>{{ task.summary || task.title }}</strong>
              <div class="task-meta">{{ task.updatedAt }}</div>
            </div>
            <el-tag :type="statusType(task.status)" effect="plain">{{ statusLabel(task.status, task) }}</el-tag>
          </div>

          <el-descriptions :column="2" border size="small">
            <el-descriptions-item label="节点">{{ targetName(task) }}</el-descriptions-item>
            <el-descriptions-item label="地址">{{ targetAddress(task) }}</el-descriptions-item>
            <el-descriptions-item label="方式">{{ installModeLabel(task) }}</el-descriptions-item>
            <el-descriptions-item label="SSH 端口">{{ metadataText(task, 'ssh_port') || '-' }}</el-descriptions-item>
            <el-descriptions-item label="SSH 鉴权">{{ authMethodLabel(task) }}</el-descriptions-item>
            <el-descriptions-item label="安装目录">{{ metadataText(task, 'install_dir') || '-' }}</el-descriptions-item>
            <el-descriptions-item label="Compose">{{ metadataText(task, 'compose_project') || '-' }}</el-descriptions-item>
            <el-descriptions-item label="强制重装">
              {{ metadataBool(task, 'force_reinstall') ? '是' : '否' }}
            </el-descriptions-item>
          </el-descriptions>

          <div class="task-detail__section">
            <div class="task-detail__title">安装步骤</div>
            <el-timeline>
              <el-timeline-item
                v-for="step in task.steps"
                :key="step.key"
                :type="stepTagType(step.status)"
                :hollow="step.status === 'pending'"
              >
                <div class="task-step">
                  <div class="task-step__head">
                    <strong>{{ step.title || step.key }}</strong>
                    <el-tag size="small" :type="stepTagType(step.status)" effect="plain">
                      {{ stepStatusLabel(step.status) }}
                    </el-tag>
                  </div>
                  <div class="task-meta">{{ step.detail }}</div>
                </div>
              </el-timeline-item>
            </el-timeline>
          </div>

          <div v-if="task.status === 'failed'" class="task-detail__section task-failure">
            <div class="task-detail__title">失败原因</div>
            <p><strong>失败步骤：</strong>{{ failedStepTitle(task) }}</p>
            <p><strong>错误摘要：</strong>{{ task.errorSummary || '后端未返回错误摘要' }}</p>
            <details v-if="typeof task.result.error === 'string' && task.result.error" class="task-error-details">
              <summary>查看详细错误</summary>
              <pre>{{ task.result.error }}</pre>
            </details>
            <ul>
              <li v-for="item in failureSuggestions(task)" :key="item">{{ item }}</li>
            </ul>
          </div>

          <div v-if="resultRows(task).length > 0" class="task-detail__section">
            <div class="task-detail__title">安装结果</div>
            <dl class="result-list">
              <template v-for="item in resultRows(task)" :key="item.label">
                <dt>{{ item.label }}</dt>
                <dd>{{ item.value }}</dd>
              </template>
            </dl>
          </div>
        </section>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped src="../../styles/deployment-tasks.css"></style>
