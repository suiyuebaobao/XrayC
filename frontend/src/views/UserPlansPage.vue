<!--
  本页面用于普通用户浏览可购买套餐。
  它只展示已启用且未删除的套餐信息。
  点击购买后按可用支付渠道下单：多渠道让用户选择，单渠道直接用，弹窗完成扫码或跳转支付。
-->
<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { computed, onMounted, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import AlipayQrPaymentDialog from '@/views/plans/AlipayQrPaymentDialog.vue';
import { apiClient, type OrderInfo, type PaymentChannel, type PlanInfo } from '@/services/api';

const loading = ref(true);
const plans = ref<PlanInfo[]>([]);
const sortedPlans = computed(() => [...plans.value].sort((left, right) => left.sortWeight - right.sortWeight));

// 可用支付渠道，进页面拉取一次；多渠道时购买前让用户选择。
const channels = ref<PaymentChannel[]>([]);

// 正在下单的套餐 id，用于单个购买按钮的 loading 与禁用其它按钮。
const purchasingPlanId = ref('');
const payDialogVisible = ref(false);
const currentOrder = ref<OrderInfo | null>(null);

// 渠道选择弹窗状态：多渠道时打开，确认/取消通过 resolveChannel 回传结果。
const channelDialogVisible = ref(false);
const selectedChannel = ref('');
let resolveChannel: ((channel: string | undefined) => void) | null = null;

onMounted(() => {
  void loadPlans();
  void loadChannels();
});

async function loadPlans() {
  loading.value = true;
  try {
    plans.value = (await apiClient.getPlans()).filter((plan) => plan.enabled && !plan.isDeleted);
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载套餐失败');
  } finally {
    loading.value = false;
  }
}

async function loadChannels() {
  try {
    const info = await apiClient.getPaymentChannels();
    channels.value = info.enabled ? info.channels : [];
  } catch {
    // 渠道拉取失败不阻断浏览，下单时退回后端默认渠道。
    channels.value = [];
  }
}

// 返回选中渠道：单渠道直接用，无渠道返回 undefined（用后端默认），多渠道弹窗让用户选。
function pickChannel(): Promise<string | undefined> {
  if (channels.value.length <= 1) {
    return Promise.resolve(channels.value[0]?.channel);
  }
  selectedChannel.value = channels.value[0].channel;
  channelDialogVisible.value = true;
  return new Promise((resolve) => {
    resolveChannel = resolve;
  });
}

function confirmChannel() {
  channelDialogVisible.value = false;
  resolveChannel?.(selectedChannel.value);
  resolveChannel = null;
}

function cancelChannel() {
  channelDialogVisible.value = false;
  resolveChannel?.(undefined);
  resolveChannel = null;
}

// 弹窗被点遮罩/Esc 关闭时，按取消处理，避免下单挂起。
function onChannelDialogClosed() {
  if (resolveChannel) {
    resolveChannel(undefined);
    resolveChannel = null;
  }
}

async function purchase(plan: PlanInfo) {
  if (purchasingPlanId.value) {
    return;
  }
  const channel = await pickChannel();
  // 多渠道弹窗取消时返回 undefined 且未发起下单意图，结束本次购买。
  if (channels.value.length > 1 && channel === undefined) {
    return;
  }
  purchasingPlanId.value = plan.id;
  try {
    const order = await apiClient.createOrder({ planId: plan.id, channel });
    // 出码失败时后端返回 paymentError，直接提示不弹支付窗。
    if (order.paymentError) {
      ElMessage.error(order.paymentError);
      return;
    }
    if (!order.qrCode && !order.payUrl) {
      ElMessage.error('未获取到支付信息，请稍后重试');
      return;
    }
    currentOrder.value = order;
    payDialogVisible.value = true;
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '下单失败');
  } finally {
    purchasingPlanId.value = '';
  }
}

function onPaid() {
  void loadPlans();
}

function formatTraffic(plan: PlanInfo) {
  return formatGb(plan.trafficLimitGb);
}

function formatGb(value: number) {
  return `${Number(value.toFixed(value >= 10 ? 0 : 2))} GB`;
}

function formatPrice(plan: PlanInfo) {
  return `${plan.currency} ${Number((plan.priceCents / 100).toFixed(2))}`;
}
</script>

<template>
  <PageHeader title="套餐" description="选择适合当前账号的流量套餐，套餐决定可用节点范围。">
    <el-button :loading="loading" @click="loadPlans">刷新</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="8" animated />
  <div v-else class="user-plan-grid">
    <el-card v-for="plan in sortedPlans" :key="plan.id" shadow="never" :class="{ 'is-default': plan.isDefault }">
      <template #header>
        <div class="user-plan-card__header">
          <strong>{{ plan.name }}</strong>
          <el-tag v-if="plan.isDefault" type="success" effect="plain">基础套餐</el-tag>
        </div>
      </template>
      <div class="user-plan-card__price">{{ formatPrice(plan) }}</div>
      <p class="muted">{{ plan.durationDays }} 天有效期</p>
      <el-divider />
      <p>流量额度 {{ formatTraffic(plan) }}</p>
      <p>可用节点由套餐授权分组控制</p>
      <p>扣费倍率 {{ plan.billingMultiplier }}x</p>
      <el-button
        type="primary"
        :loading="purchasingPlanId === plan.id"
        :disabled="!!purchasingPlanId && purchasingPlanId !== plan.id"
        @click="purchase(plan)"
      >
        购买
      </el-button>
    </el-card>
    <el-empty v-if="sortedPlans.length === 0" description="暂无可购买套餐" />
  </div>

  <el-dialog
    v-model="channelDialogVisible"
    title="选择支付方式"
    width="320px"
    align-center
    @closed="onChannelDialogClosed"
  >
    <el-radio-group v-model="selectedChannel" class="channel-options">
      <el-radio v-for="item in channels" :key="item.channel" :value="item.channel" border>
        {{ item.label }}
      </el-radio>
    </el-radio-group>
    <template #footer>
      <el-button @click="cancelChannel">取消</el-button>
      <el-button type="primary" @click="confirmChannel">去支付</el-button>
    </template>
  </el-dialog>

  <AlipayQrPaymentDialog v-model="payDialogVisible" :order="currentOrder" @paid="onPaid" />
</template>

<style scoped>
.channel-options {
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: 100%;
}

.channel-options :deep(.el-radio) {
  margin-right: 0;
  width: 100%;
}

.user-plan-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 18px;
}

.user-plan-grid .el-card {
  border-radius: 24px;
}

.user-plan-grid .el-card.is-default {
  background: linear-gradient(180deg, rgba(229, 255, 249, 0.88), rgba(255, 255, 255, 0.82));
}

.user-plan-card__header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.user-plan-card__price {
  color: var(--accent-dark);
  font-size: 36px;
  font-weight: 900;
  letter-spacing: -0.05em;
}

@media (max-width: 900px) {
  .user-plan-grid {
    grid-template-columns: 1fr;
  }
}
</style>
