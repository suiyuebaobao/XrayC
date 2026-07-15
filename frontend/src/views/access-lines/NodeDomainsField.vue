<!--
  本子组件维护中转节点的多域名列表表单（直连 + CF 各成一组）。
  每组可增删多行：每行一个域名输入框 + 删除按钮 + 同组唯一的「设为主域名」单选。
  CF 行额外带 CF API Token（仅安装期透传做 DNS-01，不写入控制台保存载荷）。
  只做表单绑定，不调用接口；保存动作由父弹窗经事件交给页面统一处理。
-->
<script setup lang="ts">
import { computed } from 'vue';
import type { NodeDomainFormRow } from '@/views/access-lines/types';

// 双向绑定父表单的 domains 多行列表。
const domains = defineModel<NodeDomainFormRow[]>({ required: true });

// 为每个域名行对象分配稳定 key:用 WeakMap 按对象身份缓存自增 id，不随数组 splice 变化。
// （此前用数组下标当 :key，删行后下标整体前移、Vue 按位置复用输入框/单选实例，
// 导致行内容与主域名选中态整体串位——表现为「删不掉、设置会乱」。行对象在增删期间引用稳定，故 key 稳定。）
const rowKeys = new WeakMap<NodeDomainFormRow, number>();
let rowKeySeq = 0;
function keyFor(row: NodeDomainFormRow): number {
  let key = rowKeys.get(row);
  if (key === undefined) {
    rowKeySeq += 1;
    key = rowKeySeq;
    rowKeys.set(row, key);
  }
  return key;
}

// 按 kind 拆出两组视图，带稳定 key 与原对象引用（就地改字段触发响应式）。
const directRows = computed(() =>
  domains.value.filter((row) => row.kind === 'direct').map((row) => ({ row, key: keyFor(row) })),
);
const cfRows = computed(() =>
  domains.value.filter((row) => row.kind === 'cf').map((row) => ({ row, key: keyFor(row) })),
);

function addRow(kind: NodeDomainFormRow['kind']) {
  // 同组第一行默认设为主域名，避免一组里没有主域名。
  const hasPrimary = domains.value.some((row) => row.kind === kind && row.isPrimary);
  domains.value.push({ domain: '', kind, isPrimary: !hasPrimary, cfApiToken: '' });
}

function removeRow(row: NodeDomainFormRow) {
  const index = domains.value.indexOf(row);
  if (index < 0) {
    return;
  }
  const removedPrimary = row.isPrimary;
  const kind = row.kind;
  domains.value.splice(index, 1);
  // 删掉主域名后，把同组第一行顶上来当主域名，保持每组至多/至少一个主域名。
  if (removedPrimary) {
    const next = domains.value.find((item) => item.kind === kind);
    if (next) {
      next.isPrimary = true;
    }
  }
}

// 同组互斥地设主域名：选中行置真，同组其它行置假。
function setPrimary(row: NodeDomainFormRow) {
  domains.value.forEach((item) => {
    if (item.kind === row.kind) {
      item.isPrimary = item === row;
    }
  });
}
</script>

<template>
  <div class="node-domains">
    <div class="node-domains__group">
      <div class="node-domains__group-head">
        <span class="node-domains__label">域名直连地址（灰云，HTTP-01 签证书，可多填）</span>
      </div>
      <p v-if="directRows.length === 0" class="node-domains__empty">
        暂无域名直连地址，点击下方按钮添加，例如 hk-relay.example.com。
      </p>
      <div v-for="item in directRows" :key="item.key" class="node-domains__row">
        <el-input
          v-model="item.row.domain"
          class="node-domains__input"
          placeholder="灰云域名，例如 hk-relay.example.com（直连 Trojan/HY2/WS/gRPC/XHTTP+TLS）"
        />
        <el-radio
          :model-value="item.row.isPrimary"
          :value="true"
          class="node-domains__primary"
          @change="setPrimary(item.row)"
        >
          主域名
        </el-radio>
        <el-button text type="danger" @click="removeRow(item.row)">删除</el-button>
      </div>
      <el-button class="node-domains__add" plain @click="addRow('direct')">添加域名直连地址</el-button>
    </div>

    <div class="node-domains__group">
      <div class="node-domains__group-head">
        <span class="node-domains__label">CF 直连地址（橙云，填了即启用 CF，DNS-01 签自己证书，可多填）</span>
      </div>
      <p v-if="cfRows.length === 0" class="node-domains__empty">
        暂无 CF 直连地址，点击下方按钮添加，例如 cf-entry.example.com。
      </p>
      <div v-for="item in cfRows" :key="item.key" class="node-domains__cf-row">
        <div class="node-domains__row">
          <el-input
            v-model="item.row.domain"
            class="node-domains__input"
            placeholder="橙云域名，例如 cf-entry.example.com（填了即启用 CF）"
          />
          <el-radio
            :model-value="item.row.isPrimary"
            :value="true"
            class="node-domains__primary"
            @change="setPrimary(item.row)"
          >
            主域名
          </el-radio>
          <el-button text type="danger" @click="removeRow(item.row)">删除</el-button>
        </div>
        <el-input
          v-model="item.row.cfApiToken"
          type="password"
          show-password
          autocomplete="new-password"
          class="node-domains__token"
          placeholder="CF API Token（可选，仅安装期透传做 DNS-01，不保存）"
        />
      </div>
      <el-button class="node-domains__add" plain @click="addRow('cf')">添加 CF 直连地址</el-button>
    </div>
  </div>
</template>

<style scoped>
.node-domains__group {
  margin-bottom: 18px;
}

.node-domains__group-head {
  margin-bottom: 8px;
}

.node-domains__label {
  color: #374151;
  font-size: 13px;
  font-weight: 600;
}

.node-domains__empty {
  color: #9ca3af;
  font-size: 12px;
  margin: 0 0 8px;
}

.node-domains__row {
  align-items: center;
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
}

.node-domains__cf-row {
  margin-bottom: 12px;
}

.node-domains__input {
  flex: 1;
}

.node-domains__primary {
  flex-shrink: 0;
  margin-right: 0;
}

.node-domains__token {
  margin-top: 4px;
}

.node-domains__add {
  margin-top: 2px;
}
</style>
