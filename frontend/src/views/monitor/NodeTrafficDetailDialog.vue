<!--
  本文件是监控中心「节点流量详情」弹窗：顶部时间段选择 + 中间上/下/总汇总 + 下面手写柱状趋势图。
  时间段（今日/近7天/近30天/自定义）与桶粒度、柱高归一化全走 nodeTraffic.ts 纯函数，本组件只做展示与取数。
  趋势图不引入任何图表库：纯 div + CSS 柱，柱高按区间最大总量归一化，柱内上/下行两色叠加。
  弹窗打开或时间段变化时按当前区间调 getNodeTrafficTrend 拉趋势 + summary。
-->
<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { apiClient, type NodeTrafficTrend } from '@/services/api';
import { formatBytes } from '@/views/operations/operationsFormatters';
import {
  buildTrafficTrendBars,
  formatBucketLabel,
  resolveTrafficRange,
  type TrafficBucket,
  type TrafficRangeKey,
} from '@/views/monitor/nodeTraffic';

const visible = defineModel<boolean>({ required: true });

const props = defineProps<{
  nodeId: string;
  nodeName: string;
}>();

const rangeKey = ref<TrafficRangeKey>('today');
// 自定义区间：el-date-picker daterange，value-format="x" 得到 [起毫秒串, 止毫秒串]。
const customRange = ref<[string, string] | null>(null);
const loading = ref(false);
const errorMessage = ref('');
const trend = ref<NodeTrafficTrend | null>(null);
// 当前请求所用桶粒度，供柱下标签按小时/天格式化。
const activeBucket = ref<TrafficBucket>('hour');

const summary = computed(() => trend.value?.summary ?? { uplinkBytes: 0, downlinkBytes: 0, totalBytes: 0 });
const bars = computed(() => buildTrafficTrendBars(trend.value?.points ?? []));

// 自定义档但未选起止时不取数，提示用户先选区间。
const customPending = computed(
  () => rangeKey.value === 'custom' && (!customRange.value || !customRange.value[0] || !customRange.value[1]),
);

async function load() {
  if (!props.nodeId || customPending.value) {
    trend.value = null;
    return;
  }
  loading.value = true;
  errorMessage.value = '';
  const customFrom = customRange.value ? Number(customRange.value[0]) : null;
  const customTo = customRange.value ? Number(customRange.value[1]) : null;
  const { fromMs, toMs, bucket } = resolveTrafficRange(rangeKey.value, Date.now(), customFrom, customTo);
  activeBucket.value = bucket;
  try {
    trend.value = await apiClient.getNodeTrafficTrend(props.nodeId, fromMs, toMs, bucket);
  } catch (err) {
    trend.value = null;
    errorMessage.value = err instanceof Error && err.message ? `趋势加载失败：${err.message}` : '趋势加载失败';
  } finally {
    loading.value = false;
  }
}

// 打开弹窗时按当前时间段拉取；关闭不清空，避免下次打开闪烁。
watch(visible, (open) => {
  if (open) {
    void load();
  }
});

// 时间段或自定义区间变化时（弹窗打开状态下）重新取数。
watch([rangeKey, customRange], () => {
  if (visible.value) {
    void load();
  }
});

// 柱悬浮提示文案：时间标签 + 上/下/总字节。
function barTooltip(bar: (typeof bars.value)[number]): string {
  const label = formatBucketLabel(bar.bucketStartMs, activeBucket.value);
  return `${label}｜上行 ${formatBytes(bar.uplinkBytes)}｜下行 ${formatBytes(bar.downlinkBytes)}｜总 ${formatBytes(bar.totalBytes)}`;
}
</script>

<template>
  <el-dialog v-model="visible" :title="`节点流量详情 · ${nodeName || nodeId}`" width="720px">
    <div class="node-traffic-detail">
      <!-- 顶部：时间段选择（今日/近7天/近30天/自定义） -->
      <div class="node-traffic-detail__toolbar">
        <el-radio-group v-model="rangeKey">
          <el-radio-button value="today">今日</el-radio-button>
          <el-radio-button value="7d">近 7 天</el-radio-button>
          <el-radio-button value="30d">近 30 天</el-radio-button>
          <el-radio-button value="custom">自定义</el-radio-button>
        </el-radio-group>
        <el-date-picker
          v-if="rangeKey === 'custom'"
          v-model="customRange"
          type="daterange"
          value-format="x"
          start-placeholder="开始日期"
          end-placeholder="结束日期"
          range-separator="至"
        />
      </div>

      <el-alert
        v-if="errorMessage"
        class="node-traffic-detail__alert"
        :title="errorMessage"
        type="warning"
        show-icon
        :closable="false"
      />

      <!-- 中间：区间上/下/总汇总数字 -->
      <dl class="node-traffic-detail__summary">
        <div>
          <dt>上行</dt>
          <dd>{{ formatBytes(summary.uplinkBytes) }}</dd>
        </div>
        <div>
          <dt>下行</dt>
          <dd>{{ formatBytes(summary.downlinkBytes) }}</dd>
        </div>
        <div>
          <dt>总流量</dt>
          <dd>{{ formatBytes(summary.totalBytes) }}</dd>
        </div>
      </dl>

      <!-- 下面：纯 CSS 手写柱状趋势图（无图表库），上行/下行两色叠加 -->
      <div class="node-traffic-detail__chart" v-loading="loading">
        <div class="trend-legend">
          <span><i class="trend-legend__swatch trend-legend__swatch--up" />上行</span>
          <span><i class="trend-legend__swatch trend-legend__swatch--down" />下行</span>
        </div>
        <el-empty v-if="customPending" description="请选择自定义起止日期" :image-size="60" />
        <el-empty v-else-if="bars.length === 0" description="该区间暂无流量数据" :image-size="60" />
        <div v-else class="trend-bars">
          <el-tooltip v-for="bar in bars" :key="bar.bucketStartMs" :content="barTooltip(bar)" placement="top">
            <div class="trend-bar">
              <div class="trend-bar__track">
                <div class="trend-bar__stack" :style="{ height: `${bar.heightPercent}%` }">
                  <div class="trend-bar__seg trend-bar__seg--down" :style="{ height: `${bar.downlinkPercent}%` }" />
                  <div class="trend-bar__seg trend-bar__seg--up" :style="{ height: `${bar.uplinkPercent}%` }" />
                </div>
              </div>
              <div class="trend-bar__label">{{ formatBucketLabel(bar.bucketStartMs, activeBucket) }}</div>
            </div>
          </el-tooltip>
        </div>
      </div>
    </div>

    <template #footer>
      <el-button @click="visible = false">关闭</el-button>
    </template>
  </el-dialog>
</template>

<style scoped>
.node-traffic-detail__toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  margin-bottom: 14px;
}

.node-traffic-detail__alert {
  margin-bottom: 14px;
}

.node-traffic-detail__summary {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
  margin: 0 0 18px;
}

.node-traffic-detail__summary > div {
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.56);
}

.node-traffic-detail__summary dt {
  color: var(--ink-soft);
  font-size: 12px;
}

.node-traffic-detail__summary dd {
  margin: 4px 0 0;
  font-size: 20px;
  font-weight: 800;
  color: var(--ink);
  letter-spacing: -0.02em;
}

.trend-legend {
  display: flex;
  gap: 16px;
  margin-bottom: 10px;
  color: var(--ink-soft);
  font-size: 12px;
}

.trend-legend span {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.trend-legend__swatch {
  display: inline-block;
  width: 12px;
  height: 12px;
  border-radius: 3px;
}

.trend-legend__swatch--up {
  background: #409eff;
}

.trend-legend__swatch--down {
  background: #67c23a;
}

.trend-bars {
  display: flex;
  align-items: flex-end;
  gap: 4px;
  height: 180px;
  padding-top: 8px;
  overflow-x: auto;
}

.trend-bar {
  display: flex;
  flex: 1 0 22px;
  min-width: 22px;
  flex-direction: column;
  align-items: center;
}

.trend-bar__track {
  display: flex;
  align-items: flex-end;
  width: 100%;
  height: 150px;
  border-bottom: 1px solid var(--border);
}

.trend-bar__stack {
  display: flex;
  flex-direction: column-reverse;
  width: 100%;
  min-height: 2px;
  border-radius: 4px 4px 0 0;
  overflow: hidden;
}

.trend-bar__seg--down {
  background: #67c23a;
}

.trend-bar__seg--up {
  background: #409eff;
}

.trend-bar__label {
  margin-top: 6px;
  color: var(--ink-soft);
  font-size: 11px;
  white-space: nowrap;
}
</style>
