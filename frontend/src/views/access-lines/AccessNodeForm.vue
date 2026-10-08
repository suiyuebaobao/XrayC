<!--
  本文件承载新增和编辑中转节点的基础表单弹窗。
  它只维护表单展示和输入绑定，不直接调用 API。
  保存动作通过 save 事件交给 useAccessLinesPage 统一处理。
  拆出后 AccessLinesPage 不再包含节点表单模板。
-->
<script setup lang="ts">
import { computed } from 'vue';
import NodeDomainsField from '@/views/access-lines/NodeDomainsField.vue';
import type { NodeForm } from '@/views/access-lines/types';

const form = defineModel<NodeForm>('form', { required: true });

defineProps<{
  mode: 'create' | 'edit';
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'cancel'): void;
  (event: 'save'): void;
}>();

// 填了任意域名直连或 CF 直连地址就要签证书，需要 ACME 邮箱；纯 IP 节点不需要。
const needsAcmeEmail = computed(
  () => form.value.domains.some((row) => row.domain.trim()),
);

// 是否含 CF 直连域名（任一行 kind=cf 且填了域名）：决定是否展示 CF 启用提示。
const hasCfDomain = computed(
  () => form.value.domains.some((row) => row.kind === 'cf' && row.domain.trim()),
);
</script>

<template>
  <section class="node-form-content">
    <el-form label-position="top">
      <el-form-item label="节点名称">
        <el-input v-model="form.name" placeholder="例如：香港中转 01" />
      </el-form-item>
      <el-row :gutter="12">
        <el-col :span="8">
          <el-form-item label="端口">
            <el-input-number v-model="form.publicPort" :min="1" :max="65535" class="form-number" />
          </el-form-item>
        </el-col>
      </el-row>
      <el-form-item label="SSH IP">
        <el-input v-model="form.sshHost" placeholder="服务器 SSH 登录 IP 或域名，可和客户连接地址不同" />
      </el-form-item>
      <el-divider content-position="left">对外直连地址（IP / 域名 / CF 至少填一个，域名可多填）</el-divider>
      <el-form-item label="IP 直连地址">
        <el-input v-model="form.ipDirectAddress" placeholder="公网 IP，例如 203.0.113.10（用于 Reality / Shadowsocks 直连）" />
      </el-form-item>
      <NodeDomainsField v-model="form.domains" />
      <el-form-item v-if="needsAcmeEmail" label="ACME 邮箱">
        <el-input v-model="form.acmeEmail" placeholder="ops@example.com（签发域名/CF 证书必填）" />
      </el-form-item>
      <el-alert
        v-if="hasCfDomain"
        class="cf-note"
        type="info"
        show-icon
        :closable="false"
        title="填了 CF 直连地址即启用 CF；提供 CF API Token 时用 DNS-01 给该 CF 域名签自己的证书，否则兜底复用域名直连证书（需 CF SSL=Full 非 strict）。"
      />
      <el-form-item v-if="mode === 'create'" label="鉴权码">
        <el-input v-model="form.agentToken" type="password" show-password placeholder="服务器安装 agent 成功后输出的节点鉴权码" />
      </el-form-item>
      <el-form-item label="备注">
        <el-input v-model="form.remark" type="textarea" :rows="3" maxlength="512" show-word-limit placeholder="可留空" />
      </el-form-item>
    </el-form>
    <div class="node-form-actions">
      <el-button @click="emit('cancel')" :disabled="saving">取消</el-button>
      <el-button type="primary" :loading="saving" @click="emit('save')">保存</el-button>
    </div>
  </section>
</template>

<style scoped>
.node-form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
.form-number {
  width: 100%;
}

.cf-note {
  margin-bottom: 18px;
}
</style>
