// 本文件集中提供运营中心页面的展示格式化函数。
// 这些函数只处理数字、时间、状态标签和入口文案转换。
// 页面 composable 与局部组件共用这里的纯函数，避免重复实现。
// 本文件不发起接口请求，也不持有页面响应式状态。
import type {
  AccessLine,
  ExitEndpointSummary,
  OperationsEvent,
  OperationsLedgerRankingItem,
  RuntimeMetricStatus,
} from '@/services/api';

const numberFormatter = new Intl.NumberFormat('zh-CN');
const rateFormatter = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 });
const byteFormatter = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 2 });

export function formatKnownNumber(value: number | null | undefined) {
  return value === null || value === undefined ? '未知' : numberFormatter.format(value);
}

export function formatReportedNumber(value: number | null | undefined) {
  return value === null || value === undefined ? '未上报' : numberFormatter.format(value);
}

export function formatRuntimeRate(value: number | null | undefined, status: RuntimeMetricStatus) {
  if (status === 'no_data') {
    return '未上报';
  }
  if (status === 'unknown' && !isReportedNumber(value)) {
    return '未知';
  }
  return formatLineRate(value);
}

export function formatRuntimeNumber(value: number | null | undefined, status: RuntimeMetricStatus) {
  if (status === 'no_data') {
    return '未上报';
  }
  if (status === 'unknown') {
    return '未知';
  }
  return formatReportedNumber(value);
}

export function formatLineRate(value: number | null | undefined) {
  if (!isReportedNumber(value)) {
    return '未上报';
  }

  if (value < 1000) {
    return `${numberFormatter.format(value)} bps`;
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

export function formatProbeLatency(value: number | null | undefined) {
  if (value === null || value === undefined) {
    return '延迟未上报';
  }
  return value <= 0 ? '< 1 ms' : `${numberFormatter.format(value)} ms`;
}

export function formatTraffic(value: number | null | undefined) {
  return value === null || value === undefined ? '未知' : `${numberFormatter.format(value)} GB`;
}

export function formatBytes(value: number | null | undefined) {
  if (!isReportedNumber(value)) {
    return '未知';
  }
  if (value < 1024) {
    return `${numberFormatter.format(value)} B`;
  }

  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let scaled = value;
  let unitIndex = -1;
  do {
    scaled /= 1024;
    unitIndex += 1;
  } while (scaled >= 1024 && unitIndex < units.length - 1);
  return `${byteFormatter.format(scaled)} ${units[unitIndex]}`;
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

export function eventType(level: OperationsEvent['level']) {
  return level === 'danger' ? 'danger' : level === 'warning' ? 'warning' : 'info';
}

export function eventTitle(event: OperationsEvent) {
  return event.title || '未上报';
}

export function runtimeMetricTagType(status: RuntimeMetricStatus) {
  if (status === 'fresh') {
    return 'success';
  }
  if (status === 'stale') {
    return 'warning';
  }
  return 'info';
}

export function runtimeStatusLabel(status: RuntimeMetricStatus) {
  if (status === 'fresh') {
    return '实时';
  }
  if (status === 'stale') {
    return '旧快照';
  }
  if (status === 'no_data') {
    return '未上报';
  }
  return '未知';
}

export function probeStatusLabel(status: string | null | undefined) {
  const normalized = (status ?? '').toLowerCase();
  if (!normalized) {
    return '未上报';
  }
  if (normalized === 'healthy') {
    return '健康';
  }
  if (normalized === 'unhealthy') {
    return '异常';
  }
  if (normalized === 'timeout') {
    return '超时';
  }
  if (normalized === 'error') {
    return '错误';
  }
  if (normalized === 'unknown') {
    return '未知';
  }
  return status ?? '未上报';
}

export function probeTagType(status: string | null | undefined) {
  const normalized = (status ?? '').toLowerCase();
  if (normalized === 'healthy') {
    return 'success';
  }
  if (normalized === 'unhealthy' || normalized === 'timeout' || normalized === 'error') {
    return 'danger';
  }
  return 'info';
}

export function exitMemberStatusLabel(member: ExitEndpointSummary) {
  if (!member.statusKnown) {
    return member.healthy ? '健康（状态未回显）' : '异常（状态未回显）';
  }

  const labels: Record<ExitEndpointSummary['status'], string> = {
    healthy: '健康',
    degraded: '降级',
    draining: '排空',
    offline: '离线',
    unknown: '未知',
  };

  return labels[member.status];
}

export function exitMemberStatusType(member: ExitEndpointSummary) {
  if (!member.statusKnown) {
    return member.healthy ? 'success' : 'warning';
  }
  if (member.status === 'healthy') {
    return 'success';
  }
  if (member.status === 'degraded' || member.status === 'draining') {
    return 'warning';
  }
  return member.status === 'offline' ? 'danger' : 'info';
}

export function lineEndpoint(line: AccessLine) {
  if (!line.listenHost && !line.listenPort) {
    return '中转入口未上报';
  }
  if (!line.listenHost) {
    return `端口 ${line.listenPort}`;
  }
  if (!line.listenPort) {
    return line.listenHost;
  }
  return `${line.listenHost}:${line.listenPort}`;
}

export function rankingEndpoint(row: OperationsLedgerRankingItem) {
  if (!row.listenHost && !row.listenPort) {
    return '中转入口未上报';
  }
  if (!row.listenHost) {
    return `端口 ${row.listenPort}`;
  }
  if (!row.listenPort) {
    return row.listenHost;
  }
  return `${row.listenHost}:${row.listenPort}`;
}

export function dirtyNodeHint(value: number | null) {
  if (value === null) {
    return '配置状态未知';
  }

  return value > 0 ? '等待节点运行组件拉取' : '配置已同步';
}

export function dirtyTagType(value: number | null) {
  if (value === null) {
    return 'info';
  }

  return value > 0 ? 'warning' : 'success';
}

export function appendHint(primary: string, secondary: string) {
  return secondary ? `${primary}；${secondary}` : primary;
}

export function isReportedNumber(value: number | null | undefined): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

export function sumReported(values: Array<number | null | undefined>) {
  let hasReportedValue = false;
  const total = values.reduce<number>((sum, value) => {
    if (!isReportedNumber(value)) {
      return sum;
    }
    hasReportedValue = true;
    return sum + value;
  }, 0);
  return hasReportedValue ? total : null;
}
