<!--
  本页面用于展示平台能力说明。
  它面向登录后的用户解释中转节点、流量计费和安全能力。
  页面是静态说明，不发起 API 请求。
-->
<script setup lang="ts">
import { Connection, DataAnalysis, Lock, Tickets } from '@element-plus/icons-vue';

const capabilities = [
  {
    icon: Connection,
    title: '中转节点编排',
    description: '控制面管理中转节点和分组，用户只看到被授权的中转入口。',
  },
  {
    icon: Tickets,
    title: '套餐与订阅',
    description: '套餐授权单一流量额度和分组，用户通过订阅链接导入客户端。',
  },
  {
    icon: DataAnalysis,
    title: '运营观测',
    description: '汇总在线用户、活跃连接、独立客户端 IP、探测策略和近期事件。',
  },
  {
    icon: Lock,
    title: '认证安全',
    description: '支持邀请码、算术验证码、邮箱验证码、登录锁定和 SMTP 配置。',
  },
];

const workflow = [
  '管理员先在出口管理维护真实出口，再到入口管理创建入口并绑定出口。',
  '分组直接选择入口出口绑定节点，套餐授权分组决定用户可见节点。',
  '用户注册或登录后进入首页，查看当前套餐、订阅链接和流量用量。',
  '客户端访问订阅地址获取配置，控制面只暴露用户被授权的中转入口。',
];
</script>

<template>
  <main class="platform-page">
    <header class="platform-nav">
      <RouterLink class="marketing-brand" to="/">
        <span>XC</span>
        XrayC
      </RouterLink>
      <nav>
        <RouterLink to="/">销售首页</RouterLink>
        <RouterLink to="/login">登录</RouterLink>
      </nav>
    </header>

    <section class="platform-hero">
      <p class="eyebrow">Platform</p>
      <h1>出口管理、入口管理、分组、套餐授权统一管理。</h1>
      <p>
        XrayC 将用户订阅、套餐授权、中转节点和分组拆开管理，让销售页面面向转化，用户页聚焦订阅，
        后台页负责运营和安全策略。
      </p>
    </section>

    <section class="capability-grid" aria-label="平台能力">
      <article v-for="item in capabilities" :key="item.title">
        <el-icon><component :is="item.icon" /></el-icon>
        <h2>{{ item.title }}</h2>
        <p>{{ item.description }}</p>
      </article>
    </section>

    <section class="platform-flow">
      <div>
        <p class="eyebrow">Flow</p>
        <h2>从后台配置到用户订阅</h2>
      </div>
      <ol>
        <li v-for="item in workflow" :key="item">{{ item }}</li>
      </ol>
    </section>
  </main>
</template>

<style scoped>
.platform-page {
  min-height: 100vh;
  padding: 26px clamp(18px, 5vw, 72px) 56px;
}

.platform-nav {
  display: flex;
  justify-content: space-between;
  gap: 20px;
  align-items: center;
  max-width: 1120px;
  margin: 0 auto;
}

.platform-nav nav {
  display: flex;
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

.platform-hero,
.capability-grid,
.platform-flow {
  max-width: 1120px;
  margin: 0 auto;
}

.platform-hero {
  padding: 104px 0 56px;
}

.platform-hero h1 {
  max-width: 900px;
  margin: 0;
  color: var(--accent-dark);
  font-size: clamp(44px, 7vw, 84px);
  line-height: 0.96;
  letter-spacing: -0.07em;
}

.platform-hero p:not(.eyebrow) {
  max-width: 780px;
  color: var(--ink-soft);
  font-size: 19px;
  line-height: 1.9;
}

.capability-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 18px;
}

.capability-grid article,
.platform-flow {
  border: 1px solid var(--border);
  border-radius: 28px;
  background: rgba(255, 255, 255, 0.78);
  backdrop-filter: blur(14px);
}

.capability-grid article {
  padding: 24px;
}

.capability-grid .el-icon {
  color: var(--accent);
  font-size: 30px;
}

.capability-grid h2,
.platform-flow h2 {
  margin: 14px 0 8px;
  color: var(--accent-dark);
  letter-spacing: -0.04em;
}

.capability-grid p,
.platform-flow li {
  color: var(--ink-soft);
  line-height: 1.8;
}

.platform-flow {
  display: grid;
  grid-template-columns: 0.8fr 1.2fr;
  gap: 28px;
  margin-top: 24px;
  padding: 30px;
}

.platform-flow ol {
  margin: 0;
  padding-left: 22px;
}

@media (max-width: 900px) {
  .platform-nav,
  .platform-flow {
    display: grid;
    grid-template-columns: 1fr;
  }

  .capability-grid {
    grid-template-columns: 1fr;
  }
}
</style>
