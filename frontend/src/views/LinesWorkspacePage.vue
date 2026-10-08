<!-- 线路工作区以用户可见的绑定为主体，列表和路径共用同一读模型；不从 IP 猜测转送关系。 -->
<script setup lang="ts">
import { ArrowRight, Connection, Grid, List, Plus, Refresh, Search, Share } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import PageHeader from "@/components/PageHeader.vue";
import {
  apiClient,
  type AccessEntrySummary,
  type AccessEntryExitBindingSummary,
  type AccessNodeSummary,
  type ExitEndpointSummary,
  type LineGroupSummary,
} from "@/services/api";
import { endpointAddress, lineConfigStatus } from "@/views/lines/lineStatus";
const router = useRouter();
const loading = ref(true);
const saving = ref(false);
const loadError = ref("");
const keyword = ref("");
const nodeFilter = ref("");
const statusFilter = ref("");
const compactQuery = window.matchMedia("(max-width: 600px)");
const view = ref<"table" | "path">(compactQuery.matches ? "path" : "table");
const updateCompactView = () => {
  if (compactQuery.matches) view.value = "path";
};
compactQuery.addEventListener("change", updateCompactView);
onUnmounted(() => compactQuery.removeEventListener("change", updateCompactView));
const page = ref(1);
const pageSize = 15;
const entries = ref<AccessEntrySummary[]>([]);
const nodes = ref<AccessNodeSummary[]>([]);
const exits = ref<ExitEndpointSummary[]>([]);
const bindings = ref<AccessEntryExitBindingSummary[]>([]);
const groups = ref<LineGroupSummary[]>([]);
const selectedId = ref("");
const drawerOpen = ref(false);
const createOpen = ref(false);
const form = reactive({ entryId: "", exitId: "", name: "" });
const entryMap = computed(() => new Map(entries.value.map((x) => [x.id, x])));
const exitMap = computed(() => new Map(exits.value.map((x) => [x.id, x])));
const nodeMap = computed(() => new Map(nodes.value.map((x) => [x.id, x])));
const rows = computed(() =>
  bindings.value.map((binding) => {
    const entry = entryMap.value.get(binding.accessEntryId);
    const node = nodeMap.value.get(binding.accessNodeId);
    const exit = exitMap.value.get(binding.exitEndpointId);
    const memberships = groups.value.filter((group) => group.bindingNodeIds.includes(binding.id));
    return {
      ...binding,
      entry,
      node,
      exit,
      memberships,
      config: lineConfigStatus(binding.enabled, entry, node, exit),
      displayName: binding.name || binding.accessEntryName || "未命名线路",
      address: endpointAddress(
        entry?.cdnEnabled && entry.cdnHostname ? entry.cdnHostname : entry?.listenHost || "",
        entry?.listenPort || 0,
      ),
      protocol: entry
        ? [entry.protocol.toUpperCase(), entry.transport.toUpperCase(), entry.security.toUpperCase()].filter(Boolean).join(" · ")
        : "待配置",
    };
  }),
);
const filtered = computed(() => {
  const q = keyword.value.trim().toLowerCase();
  return rows.value.filter(
    (row) =>
      (!nodeFilter.value || row.accessNodeId === nodeFilter.value) &&
      (!statusFilter.value || row.config.label === statusFilter.value) &&
      (!q || [row.displayName, row.accessNodeName, row.exitEndpointName, row.address].some((value) => value.toLowerCase().includes(q))),
  );
});
const paged = computed(() => filtered.value.slice((page.value - 1) * pageSize, page.value * pageSize));
const selected = computed(() => rows.value.find((row) => row.id === selectedId.value));
const syncedCount = computed(() => rows.value.filter((row) => row.config.label === "已同步").length);
const availableEntries = computed(() => entries.value.filter((entry) => entry.enabled));
const availableExits = computed(() => exits.value.filter((exit) => exit.enabled && exit.exitResourceEnabled));
onMounted(load);
async function load() {
  loading.value = true;
  loadError.value = "";
  try {
    const [control, entryList, bindingList, exitList] = await Promise.all([
      apiClient.getControlPlane(),
      apiClient.listAccessEntries(),
      apiClient.listAccessEntryExitBindings(),
      apiClient.getExitEndpoints(),
    ]);
    nodes.value = control.accessNodes;
    groups.value = control.lineGroups;
    entries.value = entryList;
    bindings.value = bindingList;
    exits.value = exitList;
    page.value = Math.min(page.value, Math.max(1, Math.ceil(filtered.value.length / pageSize)));
  } catch (e) {
    loadError.value = e instanceof Error ? e.message : "读取线路失败";
  } finally {
    loading.value = false;
  }
}
function openRowDetails(row: { id: string }) {
  openDetails(row.id);
}
function openDetails(id: string) {
  selectedId.value = id;
  drawerOpen.value = true;
}
function openCreate() {
  form.entryId = availableEntries.value[0]?.id || "";
  form.exitId = availableExits.value[0]?.id || "";
  form.name = "";
  createOpen.value = true;
}
async function createLine() {
  if (!form.entryId || !form.exitId || !form.name.trim()) {
    ElMessage.warning("请填写线路名称，并选择入口与出口");
    return;
  }
  saving.value = true;
  try {
    await apiClient.createAccessEntryExitBinding(form.entryId, {
      exit_endpoint_id: form.exitId,
      name: form.name.trim(),
      enabled: true,
      sort_weight: 100,
    });
    createOpen.value = false;
    await load();
    ElMessage.success("线路已建立，请在分组中配置套餐授权");
  } catch (e) {
    ElMessage.error(e instanceof Error ? e.message : "建立线路失败");
  } finally {
    saving.value = false;
  }
}
function observedAt(value?: string) {
  if (!value) return "未上报";
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toLocaleString("zh-CN", { hour12: false }) : value;
}
function probeLabel(value?: string) {
  return (
    (
      { healthy: "探测正常", degraded: "探测降级", unhealthy: "探测失败", offline: "探测离线", unknown: "未确认" } as Record<string, string>
    )[value || "unknown"] || "未上报"
  );
}
function editEntry(id: string) {
  router.push({ path: "/admin/access-entries", query: { entry: id } });
}
</script>
<template>
  <PageHeader title="订阅线路" description="连接入口与出口，为用户提供清楚、可管理的服务线路。">
    <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
    <el-button type="primary" :icon="Plus" @click="openCreate">建立线路</el-button>
  </PageHeader>
  <div class="lines-pulse">
    <span
      ><strong>{{ bindings.length }}</strong> 条线路</span
    ><i /><span
      ><b class="lines-pulse__dot" /><strong>{{ syncedCount }}</strong> 已同步</span
    ><i /><span>{{ nodes.length }} 个节点</span><span class="lines-pulse__note">配置同步不等于客户端连通</span>
  </div>
  <el-alert v-if="loadError" :title="loadError" type="error" :closable="false" show-icon class="section-row" />
  <section class="lines-workspace">
    <div class="lines-toolbar">
      <el-input
        v-model="keyword"
        :prefix-icon="Search"
        placeholder="搜索线路、节点或地址"
        clearable
        aria-label="搜索线路"
        @input="page = 1"
      />
      <el-select v-model="nodeFilter" placeholder="全部节点" clearable aria-label="筛选节点" @change="page = 1"
        ><el-option v-for="node in nodes" :key="node.id" :label="node.name" :value="node.id"
      /></el-select>
      <el-select v-model="statusFilter" placeholder="全部状态" clearable aria-label="筛选状态" @change="page = 1"
        ><el-option v-for="s in ['已同步', '待同步', '已停用', '节点离线', '关联缺失']" :key="s" :label="s" :value="s"
      /></el-select>
      <div class="lines-view-toggle" role="group" aria-label="线路视图">
        <button :class="{ active: view === 'table' }" :aria-pressed="view === 'table'" aria-label="表格视图" @click="view = 'table'">
          <el-icon><List /></el-icon></button
        ><button :class="{ active: view === 'path' }" :aria-pressed="view === 'path'" aria-label="路径视图" @click="view = 'path'">
          <el-icon><Share /></el-icon>
        </button>
      </div>
    </div>
    <el-table v-if="view === 'table'" v-loading="loading" :data="paged" row-key="id" class="lines-table" @row-dblclick="openRowDetails">
      <el-table-column label="线路" min-width="230"
        ><template #default="{ row }"
          ><button class="line-name-button" @click="openDetails(row.id)">
            <span class="line-symbol"
              ><el-icon><Connection /></el-icon></span
            ><span
              ><strong>{{ row.displayName }}</strong
              ><small>{{ row.protocol }}</small></span
            >
          </button></template
        ></el-table-column
      >
      <el-table-column label="入口节点" min-width="150"
        ><template #default="{ row }"
          ><div class="line-cell">
            <span>{{ row.accessNodeName || "未配置" }}</span
            ><small>{{ row.address }}</small>
          </div></template
        ></el-table-column
      >
      <el-table-column label="出口" min-width="145"
        ><template #default="{ row }"
          ><div class="line-cell">
            <span>{{ row.exitEndpointName || "未配置" }}</span
            ><small>{{ row.exit?.outboundType.toUpperCase() || "—" }}</small>
          </div></template
        ></el-table-column
      >
      <el-table-column label="授权分组" min-width="135"
        ><template #default="{ row }"
          ><span v-if="!row.memberships.length" class="lines-unassigned">未加入分组</span>
          <div v-else class="line-groups">
            <el-tag v-for="g in row.memberships" :key="g.id" :type="g.enabled ? 'info' : 'warning'" effect="light">{{ g.name }}</el-tag>
          </div></template
        ></el-table-column
      >
      <el-table-column label="配置状态" width="112"
        ><template #default="{ row }"
          ><el-tag :type="row.config.type" effect="light">{{ row.config.label }}</el-tag></template
        ></el-table-column
      >
      <el-table-column label="" width="80" fixed="right"
        ><template #default="{ row }"
          ><el-button text type="primary" @click="openDetails(row.id)"
            >详情<el-icon class="line-detail-arrow"><ArrowRight /></el-icon></el-button></template
      ></el-table-column>
      <template #empty
        ><el-empty
          :description="loadError ? '线路数据读取失败' : keyword || nodeFilter || statusFilter ? '没有符合条件的线路' : '还没有订阅线路'"
          ><el-button v-if="!bindings.length && !loadError" type="primary" @click="openCreate">建立第一条线路</el-button></el-empty
        ></template
      >
    </el-table>
    <div v-else v-loading="loading" class="line-path-grid">
      <el-empty v-if="!paged.length" description="没有符合条件的线路" />
      <article v-for="row in paged" :key="row.id" class="line-path-card">
        <header>
          <div>
            <h2>{{ row.displayName }}</h2>
            <span>{{ row.protocol }}</span>
          </div>
          <el-tag :type="row.config.type" effect="light">{{ row.config.label }}</el-tag>
        </header>
        <div class="line-path">
          <div>
            <small>客户端公布入口</small><strong>{{ row.address }}</strong>
          </div>
          <el-icon><ArrowRight /></el-icon>
          <div>
            <small>处理节点</small><strong>{{ row.accessNodeName }}</strong>
          </div>
          <el-icon><ArrowRight /></el-icon>
          <div>
            <small>出口</small><strong>{{ row.exitEndpointName }}</strong>
          </div>
        </div>
        <footer>
          <span>{{ row.memberships.map((g) => g.name).join(" · ") || "未加入分组" }}</span
          ><el-button text type="primary" @click="openDetails(row.id)"
            >线路详情<el-icon><ArrowRight /></el-icon
          ></el-button>
        </footer>
      </article>
    </div>
    <footer class="lines-pagination">
      <span>共 {{ filtered.length }} 条线路</span
      ><el-pagination
        v-if="filtered.length > pageSize"
        v-model:current-page="page"
        :page-size="pageSize"
        :total="filtered.length"
        layout="prev, pager, next"
        small
      />
    </footer>
  </section>
  <div class="lines-guide-links">
    <RouterLink to="/admin/line-groups"
      ><el-icon><Grid /></el-icon>
      <div><strong>管理访问分组</strong><span>把线路加入分组，再授权给套餐</span></div>
      <el-icon><ArrowRight /></el-icon></RouterLink
    ><RouterLink to="/admin/rule-settings"
      ><el-icon><Share /></el-icon>
      <div><strong>订阅分流规则</strong><span>在同一个工作区管理订阅与规则</span></div>
      <el-icon><ArrowRight /></el-icon
    ></RouterLink>
  </div>
  <el-drawer v-model="drawerOpen" title="线路详情" size="520px">
    <template v-if="selected"
      ><div class="line-drawer-heading">
        <span class="line-symbol"
          ><el-icon><Connection /></el-icon
        ></span>
        <div>
          <h2>{{ selected.displayName }}</h2>
          <p>{{ selected.protocol }}</p>
        </div>
        <el-tag :type="selected.config.type">{{ selected.config.label }}</el-tag>
      </div>
      <div class="line-drawer-route">
        <div>
          <small>客户端公布入口</small><strong>{{ selected.address }}</strong>
        </div>
        <el-icon><ArrowRight /></el-icon>
        <div>
          <small>处理节点</small><strong>{{ selected.accessNodeName }}</strong>
        </div>
        <el-icon><ArrowRight /></el-icon>
        <div>
          <small>出口</small><strong>{{ selected.exitEndpointName }}</strong>
        </div>
      </div>
      <el-descriptions :column="1" border
        ><el-descriptions-item label="访问分组">{{
          selected.memberships.map((g) => g.name).join("、") || "未加入分组"
        }}</el-descriptions-item
        ><el-descriptions-item label="节点心跳">{{ observedAt(selected.node?.lastHeartbeatAt) }}</el-descriptions-item
        ><el-descriptions-item label="出口探测">{{ probeLabel(selected.exit?.lastProbeStatus) }}</el-descriptions-item
        ><el-descriptions-item label="备注">{{ selected.remark || "—" }}</el-descriptions-item></el-descriptions
      >
      <p class="line-state-note">这里分别展示配置与探测状态；实际连通性仍以客户端连接结果为准。</p>
      <div class="line-drawer-actions">
        <el-button type="primary" @click="editEntry(selected.accessEntryId)">管理入口与绑定</el-button
        ><el-button @click="router.push('/admin/line-groups')">分组授权</el-button>
      </div></template
    >
    <el-empty v-else description="此线路已不存在，请刷新列表" />
  </el-drawer>
  <el-dialog v-model="createOpen" title="建立订阅线路" width="540px">
    <p class="line-dialog-intro">选择现有入口和出口，建立一条用户可以订阅的线路。</p>
    <el-alert
      v-if="!availableEntries.length || !availableExits.length"
      title="需要先准备已启用的入口和出口"
      type="info"
      :closable="false"
      show-icon
    />
    <el-form label-position="top" class="line-create-form" @submit.prevent="createLine"
      ><el-form-item label="线路名称" required
        ><el-input v-model="form.name" placeholder="例如：日本 · 日常线路" maxlength="128" /></el-form-item
      ><el-form-item label="接入入口" required
        ><el-select v-model="form.entryId" placeholder="选择入口" filterable
          ><el-option
            v-for="entry in availableEntries"
            :key="entry.id"
            :value="entry.id"
            :label="`${entry.name} · ${entry.accessNodeName}`" /></el-select></el-form-item
      ><el-form-item label="出口线路" required
        ><el-select v-model="form.exitId" placeholder="选择出口" filterable
          ><el-option
            v-for="exit in availableExits"
            :key="exit.id"
            :value="exit.id"
            :label="exit.name || exit.resourceName" /></el-select></el-form-item
    ></el-form>
    <div class="line-dialog-links">
      <el-button link type="primary" @click="router.push({ path: '/admin/access-entries', query: { action: 'create' } })"
        >创建新入口</el-button
      ><el-button link type="primary" @click="router.push('/admin/line-pool')">管理出口</el-button>
    </div>
    <template #footer
      ><el-button @click="createOpen = false">取消</el-button
      ><el-button type="primary" :loading="saving" :disabled="!form.entryId || !form.exitId" @click="createLine"
        >建立线路</el-button
      ></template
    >
  </el-dialog>
</template>
<style scoped src="../styles/lines-workspace.css"></style>
