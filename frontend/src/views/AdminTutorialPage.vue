<!--
  本页面是管理员后台的系统使用教程。
  它用静态内容解释出口管理、分组、中转节点、套餐和运维流程。
  页面不发起 API 请求，也不展示任何私有服务器、Token 或代理凭据。
-->
<script setup lang="ts">
import PageHeader from '@/components/PageHeader.vue';

const quickStart = [
  {
    title: '第一步：准备出口管理',
    path: '/admin/line-pool',
    action: '录入第三方出口或自建出口线路，确认协议、地址、端口、账号密码和健康探测信息完整。',
    checks: ['线路协议和对接方式填写真实值', '管理员后台可看到完整连接配置', '探测状态不影响订阅节点暴露'],
  },
  {
    title: '第二步：创建入口绑定',
    path: '/admin/access-entries',
    action: '在入口管理里为中转节点创建用户入口，并把入口绑定到一条或多条出口线路，形成用户订阅节点。',
    checks: ['入口协议和网络模式符合协议限制', '每个绑定节点都有明确入口和出口', '入口变更后中转节点会进入待同步状态'],
  },
  {
    title: '第三步：配置分组',
    path: '/admin/line-groups',
    action: '把入口出口绑定节点加入分组，例如 AI 分组、游戏分组、GPT 分组、视频分组。',
    checks: ['分组用于套餐授权、倍率和归类', '一个分组可以包含多个绑定节点', '用户订阅只看到授权分组下已同步且可连接的订阅节点'],
  },
  {
    title: '第四步：部署中转节点',
    path: '/admin/transit-nodes',
    action: '优先使用一键安装 Agent，填写服务器 SSH 和节点信息后由平台自动连接服务器、上传并执行脚本，在安装进度里查看远端执行、拉取容器和登记节点；受限环境再使用手动安装说明。',
    checks: ['安装任务能看到目标服务器和当前步骤', '失败时有脱敏错误摘要和处理建议', '新增节点后心跳正常'],
  },
  {
    title: '第五步：配置套餐授权',
    path: '/admin/plans',
    action: '套餐只授权分组和流量额度，用户能看到哪些节点由套餐绑定的分组决定。',
    checks: ['套餐启用且流量额度正确', '套餐绑定正确的分组', '修改套餐后用户订阅重新拉取生效'],
  },
  {
    title: '第六步：交付用户订阅',
    path: '/admin/users',
    action: '创建或确认用户订阅，把正式订阅链接交给用户导入 Clash Verge Rev、mihomo 等客户端。',
    checks: ['订阅中不暴露内部运行集合或第三方账号', '节点数量来自已绑定且可连接的线路', '客户端导入后每个可用节点可连接'],
  },
  {
    title: '第七步：日常运维检查',
    path: '/admin/access-operations',
    action: '通过运营中心、健康检查、用户流量日志和审计日志确认节点状态、真实流量和管理员操作。',
    checks: ['中转节点在线且有心跳', '流量上报和扣费账本有增量', '异常节点先看健康检查再处理'],
  },
];

const operationCards = [
  {
    title: '新增一条可用线路',
    steps: ['进入出口管理新增出口线路', '到入口管理创建入口并绑定出口线路', '把绑定节点加入分组用于授权归类', '重新拉取用户订阅并测试出站'],
  },
  {
    title: '新增一台中转服务器',
    steps: [
      '打开一键安装 Agent',
      '填写节点名称、地址、端口、SSH 和 SSL 信息',
      '在安装进度展开任务查看每个步骤和失败原因',
      '安装成功后确认节点心跳和待同步状态',
    ],
  },
  {
    title: '给用户开通节点',
    steps: ['确认用户已有有效套餐', '在套餐授权里选择分组', '让用户重新更新订阅', '在流量日志查看真实访问记录'],
  },
  {
    title: '排查节点不可用',
    steps: ['先看健康检查卡片', '再看中转节点待同步状态', '检查出口管理探测结果', '最后查看用户流量日志和审计日志'],
  },
];

const safetyRules = [
  '订阅只应该暴露中转入口，不应该暴露第三方出口地址、账号、密码或代理 URL。',
  '真实流量统计以 access-agent 上报为准，不能用中心侧估算或手动填写替代。',
  '禁用套餐、用户、分组或出口线路后，要让用户重新拉取订阅并检查中转节点配置同步状态。',
  'Agent 安装失败时先展开安装任务查看失败步骤、脱敏错误摘要和处理建议，再检查服务器执行输出、中转节点心跳和配置状态。',
];
</script>

<template>
  <PageHeader title="使用教程" description="从出口管理到用户订阅的完整运营路径，按这个顺序操作可以减少配置混乱。">
    <el-button type="primary" plain @click="$router.push('/admin/transit-nodes')">去部署中转节点</el-button>
  </PageHeader>

  <section class="tutorial-hero">
    <div>
      <p class="eyebrow">Operator Guide</p>
      <h2>先有出口，再有入口绑定，最后通过分组交付给用户。</h2>
      <p>
        XrayC V2 的核心是中转强控制：用户连接中转入口，中转节点负责鉴权、路由、计费和流量日志；
        出口线路只作为后台可调度资源，不直接交给用户。
      </p>
    </div>
    <ol>
      <li>出口管理维护可用出口。</li>
      <li>入口管理创建入口并绑定出口。</li>
      <li>分组直接包含绑定节点。</li>
      <li>套餐授权决定用户能看到哪些节点。</li>
    </ol>
  </section>

  <section class="tutorial-section">
    <div class="section-title">
      <p class="eyebrow">Quick Start</p>
      <h2>开通线路的标准流程</h2>
    </div>
    <div class="step-grid">
      <article v-for="item in quickStart" :key="item.title" class="step-card">
        <div class="step-card__header">
          <h3>{{ item.title }}</h3>
          <RouterLink :to="item.path">进入页面</RouterLink>
        </div>
        <p>{{ item.action }}</p>
        <ul>
          <li v-for="check in item.checks" :key="check">{{ check }}</li>
        </ul>
      </article>
    </div>
  </section>

  <section class="tutorial-section">
    <div class="section-title">
      <p class="eyebrow">Daily Work</p>
      <h2>常见操作怎么做</h2>
    </div>
    <div class="operation-grid">
      <el-card v-for="card in operationCards" :key="card.title" shadow="never" class="operation-card">
        <template #header>{{ card.title }}</template>
        <ol>
          <li v-for="step in card.steps" :key="step">{{ step }}</li>
        </ol>
      </el-card>
    </div>
  </section>

  <section class="tutorial-section">
    <el-card shadow="never" class="safety-card">
      <template #header>上线前必须检查</template>
      <ul>
        <li v-for="rule in safetyRules" :key="rule">{{ rule }}</li>
      </ul>
    </el-card>
  </section>
</template>

<style scoped>
.tutorial-hero {
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) minmax(280px, 0.6fr);
  gap: 22px;
  align-items: stretch;
  margin-bottom: 22px;
  padding: 28px;
  border: 1px solid var(--border);
  border-radius: 28px;
  background:
    linear-gradient(135deg, rgba(15, 118, 110, 0.14), rgba(255, 255, 255, 0.72)),
    var(--surface);
}

.tutorial-hero h2,
.section-title h2,
.step-card h3 {
  margin: 0;
  color: var(--accent-dark);
  letter-spacing: -0.04em;
}

.tutorial-hero h2 {
  max-width: 820px;
  font-size: clamp(28px, 5vw, 50px);
  line-height: 1.05;
}

.tutorial-hero p:not(.eyebrow),
.step-card p,
.step-card li,
.operation-card li,
.safety-card li {
  color: var(--ink-soft);
  line-height: 1.8;
}

.tutorial-hero ol {
  display: grid;
  align-content: center;
  gap: 12px;
  margin: 0;
  padding: 22px 22px 22px 42px;
  border-radius: 22px;
  background: rgba(255, 255, 255, 0.68);
}

.tutorial-section {
  margin-top: 22px;
}

.section-title {
  margin-bottom: 14px;
}

.step-grid,
.operation-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px;
}

.step-card {
  padding: 22px;
  border: 1px solid var(--border);
  border-radius: 24px;
  background: var(--surface);
}

.step-card__header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.step-card__header a {
  color: var(--accent);
  font-weight: 800;
}

.step-card ul,
.operation-card ol,
.safety-card ul {
  margin: 12px 0 0;
  padding-left: 20px;
}

.operation-card {
  border-radius: 22px;
}

.safety-card {
  border-radius: 24px;
  background: rgba(255, 255, 255, 0.72);
}

@media (max-width: 900px) {
  .tutorial-hero,
  .step-grid,
  .operation-grid {
    grid-template-columns: 1fr;
  }
}
</style>
