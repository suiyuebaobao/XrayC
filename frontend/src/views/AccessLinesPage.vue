<!--
  本文件是后台“中转节点”页面入口。
  它只负责页面骨架、顶部动作和拆分组件编排。
  数据加载、表单状态与保存动作放在 views/access-lines/useAccessLinesPage.ts。
  摘要、节点卡片和各弹窗均拆到 views/access-lines 目录。
-->
<script setup lang="ts">
import PageHeader from '@/components/PageHeader.vue';
import AccessLinesSummaryCards from '@/views/access-lines/AccessLinesSummaryCards.vue';
import DeploymentTasksCard from '@/views/access-lines/DeploymentTasksCard.vue';
import AgentInstallGuideDialog from '@/views/access-lines/AgentInstallGuideDialog.vue';
import CreateAccessNodeDialog from '@/views/access-lines/CreateAccessNodeDialog.vue';
import LocalExitLinesDialog from '@/views/access-lines/LocalExitLinesDialog.vue';
import OneClickAgentInstallDialog from '@/views/access-lines/OneClickAgentInstallDialog.vue';
import RelayNodeCard from '@/views/access-lines/RelayNodeCard.vue';
import { useAccessLinesPage } from '@/views/access-lines/useAccessLinesPage';

const {
  loading,
  saving,
  nodes,
  deploymentTasks,
  exitEndpoints,
  nodeDialogOpen,
  installGuideDialogOpen,
  oneClickInstallDialogOpen,
  localExitDialogOpen,
  selectedNodeIds,
  nodeDialogMode,
  installGuide,
  localExitNode,
  nodeForm,
  oneClickInstallForm,
  localExitForm,
  relayNodes,
  localExitNodeUsedPorts,
  availableEndpointCount,
  totalLineCount,
  load,
  openCreateNodeDialog,
  openEditNodeDialog,
  openOneClickInstallDialog,
  saveRelayNode,
  toggleNodeSelection,
  deleteRelayNode,
  deleteSelectedRelayNodes,
  openLocalExitLinesDialog,
  openInstallGuideDialog,
  saveLocalExitLines,
  generateAgentInstallGuide,
  submitOneClickInstall,
  retryOneClickInstallFromTask,
  deleteDeploymentTask,
  renewNodeTls,
  renewingTlsNodeId,
  rebootRelayNode,
  rebootingNodeId,
} = useAccessLinesPage();
</script>

<template>
  <PageHeader
    title="中转节点"
    description="登记中转节点，生成 Agent 安装说明，查看运行入口与本机出口服务。入口协议和出口绑定请到入口管理维护。"
  >
    <el-button @click="openInstallGuideDialog">Agent 安装说明</el-button>
    <el-button type="success" @click="openOneClickInstallDialog">一键安装 Agent</el-button>
    <el-button type="primary" @click="openCreateNodeDialog">新增中转节点</el-button>
    <el-button :disabled="selectedNodeIds.length === 0" :loading="saving" @click="deleteSelectedRelayNodes">
      批量删除节点
    </el-button>
  </PageHeader>

  <AccessLinesSummaryCards
    :node-count="nodes.length"
    :total-line-count="totalLineCount"
    :available-line-count="availableEndpointCount"
    :line-pool-count="exitEndpoints.length"
  />

  <DeploymentTasksCard
    :tasks="deploymentTasks"
    @retry="retryOneClickInstallFromTask"
    @delete="deleteDeploymentTask"
  />

  <el-empty v-if="!loading && relayNodes.length === 0" description="还没有中转节点，请先新增节点。" />

  <RelayNodeCard
    v-for="node in relayNodes"
    :key="node.id"
    :node="node"
    :loading="loading"
    :selected="selectedNodeIds.includes(node.id)"
    :renewing-tls="renewingTlsNodeId === node.id"
    :rebooting="rebootingNodeId === node.id"
    @selection-change="toggleNodeSelection"
    @edit="openEditNodeDialog"
    @delete="deleteRelayNode"
    @local-exit="openLocalExitLinesDialog"
    @renew-tls="renewNodeTls"
    @reboot="rebootRelayNode"
  />

  <CreateAccessNodeDialog
    v-model="nodeDialogOpen"
    v-model:form="nodeForm"
    :mode="nodeDialogMode"
    :saving="saving"
    @save="saveRelayNode"
  />

  <AgentInstallGuideDialog
    v-model="installGuideDialogOpen"
    :saving="saving"
    :install-guide="installGuide"
    @generate="generateAgentInstallGuide"
  />

  <OneClickAgentInstallDialog
    v-model="oneClickInstallDialogOpen"
    v-model:form="oneClickInstallForm"
    :saving="saving"
    @submit="submitOneClickInstall"
  />

  <LocalExitLinesDialog
    v-model="localExitDialogOpen"
    v-model:form="localExitForm"
    :node="localExitNode"
    :used-ports="localExitNodeUsedPorts"
    :saving="saving"
    @save="saveLocalExitLines"
    @changed="load"
  />
</template>
