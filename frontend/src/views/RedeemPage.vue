<!--
  本页面用于普通用户输入兑换码兑换套餐。
  它提交兑换码到后端并展示兑换结果。
  套餐、时长和流量以后台兑换码配置为准。
-->
<script setup lang="ts">
import { Tickets } from '@element-plus/icons-vue';
import { ElMessage, type FormInstance, type FormRules } from 'element-plus';
import { reactive, ref } from 'vue';
import PageHeader from '@/components/PageHeader.vue';
import { apiClient } from '@/services/api';

const formRef = ref<FormInstance>();
const loading = ref(false);

const form = reactive({
  code: '',
});

const rules: FormRules<typeof form> = {
  code: [{ required: true, message: '请输入兑换码', trigger: 'blur' }],
};

async function submit() {
  await formRef.value?.validate();
  loading.value = true;
  try {
    const result = await apiClient.redeem({ code: form.code.trim() });
    form.code = '';
    ElMessage.success(result.message || '兑换成功');
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : '兑换失败');
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <PageHeader title="兑换码" description="提交兑换码开通或续费订阅。" />

  <el-card shadow="never" class="redeem-card">
    <template #header>使用兑换码</template>
    <el-form ref="formRef" :model="form" :rules="rules" label-position="top" @keyup.enter="submit">
      <el-form-item label="兑换码" prop="code">
        <el-input v-model="form.code" :prefix-icon="Tickets" placeholder="请输入兑换码" />
      </el-form-item>
      <el-button type="primary" :loading="loading" @click="submit">提交兑换</el-button>
    </el-form>
  </el-card>
</template>

<style scoped>
.redeem-card {
  max-width: 620px;
}
</style>
