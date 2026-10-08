<!-- 页面标题统一控制信息层级与操作区，移动端允许按钮换行，不重复产品品牌。 -->
<script setup lang="ts">
defineProps<{ title: string; description: string }>();
</script>
<template>
  <header class="page-header">
    <div class="page-header__copy">
      <h1>{{ title }}</h1>
      <p v-if="description">{{ description }}</p>
    </div>
    <div v-if="$slots.default" class="page-header__actions"><slot /></div>
  </header>
</template>
