// 本文件负责 Agent 安装和部署任务响应标准化。
// 从 routing normalizer 拆出后，路由 normalizer 继续 re-export 这些函数。
// 这里只处理字段兼容和状态归一，不发请求、不读取页面状态。
// 部署任务状态枚举要和后端 deployment_tasks 表保持一致。
// 安装说明和一键安装都允许 task 为空，前端按 null 展示。

import type {
  AccessNodeOneClickInstallResult,
  AgentInstallGuideResult,
  DeploymentTask,
  DeploymentTasksResult,
} from '../types';
import {
  arrayValue,
  numberValue,
  recordValue,
  stringValue,
} from '../primitives';

export function normalizeAgentInstallGuideResult(value: unknown): AgentInstallGuideResult {
  const data = recordValue(value);
  const status = stringValue(data.status);
  return {
    guideId: stringValue(data.guideId ?? data.guide_id ?? data.id),
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    accessNodeName: stringValue(data.accessNodeName ?? data.access_node_name),
    status: status === 'ready'
      ? status
      : 'unknown',
    summary: stringValue(data.summary ?? data.message),
    installDir: stringValue(data.installDir ?? data.install_dir),
    composeProject: stringValue(data.composeProject ?? data.compose_project),
    environmentText: stringValue(data.environmentText ?? data.environment_text),
    installCommand: stringValue(data.installCommand ?? data.install_command),
    task: data.task ? normalizeDeploymentTask(data.task) : null,
    reportToken: stringValue(data.reportToken ?? data.report_token),
    steps: arrayValue(data.steps).map((step) => {
      const item = recordValue(step);
      return {
        title: stringValue(item.title ?? item.name ?? item.step),
        detail: stringValue(item.detail ?? item.message),
        command: stringValue(item.command),
      };
    }),
  };
}

export function normalizeAccessNodeOneClickInstallResult(value: unknown): AccessNodeOneClickInstallResult {
  const data = recordValue(value);
  return {
    status: stringValue(data.status) || 'unknown',
    accessNodeId: stringValue(data.accessNodeId ?? data.access_node_id),
    task: data.task ? normalizeDeploymentTask(data.task) : null,
  };
}

export function normalizeDeploymentTasksResult(value: unknown): DeploymentTasksResult {
  const data = recordValue(value);
  return {
    items: arrayValue(data.items ?? data.deployment_tasks ?? data.tasks).map(normalizeDeploymentTask),
  };
}

export function normalizeDeploymentTask(value: unknown): DeploymentTask {
  const data = recordValue(value);
  return {
    id: stringValue(data.id),
    kind: stringValue(data.kind),
    status: normalizeDeploymentTaskStatus(data.status),
    targetType: stringValue(data.targetType ?? data.target_type),
    targetId: stringValue(data.targetId ?? data.target_id),
    title: stringValue(data.title),
    summary: stringValue(data.summary),
    currentStep: stringValue(data.currentStep ?? data.current_step),
    progressPercent: numberValue(data.progressPercent ?? data.progress_percent),
    safeMetadata: recordValue(data.safeMetadata ?? data.safe_metadata),
    steps: arrayValue(data.steps).map((step) => {
      const item = recordValue(step);
      return {
        key: stringValue(item.key),
        title: stringValue(item.title),
        detail: stringValue(item.detail),
        status: stringValue(item.status),
      };
    }),
    result: recordValue(data.result),
    errorSummary: stringValue(data.errorSummary ?? data.error_summary),
    createdAt: stringValue(data.createdAt ?? data.created_at),
    updatedAt: stringValue(data.updatedAt ?? data.updated_at),
    startedAt: stringValue(data.startedAt ?? data.started_at),
    completedAt: stringValue(data.completedAt ?? data.completed_at),
  };
}

function normalizeDeploymentTaskStatus(value: unknown): DeploymentTask['status'] {
  const status = stringValue(value);
  if (
    status === 'waiting_for_server'
    || status === 'running'
    || status === 'succeeded'
    || status === 'failed'
    || status === 'canceled'
  ) {
    return status;
  }
  return 'unknown';
}
