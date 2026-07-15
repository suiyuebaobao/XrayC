<!--
  本组件承载入口管理「监听」区块（监听地址 + 监听端口），从 AccessEntryDialog 拆出以守 550 行上限。
  CF（橙云）端口只放行 CF 支持的 HTTPS 端口（下拉选择）；非 CF 入口用数字输入框自由填。
  端口默认值由页面按「节点 + 连接方式」智能填未占用端口（父级驱动），本组件只展示与双向绑定，
  并在手填端口被占用 / CF 端口不合规时就地红字提示（portError 由父级即时校验后传入）。
-->
<script setup lang="ts">
import { computed } from 'vue';
import { CF_PORT_CANDIDATES } from '@/views/shared/nextFreePort';

// form 是父级 reactive 入口表单对象，直接就地改写监听地址/端口（与其它 Entry* 子组件同口径）。
defineProps<{
  form: { listenHost: string; listenPort: number };
  // 是否 CF（橙云）：CF 时端口限 CF_PORT_CANDIDATES，用下拉；否则数字输入框。
  isCf: boolean;
  // 即时端口校验文案：被占用 / CF 端口不合规时的红字提示，空串=无提示。
  portError: string;
}>();

const emit = defineEmits<{
  // 管理员手改监听地址：页面据此标脏不再自动覆盖。
  (event: 'listen-host-input'): void;
}>();

// CF 端口下拉项：只列 CF 支持的 HTTPS 端口（填别的到 CF 边缘连不上）。
const cfPortOptions = computed(() => CF_PORT_CANDIDATES.map((port) => ({ label: String(port), value: port })));
</script>

<template>
  <el-row :gutter="16">
    <el-col :span="16">
      <el-form-item label="监听地址">
        <el-input
          v-model="form.listenHost"
          placeholder="0.0.0.0 或节点公网地址"
          @input="emit('listen-host-input')"
        />
        <small class="field-hint">默认取自所选域名（未选时取节点直连地址），可改。</small>
      </el-form-item>
    </el-col>
    <el-col :span="8">
      <el-form-item label="监听端口">
        <!-- CF（橙云）走 CF 代理，端口只能从 CF 支持的 HTTPS 端口里选；非 CF 入口自由填。 -->
        <el-select v-if="isCf" v-model="form.listenPort" class="entry-field">
          <el-option
            v-for="option in cfPortOptions"
            :key="option.value"
            :label="option.label"
            :value="option.value"
          />
        </el-select>
        <el-input-number
          v-else
          v-model="form.listenPort"
          :min="1"
          :max="65535"
          class="entry-field"
        />
        <small v-if="portError" class="field-error">{{ portError }}</small>
        <small v-else-if="isCf" class="field-hint">
          橙云走 CF 代理，端口只能用 CF 支持的 HTTPS 端口（443/8443/2053/2083/2087/2096）。
        </small>
        <small v-else class="field-hint">已自动填未占用端口，可改；撞到已占用端口会提示。</small>
      </el-form-item>
    </el-col>
  </el-row>
</template>

<style scoped>
.entry-field {
  width: 100%;
}

.field-hint {
  color: var(--el-text-color-secondary);
  display: block;
  line-height: 1.4;
  margin-top: 4px;
}

/* 端口即时校验红字：被占用 / CF 端口不合规时提示，提交前就拦住，别等后端 400。 */
.field-error {
  color: var(--el-color-danger);
  display: block;
  line-height: 1.4;
  margin-top: 4px;
}
</style>
