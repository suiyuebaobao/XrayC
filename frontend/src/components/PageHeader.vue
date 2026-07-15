<!--
  本组件用于后台和用户页面顶部标题区域。
  它只展示页面标题与说明，不包含业务请求和权限判断。
  统一使用该组件可以保持页面头部视觉一致。
-->
<script setup lang="ts">
defineProps<{
  title: string;
  description: string;
}>();
</script>

<template>
  <div class="page-header">
    <div>
      <p class="eyebrow">XrayC V2</p>
      <h1>{{ title }}</h1>
      <p>{{ description }}</p>
    </div>
    <slot />
  </div>
</template>
