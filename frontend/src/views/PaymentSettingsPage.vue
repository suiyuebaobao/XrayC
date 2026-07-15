<!--
  本页面用于后台配置支付渠道（支付宝、聚合易支付、微信即将支持）。
  它读写 /api/admin/payment-settings，控制购买页可用的下单渠道。
  私钥/密钥等写值字段不回显，仅用占位提示是否已配置，留空提交则保留旧值。
-->
<script setup lang="ts">
import { Refresh } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type PaymentSettings } from '@/services/api';

const loading = ref(true);
const saving = ref(false);

const form = reactive<PaymentSettings>({
  enabled: false,
  defaultChannel: 'alipay',
  providers: {
    alipay: {
      enabled: false,
      environment: 'sandbox',
      appId: '',
      appPrivateKey: '',
      appPrivateKeySet: false,
      alipayPublicKey: '',
      notifyUrl: '',
    },
    epay: {
      enabled: false,
      apiUrl: '',
      pid: '',
      key: '',
      keySet: false,
      notifyUrl: '',
    },
    wechat: {
      enabled: false,
      mchId: '',
      appId: '',
      apiV3Key: '',
      apiV3KeySet: false,
      certSerialNo: '',
      privateKey: '',
      privateKeySet: false,
      notifyUrl: '',
    },
  },
});

onMounted(load);

async function load() {
  loading.value = true;
  try {
    applySettings(await apiClient.getPaymentSettings());
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '加载支付设置失败');
  } finally {
    loading.value = false;
  }
}

async function save() {
  saving.value = true;
  try {
    applySettings(await apiClient.updatePaymentSettings({ ...form }));
    ElMessage.success('支付设置已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存支付设置失败');
  } finally {
    saving.value = false;
  }
}

function applySettings(settings: PaymentSettings) {
  form.enabled = settings.enabled;
  form.defaultChannel = settings.defaultChannel || 'alipay';
  const { alipay, epay, wechat } = settings.providers;

  form.providers.alipay.enabled = alipay.enabled;
  form.providers.alipay.environment = alipay.environment;
  form.providers.alipay.appId = alipay.appId;
  // 私钥写值不回显，保存后清空输入框，仅靠 *Set 标记是否已配置。
  form.providers.alipay.appPrivateKey = '';
  form.providers.alipay.appPrivateKeySet = alipay.appPrivateKeySet;
  form.providers.alipay.alipayPublicKey = alipay.alipayPublicKey;
  form.providers.alipay.notifyUrl = alipay.notifyUrl;

  form.providers.epay.enabled = epay.enabled;
  form.providers.epay.apiUrl = epay.apiUrl;
  form.providers.epay.pid = epay.pid;
  form.providers.epay.key = '';
  form.providers.epay.keySet = epay.keySet;
  form.providers.epay.notifyUrl = epay.notifyUrl;

  form.providers.wechat.enabled = wechat.enabled;
  form.providers.wechat.mchId = wechat.mchId;
  form.providers.wechat.appId = wechat.appId;
  form.providers.wechat.apiV3Key = '';
  form.providers.wechat.apiV3KeySet = wechat.apiV3KeySet;
  form.providers.wechat.certSerialNo = wechat.certSerialNo;
  form.providers.wechat.privateKey = '';
  form.providers.wechat.privateKeySet = wechat.privateKeySet;
  form.providers.wechat.notifyUrl = wechat.notifyUrl;
}

function secretPlaceholder(isSet: boolean) {
  return isSet ? '已设置，留空不变' : '请输入';
}
</script>

<template>
  <PageHeader title="支付设置" description="配置购买页可用的支付渠道与密钥，密钥留空表示保留原值。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :loading="saving" @click="save">保存配置</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="8" animated />
  <div v-else class="payment-settings">
    <el-card shadow="never">
      <template #header>支付总开关</template>
      <el-form label-position="top" class="payment-form">
        <el-row :gutter="18">
          <el-col :xs="24" :md="12">
            <el-form-item label="启用支付">
              <el-switch v-model="form.enabled" active-text="开启" inactive-text="关闭" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="默认渠道">
              <el-select v-model="form.defaultChannel" placeholder="选择默认渠道">
                <el-option label="支付宝" value="alipay" />
                <el-option label="聚合支付（易支付）" value="epay" />
              </el-select>
            </el-form-item>
          </el-col>
        </el-row>
      </el-form>
    </el-card>

    <el-card shadow="never">
      <template #header>支付宝</template>
      <el-form label-position="top" class="payment-form">
        <el-row :gutter="18">
          <el-col :xs="24" :md="12">
            <el-form-item label="启用支付宝">
              <el-switch v-model="form.providers.alipay.enabled" active-text="开启" inactive-text="关闭" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="环境">
              <el-select v-model="form.providers.alipay.environment">
                <el-option label="沙箱" value="sandbox" />
                <el-option label="生产" value="production" />
              </el-select>
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="APPID">
              <el-input v-model="form.providers.alipay.appId" placeholder="支付宝应用 APPID" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="应用私钥">
              <el-input
                v-model="form.providers.alipay.appPrivateKey"
                type="password"
                show-password
                :placeholder="secretPlaceholder(form.providers.alipay.appPrivateKeySet)"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24">
            <el-form-item label="支付宝公钥">
              <el-input
                v-model="form.providers.alipay.alipayPublicKey"
                type="textarea"
                :rows="3"
                placeholder="支付宝公钥（用于验签）"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24">
            <el-form-item label="回调地址">
              <el-input v-model="form.providers.alipay.notifyUrl" placeholder="留空自动用站点地址" />
            </el-form-item>
          </el-col>
        </el-row>
      </el-form>
    </el-card>

    <el-card shadow="never">
      <template #header>聚合支付（易支付）</template>
      <el-form label-position="top" class="payment-form">
        <el-row :gutter="18">
          <el-col :xs="24" :md="12">
            <el-form-item label="启用易支付">
              <el-switch v-model="form.providers.epay.enabled" active-text="开启" inactive-text="关闭" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="接口地址">
              <el-input v-model="form.providers.epay.apiUrl" placeholder="https://pay.example.com" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="商户 PID">
              <el-input v-model="form.providers.epay.pid" placeholder="易支付商户 PID" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="商户密钥">
              <el-input
                v-model="form.providers.epay.key"
                type="password"
                show-password
                :placeholder="secretPlaceholder(form.providers.epay.keySet)"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24">
            <el-form-item label="回调地址">
              <el-input v-model="form.providers.epay.notifyUrl" placeholder="留空自动用站点地址" />
            </el-form-item>
          </el-col>
        </el-row>
      </el-form>
    </el-card>

    <el-card shadow="never">
      <template #header>
        <span>微信支付</span>
        <el-tag type="info" effect="plain" size="small" class="payment-soon">即将支持</el-tag>
      </template>
      <el-form label-position="top" class="payment-form">
        <el-row :gutter="18">
          <el-col :xs="24" :md="12">
            <el-form-item label="启用微信支付">
              <el-switch v-model="form.providers.wechat.enabled" disabled active-text="开启" inactive-text="关闭" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="商户号 mch_id">
              <el-input v-model="form.providers.wechat.mchId" disabled placeholder="即将支持" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="APPID">
              <el-input v-model="form.providers.wechat.appId" disabled placeholder="即将支持" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="证书序列号">
              <el-input v-model="form.providers.wechat.certSerialNo" disabled placeholder="即将支持" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="APIv3 密钥">
              <el-input
                v-model="form.providers.wechat.apiV3Key"
                type="password"
                show-password
                disabled
                :placeholder="secretPlaceholder(form.providers.wechat.apiV3KeySet)"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :md="12">
            <el-form-item label="商户私钥">
              <el-input
                v-model="form.providers.wechat.privateKey"
                type="password"
                show-password
                disabled
                :placeholder="secretPlaceholder(form.providers.wechat.privateKeySet)"
              />
            </el-form-item>
          </el-col>
          <el-col :xs="24">
            <el-form-item label="回调地址">
              <el-input v-model="form.providers.wechat.notifyUrl" disabled placeholder="即将支持" />
            </el-form-item>
          </el-col>
        </el-row>
      </el-form>
    </el-card>
  </div>
</template>

<style scoped>
.payment-settings {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.payment-form {
  max-width: 900px;
}

.payment-soon {
  margin-left: 8px;
}
</style>
