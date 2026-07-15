<!--
  本组件承载入口管理的 CDN（Cloudflare）字段区块，仅在「CF 直连」连接方式下由父级渲染。
  选了 CF 直连即等于开启 CDN（cdnEnabled 由连接方式驱动），故不再提供手动「启用 CDN」开关，
  这里只填 CDN 服务商 / Hostname。组件只做展示与双向绑定，不直接调接口、不维护页面状态。
-->
<script setup lang="ts">
interface EntryCdnFormState {
  cdnProvider: string;
  cdnHostname: string;
}

// form 是父级 reactive 入口表单对象，直接就地改写其 CDN 字段（与其它入口字段一致）。
defineProps<{
  form: EntryCdnFormState;
}>();

const emit = defineEmits<{
  // 管理员手动改了 CF 域名，父级据此标脏不再自动覆盖。
  (event: 'cdn-hostname-input'): void;
}>();
</script>

<template>
  <el-col :span="8">
    <el-form-item label="CDN 服务商">
      <el-select v-model="form.cdnProvider" class="entry-cdn-field">
        <el-option label="Cloudflare" value="cloudflare" />
      </el-select>
    </el-form-item>
  </el-col>
  <el-col :span="8">
    <el-form-item label="CDN Hostname">
      <el-input
        v-model="form.cdnHostname"
        placeholder="cdn-entry.example.test"
        @input="emit('cdn-hostname-input')"
      />
      <small class="field-hint">默认取自所选节点的 CF 域名，可改。</small>
    </el-form-item>
  </el-col>
</template>

<style scoped>
.entry-cdn-field {
  width: 100%;
}

.field-hint {
  color: var(--el-text-color-secondary);
  display: block;
  line-height: 1.4;
  margin-top: 4px;
}
</style>
