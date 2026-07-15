<!--
  本页面是面向访客的销售首页。
  它展示 VPN 套餐卖点、使用场景、套餐卡片和常见问题。
  页面配置来自后台销售首页接口，接口失败时使用本地兜底内容。
-->
<script setup lang="ts">
import { ArrowRight, Check, Connection } from '@element-plus/icons-vue';
import { onMounted, ref } from 'vue';
import { apiClient, type SalesLandingConfig } from '@/services/api';

const landing = ref<SalesLandingConfig | null>(null);

onMounted(async () => {
  try {
    landing.value = await apiClient.getSalesLanding();
  } catch {
    landing.value = fallbackLanding();
  }
});

function fallbackLanding(): SalesLandingConfig {
  return {
    eyebrow: 'Global Transit Access',
    title: '稳定高速的全球中转流量套餐',
    subtitle: '面向跨境办公、开发测试和高频出差场景，提供可订阅、可计量、可切换的中转节点服务。',
    announcement: '新用户注册自动领取基础套餐，登录后即可复制订阅链接。',
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
      { title: '多地区可用节点', description: '按套餐专区展示可连接中转入口，真实出口由后台统一调度。' },
      { title: '统一流量计量', description: '套餐使用单一流量额度和扣费倍率，适配办公、资料检索和团队分发场景。' },
      { title: '订阅链接即用', description: '登录后复制订阅链接，导入常见代理客户端即可同步中转入口。' },
    ],
    planCards: [
      {
        name: '基础体验',
        price: '¥0',
        period: '注册领取',
        description: '用于验证订阅导入、节点质量和基础访问链路。',
        highlights: ['自动开通基础套餐', '单一流量额度', '基础中转入口'],
        featured: false,
        ctaLabel: '免费注册',
        ctaHref: '/register',
      },
      {
        name: '标准流量',
        price: '按套餐配置',
        period: '周期包',
        description: '适合日常跨境办公、资料检索和开发访问。',
        highlights: ['更高流量额度', '多地区中转入口', '订阅链接重置'],
        featured: true,
        ctaLabel: '查看套餐',
        ctaHref: '/plans',
      },
      {
        name: '团队场景',
        price: '按需开通',
        period: '周期包',
        description: '面向团队协作和多地区访问。',
        highlights: ['统一流量计量', '单一扣费倍率', '授权入口展示'],
        featured: false,
        ctaLabel: '咨询开通',
        ctaHref: '/register',
      },
    ],
    scenarios: [
      { title: '跨境办公', description: '稳定访问协作平台、开发工具和海外资料站点。' },
      { title: '开发测试', description: '为接口联调、区域化验证和网络环境回归提供可控出口。' },
      { title: '轻量团队', description: '通过套餐、兑换码和订阅链接快速分发访问能力。' },
    ],
    faqs: [
      { question: '如何开始使用？', answer: '注册账号后进入用户首页，选择套餐或兑换码开通，再复制订阅链接导入客户端。' },
      { question: '流量如何计算？', answer: '系统按套餐总流量统一统计，并使用套餐配置的单一扣费倍率。' },
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
  <main v-if="landing" class="marketing-page">
    <header class="marketing-nav">
      <RouterLink class="marketing-brand" to="/">
        <span>XC</span>
        XrayC
      </RouterLink>
      <nav>
        <a v-for="link in landing.navLinks" :key="`${link.label}-${link.href}`" :href="link.href">
          {{ link.label }}
        </a>
      </nav>
    </header>

    <section class="marketing-hero">
      <div class="marketing-hero__copy">
        <p class="eyebrow">{{ landing.eyebrow }}</p>
        <el-tag v-if="landing.announcement" effect="plain" size="large">{{ landing.announcement }}</el-tag>
        <h1>{{ landing.title }}</h1>
        <p>{{ landing.subtitle }}</p>
        <div class="marketing-hero__actions">
          <RouterLink :to="landing.primaryCtaHref">
            <el-button type="primary" size="large" :icon="ArrowRight">
              {{ landing.primaryCtaLabel }}
            </el-button>
          </RouterLink>
          <RouterLink :to="landing.secondaryCtaHref">
            <el-button size="large" plain>{{ landing.secondaryCtaLabel }}</el-button>
          </RouterLink>
        </div>
      </div>

      <div class="signal-card">
        <div class="signal-card__orb">
          <el-icon><Connection /></el-icon>
        </div>
        <strong>中转入口订阅服务</strong>
        <span>按套餐专区授权可连接中转入口，按总流量统一计量。</span>
        <div class="signal-card__grid">
          <div v-for="stat in landing.heroStats" :key="stat.label">
            <b>{{ stat.value }}</b>
            <small>{{ stat.label }}</small>
          </div>
        </div>
      </div>
    </section>

    <section class="marketing-section feature-strip" aria-label="销售卖点">
      <article v-for="feature in landing.features" :key="feature.title">
        <el-icon><Check /></el-icon>
        <h2>{{ feature.title }}</h2>
        <p>{{ feature.description }}</p>
      </article>
    </section>

    <section id="plans" class="marketing-section">
      <div class="section-heading">
        <p class="eyebrow">Plans</p>
        <h2>选择适合你的流量套餐</h2>
      </div>
      <div class="plan-card-grid">
        <article v-for="plan in landing.planCards" :key="plan.name" :class="{ featured: plan.featured }">
          <span v-if="plan.featured" class="recommend-badge">推荐</span>
          <h3>{{ plan.name }}</h3>
          <strong>{{ plan.price }}</strong>
          <small>{{ plan.period }}</small>
          <p>{{ plan.description }}</p>
          <ul>
            <li v-for="highlight in plan.highlights" :key="highlight">{{ highlight }}</li>
          </ul>
          <RouterLink :to="plan.ctaHref">
            <el-button :type="plan.featured ? 'primary' : 'default'" plain>{{ plan.ctaLabel }}</el-button>
          </RouterLink>
        </article>
      </div>
    </section>

    <section id="scenarios" class="marketing-section scenario-grid">
      <article v-for="scenario in landing.scenarios" :key="scenario.title">
        <h2>{{ scenario.title }}</h2>
        <p>{{ scenario.description }}</p>
      </article>
    </section>

    <section id="faq" class="marketing-section faq-panel">
      <div class="section-heading">
        <p class="eyebrow">FAQ</p>
        <h2>常见问题</h2>
      </div>
      <el-collapse>
        <el-collapse-item v-for="item in landing.faqs" :key="item.question" :title="item.question" :name="item.question">
          {{ item.answer }}
        </el-collapse-item>
      </el-collapse>
    </section>

    <footer class="marketing-footer">
      <span>XrayC 中转节点服务</span>
      <nav>
        <a v-for="link in landing.footerLinks" :key="`${link.label}-${link.href}`" :href="link.href">
          {{ link.label }}
        </a>
      </nav>
    </footer>
  </main>

  <main v-else class="marketing-page marketing-page--loading" aria-label="销售首页加载中">
    <section class="marketing-loading">
      <el-skeleton animated :rows="8" />
    </section>
  </main>
</template>

<style scoped>
.marketing-page {
  min-height: 100vh;
  padding: 26px clamp(18px, 5vw, 72px) 48px;
  background:
    radial-gradient(circle at 78% 12%, rgba(15, 118, 110, 0.26), transparent 28rem),
    radial-gradient(circle at 12% 72%, rgba(178, 122, 32, 0.18), transparent 24rem);
}

.marketing-page--loading {
  display: grid;
  place-items: center;
}

.marketing-loading {
  width: min(920px, 100%);
  padding: 38px;
  border-radius: 32px;
  background: rgba(255, 255, 255, 0.76);
  border: 1px solid rgba(15, 118, 110, 0.12);
  box-shadow: 0 24px 70px rgba(15, 23, 42, 0.08);
}

.marketing-nav,
.marketing-footer,
.marketing-hero,
.feature-strip,
.plan-card-grid,
.scenario-grid {
  display: grid;
  gap: 18px;
}

.marketing-nav,
.marketing-footer {
  grid-template-columns: auto 1fr;
  align-items: center;
  max-width: 1180px;
  margin: 0 auto;
}

.marketing-nav nav,
.marketing-footer nav {
  display: flex;
  justify-content: flex-end;
  gap: 20px;
  color: var(--ink-soft);
  font-weight: 700;
}

.marketing-brand {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  font-weight: 900;
  color: var(--accent-dark);
}

.marketing-brand span {
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  border-radius: 14px;
  color: #fff;
  background: linear-gradient(135deg, #0f766e, #1f2a24);
}

.marketing-hero {
  grid-template-columns: minmax(0, 1.1fr) minmax(320px, 0.8fr);
  align-items: center;
  max-width: 1180px;
  min-height: 620px;
  margin: 0 auto;
}

.marketing-hero h1 {
  max-width: 760px;
  margin: 18px 0;
  font-size: clamp(48px, 8vw, 94px);
  line-height: 0.92;
  letter-spacing: -0.07em;
  color: var(--accent-dark);
}

.marketing-hero__copy > p:not(.eyebrow) {
  max-width: 680px;
  color: var(--ink-soft);
  font-size: 19px;
  line-height: 1.9;
}

.marketing-hero__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  margin-top: 28px;
}

.signal-card {
  position: relative;
  overflow: hidden;
  min-height: 420px;
  padding: 34px;
  border: 1px solid var(--border);
  border-radius: 36px;
  background:
    linear-gradient(155deg, rgba(255, 255, 255, 0.9), rgba(255, 255, 255, 0.58)),
    repeating-linear-gradient(135deg, rgba(15, 118, 110, 0.08) 0 1px, transparent 1px 18px);
  box-shadow: 0 32px 90px rgba(31, 42, 36, 0.13);
}

.signal-card__orb {
  display: grid;
  place-items: center;
  width: 150px;
  height: 150px;
  margin: 12px auto 34px;
  border-radius: 50%;
  color: white;
  font-size: 58px;
  background: radial-gradient(circle, #20a99c, #12423d);
  box-shadow: 0 0 0 26px rgba(15, 118, 110, 0.08);
}

.signal-card strong,
.signal-card span {
  display: block;
  text-align: center;
}

.signal-card strong {
  font-size: 30px;
}

.signal-card span {
  margin-top: 10px;
  color: var(--ink-soft);
}

.signal-card__grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 10px;
  margin-top: 34px;
}

.signal-card__grid div,
.feature-strip article,
.plan-card-grid article,
.scenario-grid article,
.faq-panel {
  border: 1px solid var(--border);
  border-radius: 26px;
  background: rgba(255, 255, 255, 0.78);
  backdrop-filter: blur(14px);
}

.signal-card__grid div {
  padding: 14px;
  text-align: center;
}

.signal-card__grid b,
.signal-card__grid small {
  display: block;
}

.signal-card__grid small {
  margin-top: 6px;
  color: var(--ink-soft);
}

.marketing-section {
  max-width: 1180px;
  margin: 34px auto 0;
}

.feature-strip,
.plan-card-grid,
.scenario-grid {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.feature-strip article,
.scenario-grid article {
  padding: 26px;
}

.feature-strip .el-icon {
  color: var(--accent);
  font-size: 28px;
}

.feature-strip h2,
.scenario-grid h2,
.section-heading h2 {
  margin: 12px 0 8px;
  letter-spacing: -0.04em;
  color: var(--accent-dark);
}

.feature-strip p,
.scenario-grid p,
.plan-card-grid p {
  color: var(--ink-soft);
  line-height: 1.75;
}

.section-heading {
  margin-bottom: 18px;
}

.section-heading h2 {
  margin: 0;
  font-size: clamp(32px, 5vw, 56px);
}

.plan-card-grid article {
  position: relative;
  padding: 28px;
}

.plan-card-grid article.featured {
  border-color: rgba(15, 118, 110, 0.45);
  background: linear-gradient(180deg, rgba(229, 255, 249, 0.88), rgba(255, 255, 255, 0.88));
}

.recommend-badge {
  position: absolute;
  top: 18px;
  right: 18px;
  color: var(--accent);
  font-weight: 900;
}

.plan-card-grid h3 {
  margin: 0 0 14px;
  color: var(--accent-dark);
}

.plan-card-grid strong {
  display: block;
  font-size: 34px;
  letter-spacing: -0.04em;
}

.plan-card-grid small {
  color: var(--ink-soft);
}

.plan-card-grid ul {
  min-height: 94px;
  padding-left: 18px;
  color: var(--accent-dark);
  line-height: 1.9;
}

.faq-panel {
  padding: 28px;
}

.marketing-footer {
  margin-top: 52px;
  color: var(--ink-soft);
}

@media (max-width: 900px) {
  .marketing-nav,
  .marketing-footer,
  .marketing-hero,
  .feature-strip,
  .plan-card-grid,
  .scenario-grid {
    grid-template-columns: 1fr;
  }

  .marketing-nav nav,
  .marketing-footer nav {
    justify-content: flex-start;
    flex-wrap: wrap;
  }

  .signal-card__grid {
    grid-template-columns: 1fr;
  }
}
</style>
