<!--
  本页面用于后台编辑销售首页配置。
  它维护导航、卖点、套餐卡片、场景、FAQ 和页脚链接。
  JSON 区块只作为页面配置保存，不参与节点运行控制。
-->
<script setup lang="ts">
import { Refresh, View } from '@element-plus/icons-vue';
import { ElMessage } from 'element-plus';
import { computed, onMounted, reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient, type SalesLandingConfig } from '@/services/api';

type LandingJsonKey = 'navLinks' | 'heroStats' | 'features' | 'planCards' | 'scenarios' | 'faqs' | 'footerLinks';

const loading = ref(true);
const saving = ref(false);
const form = reactive<SalesLandingConfig>(emptyLanding());
const jsonDraft = reactive<Record<LandingJsonKey, string>>({
  navLinks: '[]',
  heroStats: '[]',
  features: '[]',
  planCards: '[]',
  scenarios: '[]',
  faqs: '[]',
  footerLinks: '[]',
});
const previewPlans = computed(() => form.planCards.slice(0, 3));

onMounted(loadLanding);

async function loadLanding() {
  loading.value = true;
  try {
    applyLanding(await apiClient.getAdminSalesLanding());
  } catch {
    applyLanding(emptyLanding());
  } finally {
    loading.value = false;
  }
}

async function saveLanding() {
  const parsed = parseDrafts();
  if (!parsed) {
    return;
  }

  saving.value = true;
  try {
    applyLanding(await apiClient.updateAdminSalesLanding({ ...form, ...parsed }));
    ElMessage.success('销售首页配置已保存');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '保存销售首页配置失败');
  } finally {
    saving.value = false;
  }
}

function applyLanding(next: SalesLandingConfig) {
  Object.assign(form, next);
  syncJsonDrafts();
}

function syncJsonDrafts() {
  jsonDraft.navLinks = pretty(form.navLinks);
  jsonDraft.heroStats = pretty(form.heroStats);
  jsonDraft.features = pretty(form.features);
  jsonDraft.planCards = pretty(form.planCards);
  jsonDraft.scenarios = pretty(form.scenarios);
  jsonDraft.faqs = pretty(form.faqs);
  jsonDraft.footerLinks = pretty(form.footerLinks);
}

function parseDrafts(): Pick<
  SalesLandingConfig,
  'navLinks' | 'heroStats' | 'features' | 'planCards' | 'scenarios' | 'faqs' | 'footerLinks'
> | null {
  try {
    return {
      navLinks: parseJson('navLinks'),
      heroStats: parseJson('heroStats'),
      features: parseJson('features'),
      planCards: parseJson('planCards'),
      scenarios: parseJson('scenarios'),
      faqs: parseJson('faqs'),
      footerLinks: parseJson('footerLinks'),
    };
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : 'JSON 格式错误');
    return null;
  }
}

function parseJson<T extends LandingJsonKey>(key: T): SalesLandingConfig[T] {
  const parsed = JSON.parse(jsonDraft[key]) as unknown;
  if (!Array.isArray(parsed)) {
    throw new Error(`${key} 必须是数组`);
  }
  return parsed as SalesLandingConfig[T];
}

function pretty(value: unknown) {
  return JSON.stringify(value, null, 2);
}

function emptyLanding(): SalesLandingConfig {
  return {
    eyebrow: 'Global Transit Access',
    title: '稳定高速的全球中转流量套餐',
    subtitle: '面向跨境办公、开发测试和高频出差场景，提供可订阅、可计量、可切换的中转节点服务。',
    announcement: '新用户注册自动领取基础套餐。',
    primaryCtaLabel: '立即注册',
    primaryCtaHref: '/register',
    secondaryCtaLabel: '查看平台能力',
    secondaryCtaHref: '/platform',
    navLinks: [
      { label: '套餐', href: '#plans' },
      { label: '场景', href: '#scenarios' },
      { label: 'FAQ', href: '#faq' },
      { label: '登录', href: '/login' },
    ],
    heroStats: [
      { label: '客户端订阅', value: 'Clash / mihomo' },
      { label: '流量额度', value: '单一额度' },
      { label: '开通方式', value: '套餐 / 兑换码' },
    ],
    features: [
      { title: '多地区可用节点', description: '按套餐专区展示可连接中转入口。' },
      { title: '统一流量计量', description: '套餐使用单一流量额度和扣费倍率。' },
      { title: '订阅链接即用', description: '复制订阅链接导入常见代理客户端。' },
    ],
    planCards: [
      {
        name: '标准流量',
        price: '按套餐配置',
        period: '周期包',
        description: '适合日常跨境办公和开发访问。',
        highlights: ['更高流量额度', '多地区中转入口', '订阅链接重置'],
        featured: true,
        ctaLabel: '查看套餐',
        ctaHref: '/plans',
      },
    ],
    scenarios: [
      { title: '跨境办公', description: '稳定访问协作平台、开发工具和海外资料站点。' },
      { title: '开发测试', description: '为接口联调、区域化验证和网络环境回归提供可控出口。' },
    ],
    faqs: [
      { question: '如何开始使用？', answer: '注册账号后进入用户首页，选择套餐或兑换码开通。' },
    ],
    footerLinks: [
      { label: '平台介绍', href: '/platform' },
      { label: '用户登录', href: '/login' },
    ],
    updatedAt: '',
  };
}
</script>

<template>
  <PageHeader title="销售首页配置" description="维护公网销售首页的标题、导航、CTA、卖点、套餐卡片、场景、FAQ 和页脚链接。">
    <el-button :icon="View" @click="$router.push('/')">预览首页</el-button>
    <el-button :icon="Refresh" :loading="loading" @click="loadLanding">刷新</el-button>
    <el-button type="primary" :loading="saving" @click="saveLanding">保存配置</el-button>
  </PageHeader>

  <el-skeleton v-if="loading" :rows="10" animated />
  <template v-else>
    <el-row :gutter="18">
      <el-col :xs="24" :lg="14">
        <el-card shadow="never">
          <template #header>首屏文案</template>
          <el-form label-position="top" class="landing-form">
            <el-form-item label="眉标">
              <el-input v-model="form.eyebrow" />
            </el-form-item>
            <el-form-item label="标题">
              <el-input v-model="form.title" />
            </el-form-item>
            <el-form-item label="副标题">
              <el-input v-model="form.subtitle" type="textarea" :rows="3" />
            </el-form-item>
            <el-form-item label="公告">
              <el-input v-model="form.announcement" />
            </el-form-item>
            <div class="landing-form__grid">
              <el-form-item label="主 CTA 文案">
                <el-input v-model="form.primaryCtaLabel" />
              </el-form-item>
              <el-form-item label="主 CTA 链接">
                <el-input v-model="form.primaryCtaHref" />
              </el-form-item>
              <el-form-item label="次 CTA 文案">
                <el-input v-model="form.secondaryCtaLabel" />
              </el-form-item>
              <el-form-item label="次 CTA 链接">
                <el-input v-model="form.secondaryCtaHref" />
              </el-form-item>
            </div>
          </el-form>
        </el-card>

        <el-card shadow="never" class="section-row">
          <template #header>结构化区块 JSON</template>
          <el-tabs>
            <el-tab-pane label="导航" name="nav">
              <el-input v-model="jsonDraft.navLinks" type="textarea" :rows="8" />
            </el-tab-pane>
            <el-tab-pane label="首屏指标" name="stats">
              <el-input v-model="jsonDraft.heroStats" type="textarea" :rows="8" />
            </el-tab-pane>
            <el-tab-pane label="卖点" name="features">
              <el-input v-model="jsonDraft.features" type="textarea" :rows="10" />
            </el-tab-pane>
            <el-tab-pane label="套餐卡片" name="plans">
              <el-input v-model="jsonDraft.planCards" type="textarea" :rows="14" />
            </el-tab-pane>
            <el-tab-pane label="场景" name="scenarios">
              <el-input v-model="jsonDraft.scenarios" type="textarea" :rows="10" />
            </el-tab-pane>
            <el-tab-pane label="FAQ" name="faq">
              <el-input v-model="jsonDraft.faqs" type="textarea" :rows="10" />
            </el-tab-pane>
            <el-tab-pane label="页脚" name="footer">
              <el-input v-model="jsonDraft.footerLinks" type="textarea" :rows="8" />
            </el-tab-pane>
          </el-tabs>
          <p class="muted">保存前会校验每个区块必须是 JSON 数组，字段使用当前示例结构。</p>
        </el-card>
      </el-col>

      <el-col :xs="24" :lg="10">
        <el-card shadow="never" class="landing-preview">
          <template #header>配置预览</template>
          <p class="eyebrow">{{ form.eyebrow }}</p>
          <h2>{{ form.title }}</h2>
          <p>{{ form.subtitle }}</p>
          <div class="preview-actions">
            <el-button type="primary" size="small">{{ form.primaryCtaLabel }}</el-button>
            <el-button size="small" plain>{{ form.secondaryCtaLabel }}</el-button>
          </div>
        </el-card>

        <el-card shadow="never" class="section-row">
          <template #header>套餐卡片预览</template>
          <div class="preview-plan-list">
            <article v-for="plan in previewPlans" :key="plan.name">
              <strong>{{ plan.name }}</strong>
              <span>{{ plan.price }} · {{ plan.period }}</span>
              <p>{{ plan.description }}</p>
            </article>
          </div>
        </el-card>
      </el-col>
    </el-row>
  </template>
</template>

<style scoped>
.landing-form__grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.landing-preview {
  border-radius: 28px;
  background:
    radial-gradient(circle at 82% 12%, rgba(15, 118, 110, 0.2), transparent 16rem),
    rgba(255, 255, 255, 0.82);
}

.landing-preview h2 {
  margin: 0;
  color: var(--accent-dark);
  font-size: 40px;
  line-height: 1;
  letter-spacing: -0.05em;
}

.landing-preview p:not(.eyebrow) {
  color: var(--ink-soft);
  line-height: 1.75;
}

.preview-actions {
  display: flex;
  gap: 10px;
  margin-top: 18px;
}

.preview-plan-list {
  display: grid;
  gap: 12px;
}

.preview-plan-list article {
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 18px;
  background: rgba(255, 255, 255, 0.62);
}

.preview-plan-list strong,
.preview-plan-list span {
  display: block;
}

.preview-plan-list span,
.preview-plan-list p {
  color: var(--ink-soft);
}

@media (max-width: 900px) {
  .landing-form__grid {
    grid-template-columns: 1fr;
  }
}
</style>
