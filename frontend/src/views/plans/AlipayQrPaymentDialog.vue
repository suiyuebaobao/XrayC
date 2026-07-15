<!--
  本组件承载购买支付的弹窗，按订单返回内容自适应渠道。
  返回 qrCode 时渲染二维码（支付宝当面付）；返回 payUrl 时给出跳转支付链接（易支付等）。
  下单请求由父页面 UserPlansPage 发起，本组件只接收已创建的订单。
  无论哪种渠道都轮询订单状态，支付成功后 paid 事件通知父页面刷新，定时器在关闭/卸载时清理避免泄漏。
-->
<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { onBeforeUnmount, ref, watch } from 'vue';
import QRCode from 'qrcode';
import { apiClient, type OrderInfo } from '@/services/api';

// 轮询间隔 3 秒；上限 5 分钟后停止并提示超时。
const POLL_INTERVAL_MS = 3000;
const POLL_TIMEOUT_MS = 5 * 60 * 1000;

const visible = defineModel<boolean>({ required: true });

const props = defineProps<{
  order: OrderInfo | null;
}>();

const emit = defineEmits<{
  (event: 'paid'): void;
}>();

const qrDataUrl = ref('');
const qrError = ref('');
let pollTimer: ReturnType<typeof setInterval> | null = null;
let pollDeadline = 0;

function stopPolling() {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

async function renderQr(text: string) {
  qrError.value = '';
  qrDataUrl.value = '';
  try {
    qrDataUrl.value = await QRCode.toDataURL(text, { width: 240, margin: 1 });
  } catch (error) {
    qrError.value = error instanceof Error ? error.message : '二维码生成失败';
  }
}

async function pollOnce(orderNo: string) {
  if (Date.now() > pollDeadline) {
    stopPolling();
    visible.value = false;
    ElMessage.warning('二维码已超时，请重新下单');
    return;
  }
  try {
    const orders = await apiClient.getUserOrders();
    const found = orders.find((item) => item.orderNo === orderNo);
    if (found && found.status === 'paid') {
      stopPolling();
      ElMessage.success('支付成功');
      visible.value = false;
      emit('paid');
    }
  } catch {
    // 单次轮询失败忽略，等待下一次重试，避免打断用户支付。
  }
}

function startPolling(orderNo: string) {
  stopPolling();
  pollDeadline = Date.now() + POLL_TIMEOUT_MS;
  pollTimer = setInterval(() => {
    void pollOnce(orderNo);
  }, POLL_INTERVAL_MS);
}

// 弹窗打开且订单可支付（二维码或跳转链接）时开始轮询；有二维码再额外渲染图片。关闭时停止轮询。
watch(
  () => [visible.value, props.order] as const,
  ([open, order]) => {
    if (open && order && (order.qrCode || order.payUrl)) {
      if (order.qrCode) {
        void renderQr(order.qrCode);
      } else {
        qrDataUrl.value = '';
        qrError.value = '';
      }
      startPolling(order.orderNo);
    } else {
      stopPolling();
    }
  },
  { immediate: true },
);

onBeforeUnmount(stopPolling);

// 易支付等渠道在新标签页打开收银台；订单状态仍由本弹窗轮询。
function openPayUrl(url: string | undefined) {
  if (url) {
    window.open(url, '_blank', 'noopener');
  }
}

function amountLabel(order: OrderInfo) {
  const cents = order.amountCents ?? 0;
  return `${Number((cents / 100).toFixed(2))} ${order.currency || 'CNY'}`;
}
</script>

<template>
  <el-dialog
    v-model="visible"
    title="支付订单"
    width="360px"
    align-center
    @closed="stopPolling"
  >
    <div v-if="order" class="alipay-qr">
      <p class="alipay-qr__plan">{{ order.planName }}</p>
      <p class="alipay-qr__amount">{{ amountLabel(order) }}</p>
      <template v-if="order.qrCode">
        <div class="alipay-qr__image">
          <img v-if="qrDataUrl" :src="qrDataUrl" alt="支付二维码" width="240" height="240" />
          <el-alert v-else-if="qrError" :title="qrError" type="error" :closable="false" show-icon />
          <el-skeleton v-else :rows="0" animated style="width: 240px; height: 240px" />
        </div>
        <p class="alipay-qr__tip">请扫码完成支付</p>
      </template>
      <template v-else-if="order.payUrl">
        <div class="alipay-qr__redirect">
          <el-button type="primary" size="large" @click="openPayUrl(order.payUrl)">去支付</el-button>
        </div>
        <p class="alipay-qr__tip">已在新标签页打开收银台，完成支付后返回本页</p>
      </template>
      <p class="muted">支付成功后将自动开通，请勿关闭本窗口</p>
    </div>
  </el-dialog>
</template>

<style scoped>
.alipay-qr {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  text-align: center;
}

.alipay-qr__plan {
  font-weight: 700;
  font-size: 16px;
}

.alipay-qr__amount {
  color: var(--accent-dark);
  font-size: 28px;
  font-weight: 900;
  letter-spacing: -0.04em;
}

.alipay-qr__image {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 240px;
}

.alipay-qr__redirect {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 120px;
}

.alipay-qr__tip {
  font-weight: 600;
}
</style>
