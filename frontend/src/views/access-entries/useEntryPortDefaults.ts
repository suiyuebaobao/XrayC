// 本文件封装入口表单的「智能默认端口 + 即时端口校验」逻辑，从 AccessEntriesPage 拆出以守行数上限。
// 已用端口取自前端已加载读模型（入口列表 ∪ 该节点自建本机出口 endpoint），编辑态排除自身端口。
// applyDefaultPort 只在新建态生效，据当前连接方式挑首个未占用端口；编辑既有入口时保留其现有端口。
// 仅做派生与就地改写页面 reactive 表单，不请求接口、不改后端护栏（护栏仍为兜底）。
import { computed, type Ref } from 'vue';
import type {
  AccessEntrySummary,
  ExitEndpointSummary,
  ExitResourceSummary,
} from '@/services/api';
import { nextFreePort, portConflictMessage } from '@/views/shared/nextFreePort';
import { collectNodeUsedPorts } from '@/views/shared/nodeUsedPorts';

// 入口端口逻辑只依赖表单这几项，单列避免与整份 AccessEntryFormState 强耦合。
interface EntryPortForm {
  id: string;
  accessNodeId: string;
  listenPort: number;
  // 连接方式即端口挑选模式：ip/domain 走防封端口、cf 限 CF 端口。
  connectionMode: 'ip' | 'domain' | 'cf';
}

export function useEntryPortDefaults(deps: {
  entryForm: EntryPortForm;
  entryMode: Ref<'create' | 'edit'>;
  entries: Ref<AccessEntrySummary[]>;
  exitEndpoints: Ref<ExitEndpointSummary[]>;
  exitResources: Ref<ExitResourceSummary[]>;
}) {
  const { entryForm, entryMode, entries, exitEndpoints, exitResources } = deps;

  // 当前节点已用端口集合；编辑态排除自身入口，避免把自己的端口当成「已占用」误报。
  const currentNodeUsedPorts = computed(() =>
    collectNodeUsedPorts(
      entryForm.accessNodeId,
      entries.value,
      exitEndpoints.value,
      exitResources.value,
      entryMode.value === 'edit' ? entryForm.id : undefined,
    ),
  );

  // 即时校验文案：手填端口被占用 / CF 选了非 CF 端口时给红字，供提交前拦截，别等后端 400。
  const portError = computed(() =>
    portConflictMessage(entryForm.connectionMode, Number(entryForm.listenPort), currentNodeUsedPorts.value),
  );

  // 新建态据连接方式挑首个未占用端口填入；编辑态保留现有端口（不覆盖）。全占用（如 CF 六口用尽）时保持原值。
  function applyDefaultPort() {
    if (entryMode.value !== 'create') {
      return;
    }
    const port = nextFreePort(entryForm.connectionMode, currentNodeUsedPorts.value);
    if (port !== null) {
      entryForm.listenPort = port;
    }
  }

  return { currentNodeUsedPorts, portError, applyDefaultPort };
}
