// 本文件提供健康检查页面专用的格式化函数。
// 它只处理状态标签、时间、速率、数量和线路地址展示。
// 这里不请求接口、不修改数据，避免页面组件重复实现格式化。
// 所有文案使用中文，方便后台运营人员直接理解。
import type { AccessLine, HealthStatus, OperationsSummary } from '@/services/api';

const numberFormatter = new Intl.NumberFormat('zh-CN');
const rateFormatter = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 });
const bytesFormatter = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 });

export function healthType(status: HealthStatus) {
  if (status === 'healthy') {
    return 'success';
  }
  if (status === 'syncing' || status === 'stale' || status === 'degraded') {
    return 'warning';
  }
  return status === 'offline' ? 'danger' : 'info';
}

export function healthLabel(status: HealthStatus) {
  const labels: Record<HealthStatus, string> = {
    healthy: '健康',
    syncing: '同步中',
    stale: '过期',
    degraded: '降级',
    offline: '离线',
    unknown: '未知',
  };
  return labels[status];
}

export function runtimeStatusLabel(status: OperationsSummary['runtimeMetricStatus']) {
  const labels: Record<OperationsSummary['runtimeMetricStatus'], string> = {
    fresh: '实时',
    stale: '过期',
    no_data: '未上报',
    unknown: '未知',
  };
  return labels[status];
}

export function probeStatusLabel(status: string) {
  const normalized = (status || '').toLowerCase();
  if (normalized === 'healthy') {
    return '探针健康';
  }
  if (normalized === 'unhealthy') {
    return '探针异常';
  }
  if (normalized === 'timeout') {
    return '探针超时';
  }
  if (normalized === 'error') {
    return '探针错误';
  }
  return '探针未上报';
}

export function formatNumber(value: number | null | undefined) {
  return value === null || value === undefined ? '未知' : numberFormatter.format(value);
}

export function formatRate(value: number | null | undefined) {
  if (value === null || value === undefined) {
    return '未上报';
  }
  if (value < 1000) {
    return `${formatNumber(value)} bps`;
  }
  const units = ['Kbps', 'Mbps', 'Gbps', 'Tbps'];
  let scaled = value;
  let unitIndex = -1;
  do {
    scaled /= 1000;
    unitIndex += 1;
  } while (scaled >= 1000 && unitIndex < units.length - 1);
  return `${rateFormatter.format(scaled)} ${units[unitIndex]}`;
}

export function formatBytes(value: number | null | undefined) {
  if (value === null || value === undefined) {
    return '未知';
  }
  if (value < 1024) {
    return `${formatNumber(value)} B`;
  }
  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let scaled = value;
  let unitIndex = -1;
  do {
    scaled /= 1024;
    unitIndex += 1;
  } while (scaled >= 1024 && unitIndex < units.length - 1);
  return `${bytesFormatter.format(scaled)} ${units[unitIndex]}`;
}

export function formatTime(value: string | undefined, fallback = '未上报') {
  if (!value) {
    return fallback;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString('zh-CN', { hour12: false });
}

export function ageSeconds(value: string, fallback: number | null) {
  if (fallback !== null && fallback !== undefined) {
    return fallback;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  return Math.max(0, Math.round((Date.now() - date.getTime()) / 1000));
}

export function formatAge(value: string, fallback: number | null) {
  const age = value ? ageSeconds(value, fallback) : null;
  if (age === null) {
    return '未上报';
  }
  if (age < 60) {
    return `${age} 秒前`;
  }
  if (age < 3600) {
    return `${Math.round(age / 60)} 分钟前`;
  }
  return `${Math.round(age / 3600)} 小时前`;
}

export function lineEndpoint(line: AccessLine) {
  if (line.listenHost && line.listenPort) {
    return `${line.listenHost}:${line.listenPort}`;
  }
  return line.listenHost || `端口 ${line.listenPort}`;
}
