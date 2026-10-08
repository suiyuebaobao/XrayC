<!-- 后台工作区布局：分组导航、上下文页签和移动端抽屉；不改变现有路由权限或业务 API。 -->
<script setup lang="ts">
import {
  DataBoard,
  Connection,
  User,
  Tickets,
  SetUp,
  Monitor,
  Setting,
  Document,
  Present,
  UserFilled,
  Lock,
  QuestionFilled,
  TopRight,
  SwitchButton,
  Fold,
  ArrowRight,
} from "@element-plus/icons-vue";
import { computed, onMounted, onUnmounted, ref, watch, type Component } from "vue";
import { useRoute, useRouter } from "vue-router";
import { usePortalFeaturesStore } from "@/stores/portalFeatures";
import { useSessionStore } from "@/stores/session";
import { adminWorkspaces, userNavigation, workspaceForPath } from "./navigation";

const icons = {
  DataBoard,
  Connection,
  User,
  Tickets,
  SetUp,
  Monitor,
  Setting,
  Document,
  Present,
  UserFilled,
  Lock,
  QuestionFilled,
  TopRight,
  SwitchButton,
  Fold,
  ArrowRight,
};
const route = useRoute();
const router = useRouter();
const session = useSessionStore();
const portal = usePortalFeaturesStore();
onMounted(() => { void portal.load(); });
const userPath = computed(() => route.path === '/subscription' ? '/dashboard' : route.path);
const visibleUserNavigation = computed(() => userNavigation.filter((item) => {
  const feature = ({ '/plans': 'plans', '/orders': 'orders', '/redeem': 'redeem', '/invite-codes': 'invites' } as const)[item.path as '/plans' | '/orders' | '/redeem' | '/invite-codes'];
  return !feature || portal.features[feature];
}));
const navigationOpen = ref(false);
const mobileQuery = window.matchMedia("(max-width: 760px)");
const isMobile = ref(mobileQuery.matches);
const updateMobile = () => {
  isMobile.value = mobileQuery.matches;
  if (!isMobile.value) navigationOpen.value = false;
};
mobileQuery.addEventListener("change", updateMobile);
onUnmounted(() => mobileQuery.removeEventListener("change", updateMobile));
const workspace = computed(() => workspaceForPath(route.path));
const sections = ["日常管理", "运维", "系统"] as const;
const currentLabel = computed(() =>
  session.isAdmin
    ? (workspace.value?.label ?? (route.path === "/admin/tutorial" ? "帮助中心" : "控制台"))
    : (userNavigation.find((item) => item.path === userPath.value)?.label ?? "我的账户"),
);
const currentTab = computed(() => workspace.value?.tabs.find((tab) => tab.path === route.path));
const preview = import.meta.env.VITE_UI_PREVIEW === "true";
const iconFor = (name: string) => (icons as Record<string, Component>)[name] ?? icons.Setting;
watch(
  () => route.fullPath,
  () => {
    navigationOpen.value = false;
  },
);
async function logout() {
  await session.logout();
  await router.push("/login");
}
</script>

<template>
  <div class="console-shell" @keydown.esc="navigationOpen = false">
    <button v-if="navigationOpen" class="console-backdrop" aria-label="关闭导航" @click="navigationOpen = false" />
    <aside
      id="workspace-navigation"
      class="console-sidebar"
      :class="{ 'is-open': navigationOpen }"
      :inert="isMobile && !navigationOpen"
      :aria-hidden="isMobile && !navigationOpen ? true : undefined"
      aria-label="工作区导航"
    >
      <RouterLink class="console-brand" :to="session.isAdmin ? '/overview' : '/dashboard'">
        <span class="console-brand__mark"
          ><el-icon><icons.Connection /></el-icon
        ></span>
        <span>XrayC<small>CONTROL CENTER</small></span>
      </RouterLink>
      <div class="console-space">
        <span class="console-space__dot" /><span>{{ session.isAdmin ? "运营工作区" : "用户中心" }}</span
        ><small>{{ preview ? "预览" : "V2" }}</small>
      </div>
      <nav class="console-navigation">
        <template v-if="session.isAdmin">
          <section v-for="section in sections" :key="section" class="console-navgroup">
            <p class="console-navgroup__label">{{ section }}</p>
            <RouterLink
              v-for="item in adminWorkspaces.filter((entry) => entry.section === section)"
              :key="item.id"
              :to="item.path"
              class="console-navitem"
              :class="{ 'is-active': workspace?.id === item.id }"
              :aria-current="workspace?.id === item.id ? 'page' : undefined"
            >
              <el-icon><component :is="iconFor(item.icon)" /></el-icon><span>{{ item.label }}</span>
              <span v-if="workspace?.id === item.id" class="console-navitem__marker" />
            </RouterLink>
          </section>
        </template>
        <section v-else class="console-navgroup">
          <RouterLink
            v-for="item in visibleUserNavigation"
            :key="item.path"
            :to="item.path"
            class="console-navitem"
            :class="{ 'is-active': userPath === item.path }"
            :aria-current="userPath === item.path ? 'page' : undefined"
          >
            <el-icon><component :is="iconFor(item.icon)" /></el-icon><span>{{ item.label }}</span>
          </RouterLink>
        </section>
      </nav>
      <div class="console-sidebar__footer">
        <RouterLink v-if="session.isAdmin" to="/admin/tutorial" class="console-help"
          ><el-icon><icons.QuestionFilled /></el-icon>帮助与教程<el-icon><icons.TopRight /></el-icon
        ></RouterLink>
        <div class="console-profile">
          <span class="console-avatar">{{ (session.user?.name || session.user?.account || "A").slice(0, 1).toUpperCase() }}</span>
          <div>
            <strong>{{ session.user?.account || session.user?.name }}</strong
            ><small>{{ session.isAdmin ? "管理员" : "用户" }}</small>
          </div>
          <el-button text class="console-logout" aria-label="退出登录" @click="logout"
            ><el-icon><icons.SwitchButton /></el-icon
          ></el-button>
        </div>
      </div>
    </aside>
    <div class="console-body">
      <header class="console-topbar">
        <div class="console-breadcrumb">
          <el-button
            text
            class="console-menu-toggle"
            aria-label="打开导航"
            :aria-expanded="navigationOpen"
            aria-controls="workspace-navigation"
            @click="navigationOpen = !navigationOpen"
            ><el-icon><icons.Fold /></el-icon></el-button
          ><span>{{ session.isAdmin ? "工作区" : "用户中心" }}</span
          ><el-icon><icons.ArrowRight /></el-icon><strong>{{ currentLabel }}</strong
          ><template v-if="currentTab"
            ><el-icon><icons.ArrowRight /></el-icon><span class="console-breadcrumb__current">{{ currentTab.label }}</span></template
          >
        </div>
        <div class="console-topbar__right">
          <span v-if="preview" class="console-preview-label">本机预览 · 示例数据</span><span v-else class="console-version">XrayC V2</span
          ><span class="console-avatar console-avatar--small">{{ session.isAdmin ? "A" : "U" }}</span>
        </div>
      </header>
      <nav v-if="session.isAdmin && workspace && workspace.tabs.length > 1" class="workspace-tabs" :aria-label="`${workspace.label}分类`">
        <RouterLink
          v-for="tab in workspace.tabs"
          :key="tab.path"
          :to="tab.path"
          :class="{ 'is-active': route.path === tab.path }"
          :aria-current="route.path === tab.path ? 'page' : undefined"
          >{{ tab.label }}</RouterLink
        >
      </nav>
      <main class="console-main"><RouterView /></main>
      <footer class="console-footnote">
        XrayC <span>·</span> {{ session.isAdmin ? "配置、授权与运行状态，清楚可见。" : "你的服务与订阅。" }}
      </footer>
    </div>
  </div>
</template>
