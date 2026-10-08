<!--
  本弹窗承载中转节点 Agent 一键安装表单。
  组件只负责字段展示和双向绑定，提交动作交给 useAccessLinesPage。
  SSH 凭据不在本地持久化；弹窗每次打开由页面重置草稿。
-->
<script setup lang="ts">
import { computed } from 'vue';
import { ElMessage } from 'element-plus';
import type { OneClickInstallForm } from '@/views/access-lines/types';

const form = defineModel<OneClickInstallForm>('form', { required: true });

defineProps<{
  saving: boolean;
}>();

const emit = defineEmits<{
  (event: 'cancel'): void;
  (event: 'submit'): void;
}>();

// 填了域名直连地址或 CF 直连地址就要签证书，需要 ACME 邮箱；纯 IP 节点不需要。
const needsAcmeEmail = computed(
  () => Boolean(form.value.certDomain.trim() || form.value.cfDomain.trim()),
);

// SSH IP 是一键部署专用的独立字段，由管理员单独填写。
// 它与客户连接地址（公网地址 publicHost、IP 直连、域名直连、CF 直连）毫无关系，
// 绝不从任何客户地址自动带出——两者是两套独立的东西，不能联动、不能混。
// SSH 目标只能是 IP（域名尤其橙云无法直连 SSH），提交前由 isIpAddress 守卫。

// 判断字符串是否为合法 IPv4/IPv6（与后端 IpAddr 校验对齐）。
// 用 URL 解析 IPv6 的方括号形式，IPv4 走简单点分四段校验，避免引入额外依赖。
function isIpAddress(value: string): boolean {
  const text = value.trim();
  if (!text) {
    return false;
  }
  const ipv4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/;
  const match = text.match(ipv4);
  if (match) {
    return match.slice(1).every((part) => {
      const num = Number(part);
      return num >= 0 && num <= 255 && String(num) === part;
    });
  }
  // IPv6：借助 URL 解析 [::1] 形式，无方括号则补上再试。
  const bracketed = text.startsWith('[') ? text : `[${text}]`;
  try {
    const url = new URL(`http://${bracketed}`);
    return url.hostname.startsWith('[') || /:/.test(text);
  } catch {
    return false;
  }
}

// 提交前先校验 SSH Host 必须是 IP；非 IP 直接告警并阻断，不发请求。
function onSubmit() {
  if (!isIpAddress(form.value.sshHost)) {
    ElMessage.warning('SSH Host 必须是 IP，不能是域名（域名尤其橙云无法直连 SSH）');
    return;
  }
  emit('submit');
}
</script>

<template>
  <section class="node-form-content">
    <el-form label-position="top">
      <div class="form-section">
        <h3>节点信息</h3>
        <el-row :gutter="12">
          <el-col :xs="24" :sm="12">
            <el-form-item label="节点名称">
              <el-input v-model="form.nodeName" placeholder="例如：新加坡中转 01" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="公网端口">
              <el-input-number v-model="form.publicPort" :min="1" :max="65535" class="form-field" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="16">
            <el-form-item label="备注">
              <el-input v-model="form.remark" type="textarea" :rows="2" maxlength="512" show-word-limit placeholder="可留空" />
            </el-form-item>
          </el-col>
        </el-row>
      </div>

      <div class="form-section">
        <h3>SSH 连接</h3>
        <el-row :gutter="12">
          <el-col :xs="24" :sm="12">
            <el-form-item label="SSH Host（IP）">
              <el-input v-model="form.sshHost" placeholder="服务器公网 IP，例如 203.0.113.10" />
            </el-form-item>
          </el-col>
          <el-col :xs="12" :sm="6">
            <el-form-item label="SSH Port">
              <el-input-number v-model="form.sshPort" :min="1" :max="65535" class="form-field" />
            </el-form-item>
          </el-col>
          <el-col :xs="12" :sm="6">
            <el-form-item label="SSH User">
              <el-input v-model="form.sshUser" placeholder="root" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="SSH 凭据">
              <el-radio-group v-model="form.sshAuthMethod">
                <el-radio-button label="password">密码</el-radio-button>
                <el-radio-button label="privateKey">私钥</el-radio-button>
              </el-radio-group>
            </el-form-item>
          </el-col>
          <el-col v-if="form.sshAuthMethod === 'password'" :xs="24" :sm="16">
            <el-form-item label="SSH 密码">
              <el-input v-model="form.sshPassword" type="password" show-password autocomplete="new-password" />
            </el-form-item>
          </el-col>
          <el-col v-else :xs="24" :sm="16">
            <el-form-item label="SSH 私钥">
              <el-input v-model="form.sshPrivateKey" type="textarea" :rows="5" />
            </el-form-item>
          </el-col>
        </el-row>
      </div>

      <div class="form-section">
        <h3>安装参数</h3>
        <el-alert
          class="install-alert"
          type="warning"
          show-icon
          :closable="false"
          title="一键安装会自动连接服务器、上传并执行脚本；无需手动上传。会先检查中心连线与安装文件，再更新节点运行环境。"
        />
        <el-row :gutter="12">
          <el-col :span="24">
            <el-divider content-position="left">对外直连地址（三选一即可，可多填）</el-divider>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="IP 直连地址">
              <el-input v-model="form.ipDirectAddress" placeholder="公网 IP，例如 203.0.113.10（Reality/SS）" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="域名直连地址">
              <el-input v-model="form.certDomain" placeholder="灰云域名，例如 hk-relay.example.com" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="8">
            <el-form-item label="CF 直连地址">
              <el-input v-model="form.cfDomain" placeholder="橙云域名，例如 cf-entry.example.com（填了即启用 CF）" />
            </el-form-item>
          </el-col>
          <el-col v-if="needsAcmeEmail" :xs="24" :sm="12">
            <el-form-item label="ACME 邮箱">
              <el-input v-model="form.acmeEmail" placeholder="ops@example.com（签发域名/CF 证书必填）" />
            </el-form-item>
          </el-col>
          <el-col v-if="form.cfDomain.trim()" :xs="24" :sm="12">
            <el-form-item label="CF API Token">
              <el-input
                v-model="form.cfApiToken"
                type="password"
                show-password
                autocomplete="new-password"
                placeholder="可选，仅安装期用于 DNS-01 签证书，不保存"
              />
            </el-form-item>
          </el-col>
          <el-col v-if="form.cfDomain.trim()" :span="24">
            <el-alert
              class="cf-note"
              type="info"
              show-icon
              :closable="false"
              title="填了 CF 直连地址即启用 CF；提供 CF API Token 时用 DNS-01 给 CF 域名签自己的证书（token 仅安装期写节点本机 ini，不保存），否则兜底复用域名直连证书（需 CF SSL=Full 非 strict）。"
            />
          </el-col>
        </el-row>
      </div>
      <el-collapse>
        <el-collapse-item title="高级安装选项" name="advanced">
          <el-row :gutter="12">
          <el-col :xs="24" :sm="12">
            <el-form-item label="安装目录">
              <el-input v-model="form.installDir" />
            </el-form-item>
          </el-col>
          <el-col :xs="24" :sm="12">
            <el-form-item label="Compose Project">
              <el-input v-model="form.composeProject" />
            </el-form-item>
          </el-col>
          </el-row>
        </el-collapse-item>
      </el-collapse>
    </el-form>

    <div class="node-form-actions">
      <el-button @click="emit('cancel')" :disabled="saving">取消</el-button>
      <el-button type="primary" :loading="saving" @click="onSubmit">创建安装任务</el-button>
    </div>
  </section>
</template>

<style scoped>
.node-form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 20px; }
.form-section + .form-section {
  border-top: 1px solid var(--el-border-color-lighter);
  margin-top: 8px;
  padding-top: 12px;
}

.form-section h3 {
  color: var(--ink);
  font-size: 14px;
  line-height: 1.4;
  margin: 0 0 12px;
}

.install-alert {
  margin-bottom: 12px;
}

.cf-note {
  margin-bottom: 12px;
}

.form-field {
  width: 100%;
}
</style>
