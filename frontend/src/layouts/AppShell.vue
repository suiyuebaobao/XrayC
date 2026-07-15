<!--
  本布局用于登录后的用户后台和管理员后台。
  它渲染侧边菜单、顶部栏和当前子路由内容。
  菜单权限依据当前 session 角色计算。
-->
<script setup lang="ts">
import {
  Brush,
  Collection,
  Connection,
  CreditCard,
  Discount,
  DocumentChecked,
  FolderChecked,
  HomeFilled,
  Lock,
  Monitor,
  Money,
  Reading,
  Setting,
  SwitchButton,
  Tickets,
  User,
} from '@element-plus/icons-vue';
import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useSessionStore } from '@/stores/session';

const route = useRoute();
const router = useRouter();
const session = useSessionStore();

const activePath = computed(() => route.path);

async function logout() {
  await session.logout();
  router.push('/login');
}
</script>

<template>
  <el-container class="shell">
    <el-aside class="shell__aside" width="236px">
      <div class="brand">
        <div class="brand__mark">XC</div>
        <div>
          <strong>XrayC</strong>
          <span>V2 运营控制台</span>
        </div>
      </div>

      <el-menu :default-active="activePath" router class="shell__menu">
        <el-menu-item v-if="session.isAdmin" index="/overview">
          <el-icon><HomeFilled /></el-icon>
          <span>概览</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/tutorial">
          <el-icon><Reading /></el-icon>
          <span>使用教程</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/access-operations">
          <el-icon><Monitor /></el-icon>
          <span>监控中心</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/transit-nodes">
          <el-icon><Connection /></el-icon>
          <span>中转节点</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/access-entries">
          <el-icon><Connection /></el-icon>
          <span>入口管理</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/line-pool">
          <el-icon><Connection /></el-icon>
          <span>出口管理</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/line-groups">
          <el-icon><Monitor /></el-icon>
          <span>分组</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/rule-settings">
          <el-icon><Setting /></el-icon>
          <span>规则设置</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/plans">
          <el-icon><Collection /></el-icon>
          <span>套餐授权</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/users">
          <el-icon><User /></el-icon>
          <span>用户管理</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/traffic-logs">
          <el-icon><Tickets /></el-icon>
          <span>用户流量日志</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/orders">
          <el-icon><Money /></el-icon>
          <span>订单</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/redeem-codes">
          <el-icon><Discount /></el-icon>
          <span>兑换码</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/invite-codes">
          <el-icon><Tickets /></el-icon>
          <span>邀请码</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/auth-security">
          <el-icon><Lock /></el-icon>
          <span>认证安全</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/payment-settings">
          <el-icon><CreditCard /></el-icon>
          <span>支付设置</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/audit-logs">
          <el-icon><DocumentChecked /></el-icon>
          <span>审计日志</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/database-backup">
          <el-icon><FolderChecked /></el-icon>
          <span>数据库备份</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/subscription-settings">
          <el-icon><Setting /></el-icon>
          <span>订阅设置</span>
        </el-menu-item>
        <el-menu-item v-if="session.isAdmin" index="/admin/sales-landing">
          <el-icon><Brush /></el-icon>
          <span>销售页配置</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/dashboard">
          <el-icon><HomeFilled /></el-icon>
          <span>首页</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/plans">
          <el-icon><Collection /></el-icon>
          <span>套餐</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/subscription">
          <el-icon><Tickets /></el-icon>
          <span>订阅</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/orders">
          <el-icon><Collection /></el-icon>
          <span>我的订单</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/redeem">
          <el-icon><Tickets /></el-icon>
          <span>兑换码</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/invite-codes">
          <el-icon><Tickets /></el-icon>
          <span>邀请码</span>
        </el-menu-item>
        <el-menu-item v-if="!session.isAdmin" index="/account/password">
          <el-icon><Lock /></el-icon>
          <span>修改密码</span>
        </el-menu-item>
      </el-menu>
    </el-aside>

    <el-container>
      <el-header class="shell__header">
        <div class="shell__hint">
          <el-icon><Connection /></el-icon>
          核心配置：出口管理先加线路，入口管理绑定出口，分组用于套餐授权和归类。
        </div>
        <div class="shell__user">
          <span>{{ session.user?.name }}</span>
          <el-button :icon="SwitchButton" text @click="logout">退出</el-button>
        </div>
      </el-header>
      <el-main class="shell__main">
        <RouterView />
      </el-main>
    </el-container>
  </el-container>
</template>
