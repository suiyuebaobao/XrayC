// 本文件定义销售首页配置相关的前端 API 类型。
// 管理后台销售页配置和公开首页都复用这些结构。
// 文件只描述可编辑内容，不包含请求、校验、预览或渲染逻辑。
// 链接、套餐卡片和 FAQ 都必须由后端配置接口提供。

export type SalesLandingLink = {
  label: string;
  href: string;
};

export type SalesLandingStat = {
  label: string;
  value: string;
};

export type SalesLandingFeature = {
  title: string;
  description: string;
};

export type SalesLandingPlanCard = {
  name: string;
  price: string;
  period: string;
  description: string;
  highlights: string[];
  featured: boolean;
  ctaLabel: string;
  ctaHref: string;
};

export type SalesLandingScenario = {
  title: string;
  description: string;
};

export type SalesLandingFaq = {
  question: string;
  answer: string;
};

export type SalesLandingConfig = {
  eyebrow: string;
  title: string;
  subtitle: string;
  announcement: string;
  primaryCtaLabel: string;
  primaryCtaHref: string;
  secondaryCtaLabel: string;
  secondaryCtaHref: string;
  navLinks: SalesLandingLink[];
  heroStats: SalesLandingStat[];
  features: SalesLandingFeature[];
  planCards: SalesLandingPlanCard[];
  scenarios: SalesLandingScenario[];
  faqs: SalesLandingFaq[];
  footerLinks: SalesLandingLink[];
  updatedAt: string;
};
