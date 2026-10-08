<!--
  本文件承载中转节点 Agent 手动安装说明弹窗。
  后台生成通用安装说明模板，不要求先创建中转节点。
  不收集 SSH 密码，不连接服务器，也不执行远端安装。
  节点鉴权码由服务器安装脚本生成，再回填到新增节点表单。
  订阅节点通过入口管理绑定出口后生成。
-->
<script setup lang="ts">
import type { InstallGuideView } from '@/views/access-lines/types';


defineProps<{
  saving: boolean;
  installGuide: InstallGuideView | null;
}>();

const emit = defineEmits<{
  (event: 'cancel'): void;
  (event: 'generate'): void;
}>();
</script>

<template>
  <section class="node-form-content">
    <el-alert
      class="dialog-tip"
      type="info"
      show-icon
      :closable="false"
      title="这是安装说明模板；后台不会创建节点、连接服务器或执行远端安装。"
    />
    <el-alert
      class="dialog-tip"
      type="warning"
      show-icon
      :closable="false"
      title="命令模板里的 XRAYC_DEPLOY_ARTIFACT_TOKEN 请替换为制品下载鉴权码；安装成功后服务器会输出节点鉴权码。"
    />

    <section v-if="installGuide" class="install-guide">
      <el-alert class="dialog-tip" type="success" show-icon :closable="false" :title="installGuide.summary" />
      <el-descriptions v-if="installGuide.task" class="install-task" :column="2" border>
        <el-descriptions-item label="部署任务">{{ installGuide.task.id }}</el-descriptions-item>
        <el-descriptions-item label="任务状态">{{ installGuide.task.status }}</el-descriptions-item>
        <el-descriptions-item label="当前步骤">{{ installGuide.task.currentStep }}</el-descriptions-item>
        <el-descriptions-item label="进度">
          <el-progress :percentage="installGuide.task.progressPercent" />
        </el-descriptions-item>
      </el-descriptions>
      <el-form label-position="top">
        <el-form-item label="服务器执行命令">
          <el-input :model-value="installGuide.installCommand" type="textarea" :rows="10" readonly />
        </el-form-item>
        <el-form-item label="环境变量">
          <el-input :model-value="installGuide.environmentText" type="textarea" :rows="8" readonly />
        </el-form-item>
      </el-form>
      <el-timeline>
        <el-timeline-item v-for="step in installGuide.steps" :key="step.title" :timestamp="step.title">
          {{ step.detail }}
        </el-timeline-item>
      </el-timeline>
    </section>
    <el-empty v-else description="点击生成说明文本，复制到服务器后按提示替换占位值。" />

    <div class="node-form-actions">
      <el-button @click="emit('cancel')" :disabled="saving">关闭</el-button>
      <el-button type="primary" :loading="saving" @click="emit('generate')">生成说明文本</el-button>
    </div>
  </section>
</template>

<style scoped>
.node-form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
.dialog-tip {
  margin-bottom: 16px;
}

.install-guide {
  margin-top: 16px;
}

.install-task {
  margin-bottom: 16px;
}
</style>
