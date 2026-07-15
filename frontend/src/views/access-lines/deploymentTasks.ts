// 本文件放中转节点页部署任务的纯判定逻辑,与 UI/副作用解耦便于单测。
// 一键安装的中转节点由后端在部署任务跑成功后才解析鉴权码登记入库,而页面轮询
// (useAccessLinesPage.refreshDeploymentTasks)只重拉任务列表更新进度卡片、不会自动拉节点,
// 导致任务显示成功后新节点仍不出现、用户被迫手动刷新。这里提供「本轮是否有任务新进入成功终态」
// 的判定,供页面据此在成功跳变时重拉读模型,让新节点/入口/运行态立即出现。
import type { DeploymentTask } from '@/services/api';

/// 判断本轮轮询相比上一轮,是否有部署任务「新」进入 succeeded 成功终态。
/// 按任务 id 比对前后状态:仅当「上一轮该任务非成功(或上一轮尚无该任务)、这一轮成功」才算新成功,
/// 从而只在成功跳变的那一次触发刷新,不会每轮重复拉取;失败/取消等终态不算(不登记节点、无需刷新)。
export function hasNewlySucceededTask(
  previous: DeploymentTask[],
  next: DeploymentTask[],
): boolean {
  const wasSucceeded = new Map(
    previous.map((task) => [task.id, task.status === 'succeeded'] as const),
  );
  // 上一轮无该任务时 get 返回 undefined,!undefined 为 true——首轮就直达 succeeded 也视为新成功。
  return next.some((task) => task.status === 'succeeded' && !wasSucceeded.get(task.id));
}
