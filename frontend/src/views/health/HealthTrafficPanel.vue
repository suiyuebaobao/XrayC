<!--
  本文件展示单一健康分类的业务流量看板。
  它用于中转节点、出口线路和分组三个标签页。
  数据全部来自 operations summary 的 traffic_health 聚合字段。
  页面只保留日周月、峰值、低谷、累计和当月累计。
  图形使用轻量 SVG，不引入额外图表库。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type { TrafficEntitySummary } from '@/services/api';
import { formatBytes, formatTime } from '@/views/health/format';

const props = defineProps<{
  kind: 'node' | 'exit' | 'group' | 'line';
  title: string;
  description: string;
  items: TrafficEntitySummary[];
  generatedAt: string;
}>();

const chartWidth = 720;
const chartHeight = 170;
const chartPadding = { top: 16, right: 18, bottom: 28, left: 46 };

const sortedItems = computed(() => (
  [...props.items].sort((left, right) => (
    right.monthRealBytes - left.monthRealBytes
    || right.totalRealBytes - left.totalRealBytes
    || left.name.localeCompare(right.name, 'zh-CN')
  ))
));

const overview = computed(() => {
  const values = sortedItems.value;
  return {
    today: sumBy(values, 'todayRealBytes'),
    week: sumBy(values, 'weekRealBytes'),
    month: sumBy(values, 'monthRealBytes'),
    monthBilled: sumBy(values, 'monthBilledBytes'),
    total: sumBy(values, 'totalRealBytes'),
    peak: maxBy(values, 'peakHourRealBytes'),
    low: minPositive(values.map((item) => item.lowHourRealBytes)),
  };
});

const trendPoints = computed(() => {
  const grouped = new Map<string, number>();
  for (const item of sortedItems.value) {
    for (const point of item.chart) {
      grouped.set(point.windowStart, (grouped.get(point.windowStart) ?? 0) + point.realBytes);
    }
  }
  return Array.from(grouped.entries())
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([windowStart, realBytes]) => ({ windowStart, realBytes }));
});

const metricCards = computed(() => [
  { label: '今日', value: overview.value.today },
  { label: '本周', value: overview.value.week },
  { label: '本月', value: overview.value.month },
  { label: '峰值', value: overview.value.peak },
  { label: '低谷', value: overview.value.low },
  { label: '总流量', value: overview.value.total },
  { label: '当月总流量', value: overview.value.monthBilled },
]);

function sumBy(items: TrafficEntitySummary[], key: keyof TrafficEntitySummary) {
  return items.reduce((sum, item) => sum + Number(item[key] || 0), 0);
}

function maxBy(items: TrafficEntitySummary[], key: keyof TrafficEntitySummary) {
  return items.reduce((max, item) => Math.max(max, Number(item[key] || 0)), 0);
}

function minPositive(values: number[]) {
  const positive = values.filter((value) => value > 0);
  return positive.length ? Math.min(...positive) : 0;
}

function itemShare(item: TrafficEntitySummary) {
  if (overview.value.month <= 0) {
    return 0;
  }
  return Math.min(100, Math.round((item.monthRealBytes / overview.value.month) * 100));
}

function trendLine() {
  if (trendPoints.value.length === 0) {
    return '';
  }
  const max = Math.max(...trendPoints.value.map((point) => point.realBytes), 1);
  const width = chartWidth - chartPadding.left - chartPadding.right;
  const height = chartHeight - chartPadding.top - chartPadding.bottom;
  const denominator = Math.max(trendPoints.value.length - 1, 1);
  return trendPoints.value
    .map((point, index) => {
      const x = chartPadding.left + (index / denominator) * width;
      const y = chartPadding.top + height - (point.realBytes / max) * height;
      return `${round(x)},${round(y)}`;
    })
    .join(' ');
}

function trendArea() {
  const line = trendLine();
  if (!line) {
    return '';
  }
  return `${chartPadding.left},${chartHeight - chartPadding.bottom} ${line} ${chartWidth - chartPadding.right},${chartHeight - chartPadding.bottom}`;
}

function trendTicks() {
  if (trendPoints.value.length === 0) {
    return [];
  }
  const indexes = Array.from(new Set([0, Math.floor((trendPoints.value.length - 1) / 2), trendPoints.value.length - 1]));
  const width = chartWidth - chartPadding.left - chartPadding.right;
  const denominator = Math.max(trendPoints.value.length - 1, 1);
  return indexes.map((index) => ({
    x: round(chartPadding.left + (index / denominator) * width),
    label: shortTime(trendPoints.value[index].windowStart),
  }));
}

function shortTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' });
}

function round(value: number) {
  return Number(value.toFixed(2));
}
</script>

<template>
  <section class="traffic-panel" :data-testid="`health-${kind}-traffic`">
    <div class="traffic-panel__hero">
      <div>
        <p class="traffic-panel__eyebrow">更新 {{ formatTime(generatedAt, '刚刚') }}</p>
        <h2>{{ title }}</h2>
        <span>{{ description }}</span>
      </div>
      <strong>{{ formatBytes(overview.month) }}</strong>
    </div>

    <div class="traffic-panel__metrics">
      <article v-for="metric in metricCards" :key="metric.label">
        <span>{{ metric.label }}</span>
        <strong>{{ formatBytes(metric.value) }}</strong>
      </article>
    </div>

    <div class="traffic-panel__body">
      <article class="traffic-panel__chart">
        <div class="traffic-panel__section-title">
          <strong>趋势</strong>
          <span>按日聚合真实流量</span>
        </div>
        <svg
          v-if="trendPoints.length > 0"
          :viewBox="`0 0 ${chartWidth} ${chartHeight}`"
          role="img"
          :aria-label="`${title}趋势图`"
        >
          <line
            v-for="index in 4"
            :key="index"
            :x1="chartPadding.left"
            :x2="chartWidth - chartPadding.right"
            :y1="chartPadding.top + ((index - 1) / 3) * (chartHeight - chartPadding.top - chartPadding.bottom)"
            :y2="chartPadding.top + ((index - 1) / 3) * (chartHeight - chartPadding.top - chartPadding.bottom)"
          />
          <polygon :points="trendArea()" />
          <polyline :points="trendLine()" />
          <text v-for="tick in trendTicks()" :key="tick.x" :x="tick.x" :y="chartHeight - 8">{{ tick.label }}</text>
        </svg>
        <el-empty v-else description="暂无趋势数据" />
      </article>

      <article class="traffic-panel__rank">
        <div class="traffic-panel__section-title">
          <strong>排行</strong>
          <span>按当月真实流量排序</span>
        </div>
        <el-empty v-if="sortedItems.length === 0" description="暂无流量数据" />
        <ul v-else>
          <li v-for="item in sortedItems.slice(0, 8)" :key="item.id">
            <div>
              <strong>{{ item.name || '未命名' }}</strong>
              <span>本月 {{ formatBytes(item.monthRealBytes) }} / 累计 {{ formatBytes(item.totalRealBytes) }}</span>
            </div>
            <em>{{ itemShare(item) }}%</em>
            <i :style="{ width: `${itemShare(item)}%` }" />
          </li>
        </ul>
      </article>
    </div>
  </section>
</template>

<style scoped>
.traffic-panel {
  display: grid;
  gap: 18px;
  padding: 22px;
  border: 1px solid rgba(220, 228, 242, 0.9);
  border-radius: 22px;
  background:
    linear-gradient(135deg, rgba(255, 255, 255, 0.96), rgba(245, 250, 255, 0.9)),
    radial-gradient(circle at top right, rgba(42, 128, 255, 0.14), transparent 38%);
  box-shadow: 0 18px 48px rgba(31, 48, 82, 0.08);
}

.traffic-panel__hero,
.traffic-panel__section-title {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.traffic-panel__eyebrow,
.traffic-panel__hero span,
.traffic-panel__section-title span,
.traffic-panel__rank span,
.traffic-panel__metrics span {
  color: #697386;
}

.traffic-panel__eyebrow {
  margin: 0 0 8px;
  font-size: 12px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.traffic-panel__hero h2 {
  margin: 0;
  color: #182033;
  font-size: 28px;
  letter-spacing: -0.03em;
}

.traffic-panel__hero strong {
  color: #0d5cff;
  font-size: 28px;
  white-space: nowrap;
}

.traffic-panel__metrics {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 10px;
}

.traffic-panel__metrics article {
  padding: 14px;
  border-radius: 16px;
  background: rgba(255, 255, 255, 0.78);
  border: 1px solid rgba(223, 230, 242, 0.88);
}

.traffic-panel__metrics span,
.traffic-panel__metrics strong {
  display: block;
}

.traffic-panel__metrics strong {
  margin-top: 8px;
  color: #1f2937;
  font-size: 17px;
}

.traffic-panel__body {
  display: grid;
  grid-template-columns: minmax(0, 1.25fr) minmax(320px, 0.75fr);
  gap: 16px;
}

.traffic-panel__chart,
.traffic-panel__rank {
  min-width: 0;
  padding: 16px;
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.72);
  border: 1px solid rgba(226, 232, 240, 0.9);
}

.traffic-panel__chart svg {
  width: 100%;
  height: 210px;
  margin-top: 8px;
}

.traffic-panel__chart line {
  stroke: #dfe7f4;
}

.traffic-panel__chart polygon {
  fill: #2563eb;
  opacity: 0.1;
}

.traffic-panel__chart polyline {
  fill: none;
  stroke: #2563eb;
  stroke-width: 3;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.traffic-panel__chart text {
  fill: #697386;
  font-size: 12px;
  text-anchor: middle;
}

.traffic-panel__rank ul {
  display: grid;
  gap: 12px;
  margin: 14px 0 0;
  padding: 0;
  list-style: none;
}

.traffic-panel__rank li {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
  overflow: hidden;
  padding: 12px;
  border-radius: 14px;
  background: #f8fbff;
}

.traffic-panel__rank li > * {
  position: relative;
  z-index: 1;
}

.traffic-panel__rank i {
  position: absolute;
  inset: auto 0 0 0;
  height: 3px;
  background: linear-gradient(90deg, #0d5cff, #48c6ef);
}

.traffic-panel__rank strong,
.traffic-panel__rank span {
  display: block;
}

.traffic-panel__rank strong {
  color: #1f2937;
}

.traffic-panel__rank span {
  margin-top: 4px;
  font-size: 12px;
}

.traffic-panel__rank em {
  color: #0d5cff;
  font-style: normal;
  font-weight: 800;
}

@media (max-width: 1180px) {
  .traffic-panel__metrics {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .traffic-panel__body {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 720px) {
  .traffic-panel {
    padding: 16px;
  }

  .traffic-panel__hero {
    flex-direction: column;
  }

  .traffic-panel__hero strong {
    font-size: 24px;
  }

  .traffic-panel__metrics {
    grid-template-columns: 1fr;
  }
}
</style>
