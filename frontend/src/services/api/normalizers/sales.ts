// 本文件负责销售首页配置接口的响应标准化和提交 payload 序列化。
// 它只处理销售页导航、卖点、套餐卡片、场景、FAQ 和页脚链接。
// 默认值集中在这里，避免公开首页和后台编辑页重复维护文案兜底。
// 本文件不发起网络请求，也不读写浏览器会话。

import type {
  SalesLandingConfig,
  SalesLandingFaq,
  SalesLandingFeature,
  SalesLandingLink,
  SalesLandingPlanCard,
  SalesLandingScenario,
  SalesLandingStat,
} from '../types';
import { arrayValue, booleanValue, recordValue, stringArrayValue, stringValue } from '../primitives';

export function normalizeSalesLanding(value: unknown): SalesLandingConfig {
  const data = recordValue(value);
  const hero = recordValue(data.hero);
  const cta = recordValue(data.cta);
  const portal = recordValue(data.portalFeatures ?? data.portal_features);

  return {
    portalFeatures: {
      plans: booleanValue(portal.plans ?? true), orders: booleanValue(portal.orders ?? true),
      redeem: booleanValue(portal.redeem ?? true), invites: booleanValue(portal.invites ?? true),
      marketing: booleanValue(portal.marketing ?? true),
    },
    eyebrow: stringValue(data.eyebrow ?? hero.eyebrow) || 'Global Transit Access',
    title: stringValue(data.title ?? hero.title) || '稳定高速的全球中转流量套餐',
    subtitle: stringValue(data.subtitle ?? data.description ?? hero.subtitle)
      || '面向跨境办公、开发测试和高频出差场景，提供可订阅、可计量、可切换的节点服务。',
    announcement: stringValue(data.announcement ?? hero.announcement),
    primaryCtaLabel: stringValue(
      data.primaryCtaLabel
      ?? data.primary_cta_label
      ?? data.primary_cta_text
      ?? hero.primaryCtaLabel
      ?? hero.primary_cta_label
      ?? hero.primaryCtaText
      ?? hero.primary_cta_text
      ?? cta.primaryLabel
      ?? cta.primary_label
      ?? cta.primaryText
      ?? cta.primary_text,
    ) || '立即注册',
    primaryCtaHref: stringValue(
      data.primaryCtaHref ?? data.primary_cta_href ?? hero.primaryCtaHref ?? hero.primary_cta_href ?? cta.primaryHref ?? cta.primary_href,
    ) || '/register',
    secondaryCtaLabel: stringValue(
      data.secondaryCtaLabel
      ?? data.secondary_cta_label
      ?? data.secondary_cta_text
      ?? hero.secondaryCtaLabel
      ?? hero.secondary_cta_label
      ?? hero.secondaryCtaText
      ?? hero.secondary_cta_text
      ?? cta.secondaryLabel
      ?? cta.secondary_label
      ?? cta.secondaryText
      ?? cta.secondary_text,
    ) || '查看平台能力',
    secondaryCtaHref: stringValue(
      data.secondaryCtaHref
      ?? data.secondary_cta_href
      ?? hero.secondaryCtaHref
      ?? hero.secondary_cta_href
      ?? cta.secondaryHref
      ?? cta.secondary_href,
    ) || '/platform',
    navLinks: normalizeSalesLandingLinks(data.navLinks ?? data.nav_links, [
      { label: '套餐', href: '#plans' },
      { label: '场景', href: '#scenarios' },
      { label: 'FAQ', href: '#faq' },
      { label: '登录', href: '/login' },
    ]),
    heroStats: normalizeSalesLandingStats(data.heroStats ?? data.hero_stats ?? data.stats ?? hero.stats, [
      { label: '订阅导入', value: 'Clash / mihomo' },
      { label: '流量额度', value: '单一额度' },
      { label: '入口策略', value: '自动分配' },
    ]),
    features: normalizeSalesLandingFeatures(data.features, [
      { title: '分组授权节点', description: '套餐授权分组决定可见中转入口，真实出口由后台统一调度。' },
      { title: '统一流量计量', description: '套餐使用单一流量额度和扣费倍率，适配办公、资料检索和团队分发场景。' },
      { title: '订阅即用', description: '登录后复制订阅链接，导入常见代理客户端即可使用。' },
    ]),
    planCards: normalizeSalesLandingPlanCards(data.planCards ?? data.plan_cards),
    scenarios: normalizeSalesLandingScenarios(data.scenarios, [
      { title: '跨境办公', description: '稳定访问常用协作平台、开发工具和海外资料站点。' },
      { title: '开发测试', description: '为接口联调、区域化验证和网络环境回归提供可控出口。' },
      { title: '轻量团队', description: '用兑换码和套餐订阅快速分发访问能力，便于集中管理。' },
    ]),
    faqs: normalizeSalesLandingFaqs(data.faqs ?? data.faq, [
      { question: '如何开始使用？', answer: '注册账号后进入用户首页，选择套餐或兑换码开通，再复制订阅链接导入客户端。' },
      { question: '流量如何计算？', answer: '系统按套餐总流量统一统计，并使用套餐配置的单一扣费倍率。' },
    ]),
    footerLinks: normalizeSalesLandingLinks(data.footerLinks ?? data.footer_links ?? data.footer, [
      { label: '平台介绍', href: '/platform' },
      { label: '用户登录', href: '/login' },
    ]),
    updatedAt: stringValue(data.updatedAt ?? data.updated_at),
  };
}

function normalizeSalesLandingLinks(value: unknown, fallback: SalesLandingLink[]): SalesLandingLink[] {
  const links = arrayValue(value)
    .map((item) => {
      const data = recordValue(item);
      return {
        label: stringValue(data.label ?? data.title ?? data.name),
        href: stringValue(data.href ?? data.url ?? data.path),
      };
    })
    .filter((item) => item.label && item.href);

  return links.length > 0 ? links : fallback;
}

function normalizeSalesLandingStats(value: unknown, fallback: SalesLandingStat[]): SalesLandingStat[] {
  const stats = arrayValue(value)
    .map((item) => {
      const data = recordValue(item);
      return {
        label: stringValue(data.label ?? data.name),
        value: stringValue(data.value ?? data.text),
      };
    })
    .filter((item) => item.label && item.value);

  return stats.length > 0 ? stats : fallback;
}

function normalizeSalesLandingFeatures(value: unknown, fallback: SalesLandingFeature[]): SalesLandingFeature[] {
  const features = arrayValue(value)
    .map((item) => {
      const data = recordValue(item);
      return {
        title: stringValue(data.title ?? data.name),
        description: stringValue(data.description ?? data.desc ?? data.content),
      };
    })
    .filter((item) => item.title && item.description);

  return features.length > 0 ? features : fallback;
}

function normalizeSalesLandingPlanCards(value: unknown): SalesLandingPlanCard[] {
  const cards = arrayValue(value)
    .map((item) => {
      const data = recordValue(item);
      return {
        name: stringValue(data.name ?? data.title),
        price: stringValue(data.price ?? data.priceText ?? data.price_text),
        period: stringValue(data.period ?? data.cycle),
        description: stringValue(data.description ?? data.desc),
        highlights: stringArrayValue(data.highlights ?? data.items),
        featured: booleanValue(data.featured ?? data.recommended),
        ctaLabel: stringValue(data.ctaLabel ?? data.cta_label) || '选择套餐',
        ctaHref: stringValue(data.ctaHref ?? data.cta_href) || '/register',
      };
    })
    .filter((item) => item.name && item.price);

  return cards.length > 0 ? cards : defaultPlanCards();
}

function defaultPlanCards(): SalesLandingPlanCard[] {
  return [
    {
      name: '基础体验',
      price: '¥0',
      period: '注册领取',
      description: '适合验证订阅导入、节点可用性和基础访问链路。',
      highlights: ['自动开通基础套餐', '单一流量额度', '可连接中转入口'],
      featured: false,
      ctaLabel: '免费注册',
      ctaHref: '/register',
    },
    {
      name: '标准流量',
      price: '按套餐配置',
      period: '月付 / 周期包',
      description: '面向日常办公、资料检索和开发访问的主力套餐。',
      highlights: ['更高流量额度', '多地区中转入口', '订阅链接重置'],
      featured: true,
      ctaLabel: '查看套餐',
      ctaHref: '/plans',
    },
    {
      name: '团队场景',
      price: '按需开通',
      period: '周期包',
      description: '为团队协作和多地区访问提供可授权的中转入口。',
      highlights: ['统一流量计量', '单一扣费倍率', '按授权入口展示'],
      featured: false,
      ctaLabel: '咨询开通',
      ctaHref: '/register',
    },
  ];
}

function normalizeSalesLandingScenarios(value: unknown, fallback: SalesLandingScenario[]): SalesLandingScenario[] {
  const scenarios = arrayValue(value)
    .map((item) => {
      if (typeof item === 'string') {
        return {
          title: item,
          description: '适合需要稳定订阅接入和统一流量管理的使用场景。',
        };
      }

      const data = recordValue(item);
      return {
        title: stringValue(data.title ?? data.name),
        description: stringValue(data.description ?? data.desc ?? data.content),
      };
    })
    .filter((item) => item.title && item.description);

  return scenarios.length > 0 ? scenarios : fallback;
}

function normalizeSalesLandingFaqs(value: unknown, fallback: SalesLandingFaq[]): SalesLandingFaq[] {
  const faqs = arrayValue(value)
    .map((item) => {
      const data = recordValue(item);
      return {
        question: stringValue(data.question ?? data.title),
        answer: stringValue(data.answer ?? data.content),
      };
    })
    .filter((item) => item.question && item.answer);

  return faqs.length > 0 ? faqs : fallback;
}

export function salesLandingPayload(payload: SalesLandingConfig) {
  return {
    portal_features: payload.portalFeatures,
    eyebrow: payload.eyebrow,
    title: payload.title,
    subtitle: payload.subtitle,
    announcement: payload.announcement,
    primary_cta_label: payload.primaryCtaLabel,
    primary_cta_href: payload.primaryCtaHref,
    secondary_cta_label: payload.secondaryCtaLabel,
    secondary_cta_href: payload.secondaryCtaHref,
    nav_links: payload.navLinks,
    hero_stats: payload.heroStats,
    features: payload.features,
    plan_cards: payload.planCards.map((card) => ({
      name: card.name,
      price: card.price,
      period: card.period,
      description: card.description,
      highlights: card.highlights,
      featured: card.featured,
      cta_label: card.ctaLabel,
      cta_href: card.ctaHref,
    })),
    scenarios: payload.scenarios,
    faqs: payload.faqs,
    footer_links: payload.footerLinks,
  };
}
