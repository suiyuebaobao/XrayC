// 本文件负责后台审计日志响应标准化。
// 它保留后端返回的请求摘要原文，不在前端替换字段内容。
// 本文件不发送请求，只把任意后端日志对象转换为 AdminAuditLog。
// 新增审计字段时，应在这里补齐兼容映射并保持展示字段稳定。

import type { AdminAuditLog } from '../types';
import { idValue, isRecord, recordValue, textValue } from '../primitives';

export function normalizeAdminAuditLog(value: unknown): AdminAuditLog {
  const data = recordValue(value);
  const actor = recordValue(data.actor);
  const resourceType = textValue(data.resourceType ?? data.resource_type ?? data.targetType ?? data.target_type);
  const resourceName = textValue(data.resourceName ?? data.resource_name ?? data.targetName ?? data.target_name);
  const resourceId = textValue(data.resourceId ?? data.resource_id ?? data.targetId ?? data.target_id);

  return {
    id: idValue(data.id ?? data.audit_id),
    createdAt: textValue(data.createdAt ?? data.created_at ?? data.time ?? data.occurred_at),
    actor: textValue(
      data.actorAccount
      ?? data.actor_account
      ?? data.actorEmail
      ?? data.actor_email
      ?? data.actorName
      ?? data.actor_name
      ?? actor.account
      ?? actor.email
      ?? actor.name,
    ) || auditActorFallback(data.actorUserId ?? data.actor_user_id ?? actor.id),
    action: textValue(data.action ?? data.event ?? data.operation) || 'unknown',
    resource: formatAuditResource(resourceType, resourceName || resourceId),
    result: textValue(data.result ?? data.status ?? data.outcome) || 'unknown',
    summary: normalizeAuditSummary(data.requestSummary ?? data.request_summary ?? data.summary ?? data.details),
  };
}

function auditActorFallback(value: unknown) {
  const id = textValue(value);
  return id ? `用户 ${id}` : '未知操作者';
}

function formatAuditResource(type: string, idOrName: string) {
  if (type && idOrName) {
    return `${type}:${idOrName}`;
  }
  return type || idOrName || '-';
}

function normalizeAuditSummary(value: unknown) {
  return truncateAuditSummary(auditSummaryText(value) || '-');
}

function auditSummaryText(value: unknown): string {
  if (typeof value === 'string') {
    return value.trim();
  }

  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }

  if (Array.isArray(value)) {
    return value.map(auditSummaryText).filter(Boolean).join('，');
  }

  if (isRecord(value)) {
    return Object.entries(value)
      .map(([key, item]) => {
        const text = auditSummaryText(item);
        return text ? `${formatAuditSummaryKey(key)}=${text}` : '';
      })
      .filter(Boolean)
      .join('；');
  }

  return '';
}

function formatAuditSummaryKey(value: string) {
  return value.replace(/[_-]+/g, ' ');
}

function truncateAuditSummary(value: string) {
  return value.length > 300 ? `${value.slice(0, 300)}...` : value;
}
