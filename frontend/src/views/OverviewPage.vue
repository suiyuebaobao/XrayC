<!-- 总览读取真实 summary 与节点状态，只展示日常决策所需信息，明细通过工作区下钻。 -->
<script setup lang="ts">
import { ArrowRight, Connection, Monitor, Plus, Refresh, SetUp, User } from "@element-plus/icons-vue";
import { computed, onMounted, ref } from "vue";
import PageHeader from "@/components/PageHeader.vue";
import { apiClient, type OverviewSummary, type AccessNodeSummary } from "@/services/api";
const loading = ref(true);
const error = ref("");
const summary = ref<OverviewSummary>();
const nodes = ref<AccessNodeSummary[]>([]);
const refreshedAt = ref("");
const synced = computed(() => nodes.value.filter((node) => node.configSynced && !node.configDirty).length);
const syncPercent = computed(() => (nodes.value.length ? Math.round((synced.value / nodes.value.length) * 100) : 0));
const metrics = computed(() => [
  { label: "活跃用户", value: summary.value?.activeUsers, icon: User, hint: "当前有效服务账号" },
  { label: "启用线路", value: summary.value?.activeAccessLines, icon: Connection, hint: "用户可授权的绑定线路" },
  { label: "接入节点", value: nodes.value.length, icon: SetUp, hint: "独立管理的服务器" },
  { label: "待同步节点", value: summary.value?.configDirtyNodes, icon: Monitor, hint: "等待应用最新配置" },
]);
onMounted(load);
async function load() {
  loading.value = true;
  error.value = "";
  try {
    const [overview, control] = await Promise.all([apiClient.getOverview(), apiClient.getControlPlane()]);
    summary.value = overview;
    nodes.value = control.accessNodes;
    refreshedAt.value = new Date().toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } catch (e) {
    error.value = e instanceof Error ? e.message : "总览读取失败";
  } finally {
    loading.value = false;
  }
}
function nodeState(node: AccessNodeSummary) {
  if (node.status === "offline" || node.healthStatus === "offline") return { label: "离线", type: "danger" as const };
  if (node.configDirty || !node.configSynced) return { label: "待同步", type: "warning" as const };
  return { label: "已同步", type: "success" as const };
}
</script>
<template>
  <PageHeader title="总览" description="查看服务状态，处理当前需要关注的工作。"
    ><span v-if="refreshedAt" class="overview-updated">更新于 {{ refreshedAt }}</span
    ><el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button></PageHeader
  >
  <el-alert v-if="error" :title="error" type="error" :closable="false" show-icon />
  <el-skeleton v-if="loading && !summary" :rows="8" animated />
  <template v-else-if="summary">
    <div class="overview-metrics">
      <article v-for="metric in metrics" :key="metric.label">
        <header>
          <span>{{ metric.label }}</span
          ><el-icon><component :is="metric.icon" /></el-icon>
        </header>
        <strong>{{ metric.value ?? "—" }}</strong>
        <p>{{ metric.hint }}</p>
      </article>
    </div>
    <div class="overview-grid">
      <section class="overview-panel">
        <header>
          <div>
            <h2>节点与配置</h2>
            <p>运行信息来自 Agent 上报</p>
          </div>
          <RouterLink to="/admin/transit-nodes"
            >全部节点<el-icon><ArrowRight /></el-icon
          ></RouterLink>
        </header>
        <div class="overview-sync">
          <div>
            <span>配置同步</span
            ><strong
              >{{ synced }} <small>/ {{ nodes.length }}</small></strong
            >
          </div>
          <el-progress :percentage="syncPercent" :show-text="false" :stroke-width="5" color="#8d88dc" />
        </div>
        <div v-if="nodes.length" class="overview-node-list">
          <RouterLink v-for="node in nodes.slice(0, 5)" :key="node.id" to="/admin/transit-nodes"
            ><span class="overview-node-icon"
              ><el-icon><SetUp /></el-icon
            ></span>
            <div>
              <strong>{{ node.name }}</strong
              ><small>{{ node.agentVersion || "Agent 版本未上报" }}</small>
            </div>
            <el-tag :type="nodeState(node).type" effect="light">{{ nodeState(node).label }}</el-tag></RouterLink
          >
        </div>
        <el-empty v-else description="尚未接入节点" :image-size="65" />
      </section>
      <section class="overview-panel overview-actions">
        <header>
          <div>
            <h2>日常工作</h2>
            <p>从这里继续管理你的服务</p>
          </div>
        </header>
        <RouterLink to="/admin/lines"
          ><span class="overview-action-icon"
            ><el-icon><Connection /></el-icon
          ></span>
          <div>
            <strong>管理订阅线路</strong>
            <p>入口、出口与分组，一处查看</p>
          </div>
          <el-icon><ArrowRight /></el-icon></RouterLink
        ><RouterLink to="/admin/users"
          ><span class="overview-action-icon"
            ><el-icon><User /></el-icon
          ></span>
          <div>
            <strong>用户与服务</strong>
            <p>查看套餐、订阅及使用情况</p>
          </div>
          <el-icon><ArrowRight /></el-icon></RouterLink
        ><RouterLink to="/admin/access-operations"
          ><span class="overview-action-icon"
            ><el-icon><Monitor /></el-icon
          ></span>
          <div>
            <strong>监控与排查</strong>
            <p>从异常状态进入详细记录</p>
          </div>
          <el-icon><ArrowRight /></el-icon
        ></RouterLink>
        <div class="overview-action-bottom">
          <el-icon><Plus /></el-icon><RouterLink to="/admin/transit-nodes">需要扩展容量？接入新的服务器</RouterLink>
        </div>
      </section>
    </div>
    <section class="overview-panel overview-events">
      <header>
        <div>
          <h2>近期事件</h2>
          <p>优先关注需要处理的变化</p>
        </div>
        <RouterLink to="/admin/audit-logs"
          >查看审计<el-icon><ArrowRight /></el-icon
        ></RouterLink>
      </header>
      <div v-if="summary.recentEvents.length" class="overview-event-list">
        <div v-for="event in summary.recentEvents.slice(0, 5)" :key="event.id">
          <span class="overview-event-dot" :class="event.level" /><strong>{{ event.title }}</strong
          ><time>{{ event.time }}</time>
        </div>
      </div>
      <el-empty v-else description="目前没有近期事件" :image-size="56" />
    </section>
  </template>
</template>
<style scoped src="../styles/overview.css"></style>
