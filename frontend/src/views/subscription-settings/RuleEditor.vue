<!--
  本组件负责订阅通用规则的可视化编辑。
  页面层只保存 string[]，组件把表单输入转换为 Clash/mihomo 规则。
  代理动作统一保存为 PROXY，订阅生成时再改写到真实代理分组。
  高级文本模式保留原始规则编辑能力，兼容批量粘贴。
  规则顺序在客户端有语义，组件提供上移和下移操作。
  未能结构化识别的规则仍然保留，并在表格中显示为高级规则。
  新增规则时只做前端基础校验，后端仍负责保存后的最终归一。
  组件不内置批量预设，避免管理员误以为按钮没有反馈。
  注释使用中文，便于后续维护订阅规则编辑器。
  本头部满足源码说明约束。
-->
<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { computed, reactive } from 'vue';

type RuleAction = 'DIRECT' | 'PROXY' | 'REJECT' | 'REJECT-DROP';
type RuleKind = 'DOMAIN' | 'DOMAIN-SUFFIX' | 'DOMAIN-KEYWORD' | 'IP-CIDR' | 'IP-CIDR6' | 'GEOIP' | 'GEOSITE' | 'MATCH';

type ParsedRule = {
  index: number;
  raw: string;
  kind: RuleKind | 'RAW';
  kindLabel: string;
  target: string;
  action: string;
  actionLabel: string;
  noResolve: boolean;
};

const props = withDefaults(defineProps<{
  modelValue: string[];
  title?: string;
  description?: string;
  proxyActionLabel?: string;
  allowMatch?: boolean;
}>(), {
  title: '通用规则',
  description: '通用规则会在分组专用规则之后匹配；PROXY 会在下载订阅时指向套餐默认出口分组。',
  proxyActionLabel: '代理',
  allowMatch: true,
});

const emit = defineEmits<{
  'update:modelValue': [value: string[]];
}>();

const kindOptions = computed<Array<{ label: string; value: RuleKind }>>(() => {
  const options: Array<{ label: string; value: RuleKind }> = [
    { label: '域名后缀', value: 'DOMAIN-SUFFIX' },
    { label: '完整域名', value: 'DOMAIN' },
    { label: '域名关键词', value: 'DOMAIN-KEYWORD' },
    { label: 'IPv4/CIDR', value: 'IP-CIDR' },
    { label: 'IPv6/CIDR', value: 'IP-CIDR6' },
    { label: 'GEOIP', value: 'GEOIP' },
    { label: 'GEOSITE', value: 'GEOSITE' },
  ];
  return props.allowMatch ? [...options, { label: '兜底 MATCH', value: 'MATCH' }] : options;
});

const actionOptions = computed<Array<{ label: string; value: RuleAction }>>(() => [
  { label: props.proxyActionLabel, value: 'PROXY' },
  { label: '直连', value: 'DIRECT' },
  { label: '拒绝', value: 'REJECT' },
  { label: '拒绝丢弃', value: 'REJECT-DROP' },
]);

const form = reactive({
  kind: 'DOMAIN-SUFFIX' as RuleKind,
  target: '',
  action: 'PROXY' as RuleAction,
  noResolve: false,
});

const rulesText = computed({
  get: () => props.modelValue.join('\n'),
  set: (value: string) => emitRules(parseLines(value)),
});

const parsedRules = computed(() => props.modelValue.map(parseRule));

function addRule() {
  const rule = buildRule();
  if (!rule) {
    return;
  }
  appendRules([rule]);
  form.target = '';
}

function removeRule(index: number) {
  emitRules(props.modelValue.filter((_, current) => current !== index));
}

function moveRule(index: number, direction: -1 | 1) {
  const target = index + direction;
  if (target < 0 || target >= props.modelValue.length) {
    return;
  }
  const next = [...props.modelValue];
  [next[index], next[target]] = [next[target], next[index]];
  emitRules(next);
}

function buildRule() {
  const target = form.target.trim();
  if (needsTarget(form.kind) && !target) {
    ElMessage.warning('请输入规则内容');
    return '';
  }
  if (form.kind === 'MATCH') {
    return `MATCH,${form.action}`;
  }
  const noResolve = form.noResolve && supportsNoResolve(form.kind) ? ',no-resolve' : '';
  return `${form.kind},${target},${form.action}${noResolve}`;
}

function appendRules(rules: string[]) {
  const exists = new Set(props.modelValue);
  const next = [...props.modelValue];
  for (const rule of rules) {
    if (!exists.has(rule)) {
      next.push(rule);
      exists.add(rule);
    }
  }
  emitRules(next);
}

function emitRules(rules: string[]) {
  emit('update:modelValue', rules.map((rule) => rule.trim()).filter(Boolean));
}

function parseLines(value: string) {
  return value.split('\n').map((line) => line.trim()).filter(Boolean);
}

function parseRule(raw: string, index: number): ParsedRule {
  const parts = raw.split(',').map((part) => part.trim()).filter(Boolean);
  const kind = parts[0]?.toUpperCase() as RuleKind;
  if (kind === 'MATCH' && parts[1]) {
    return parsed(index, raw, kind, '', parts[1], false);
  }
  if (isRuleKind(kind) && parts.length >= 3) {
    return parsed(index, raw, kind, parts[1], parts[2], parts.includes('no-resolve'));
  }
  return {
    index,
    raw,
    kind: 'RAW',
    kindLabel: '高级规则',
    target: raw,
    action: '-',
    actionLabel: '-',
    noResolve: false,
  };
}

function parsed(index: number, raw: string, kind: RuleKind, target: string, action: string, noResolve: boolean): ParsedRule {
  return {
    index,
    raw,
    kind,
    kindLabel: kindOptions.value.find((option) => option.value === kind)?.label ?? kind,
    target,
    action,
    actionLabel: actionOptions.value.find((option) => option.value === action)?.label ?? action,
    noResolve,
  };
}

function isRuleKind(value: string): value is RuleKind {
  return kindOptions.value.some((option) => option.value === value);
}

function needsTarget(kind: RuleKind) {
  return kind !== 'MATCH';
}

function supportsNoResolve(kind: RuleKind) {
  return kind === 'IP-CIDR' || kind === 'IP-CIDR6' || kind === 'GEOIP';
}
</script>

<template>
  <el-card shadow="never" class="settings-card">
    <template #header>{{ title }}</template>
    <el-alert
      :title="description"
      type="info"
      show-icon
      :closable="false"
      class="settings-alert"
    />

    <div class="rule-form">
      <div data-testid="rule-kind">
        <el-select v-model="form.kind" class="rule-type">
          <el-option v-for="option in kindOptions" :key="option.value" :label="option.label" :value="option.value" />
        </el-select>
      </div>
      <div v-if="needsTarget(form.kind)" class="rule-target" data-testid="rule-target">
        <el-input
          v-model="form.target"
          placeholder="例如 chatgpt.com 或 192.0.2.1/32"
        />
      </div>
      <div data-testid="rule-action">
        <el-select v-model="form.action" class="rule-action">
          <el-option v-for="option in actionOptions" :key="option.value" :label="option.label" :value="option.value" />
        </el-select>
      </div>
      <el-switch
        v-model="form.noResolve"
        :disabled="!supportsNoResolve(form.kind)"
        active-text="no-resolve"
        inactive-text="解析"
      />
      <el-button type="primary" @click="addRule">添加规则</el-button>
    </div>

    <el-table :data="parsedRules" size="small" border class="rules-table">
      <el-table-column label="#" type="index" width="52" />
      <el-table-column prop="kindLabel" label="类型" min-width="120" />
      <el-table-column prop="target" label="内容" min-width="180" show-overflow-tooltip />
      <el-table-column prop="actionLabel" label="动作" min-width="90" />
      <el-table-column label="原始规则" min-width="240" show-overflow-tooltip>
        <template #default="{ row }">
          <span class="rule-raw">{{ row.raw }}</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="188" fixed="right">
        <template #default="{ row, $index }">
          <el-button link :disabled="$index === 0" @click="moveRule($index, -1)">上移</el-button>
          <el-button link :disabled="$index === parsedRules.length - 1" @click="moveRule($index, 1)">下移</el-button>
          <el-button link type="danger" @click="removeRule(row.index)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>

    <el-collapse class="advanced-rules">
      <el-collapse-item title="高级文本模式" name="raw">
        <el-input
          v-model="rulesText"
          type="textarea"
          :rows="8"
          placeholder="每行一条规则，例如 DOMAIN-SUFFIX,example.com,PROXY"
        />
      </el-collapse-item>
    </el-collapse>
  </el-card>
</template>

<style scoped>
.rule-form {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 14px;
}

.rule-type {
  width: 150px;
}

.rule-action {
  width: 110px;
}

.rules-table,
.advanced-rules {
  margin-top: 14px;
}

.rule-raw {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}
</style>
