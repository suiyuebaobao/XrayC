// 本文件单测部署任务纯判定 hasNewlySucceededTask:一键安装的中转节点由后端在任务成功后才登记,
// 页面轮询只更新任务卡片、不会自动拉节点,故需检测「有任务本轮新进入 succeeded」以触发读模型重拉。
// 只用最小 { id, status } 构造任务对象(函数仅读这两字段),覆盖跳变/幂等/失败/首轮直达成功等分支。
import { describe, expect, it } from 'vitest';

import type { DeploymentTask } from '@/services/api';
import { hasNewlySucceededTask } from '@/views/access-lines/deploymentTasks';

function task(id: string, status: DeploymentTask['status']): DeploymentTask {
  // 纯判定只读 id/status,其余字段与本测试无关,断言成 DeploymentTask 保持类型契约。
  return { id, status } as DeploymentTask;
}

describe('hasNewlySucceededTask', () => {
  it('任务从 running 变为 succeeded 时返回 true(应刷新节点列表)', () => {
    const before = [task('t1', 'running')];
    const after = [task('t1', 'succeeded')];
    expect(hasNewlySucceededTask(before, after)).toBe(true);
  });

  it('任务上一轮已 succeeded 时返回 false(不重复刷新)', () => {
    const before = [task('t1', 'succeeded')];
    const after = [task('t1', 'succeeded')];
    expect(hasNewlySucceededTask(before, after)).toBe(false);
  });

  it('任务仍在 running 时返回 false', () => {
    const before = [task('t1', 'waiting_for_server')];
    const after = [task('t1', 'running')];
    expect(hasNewlySucceededTask(before, after)).toBe(false);
  });

  it('任务终态为 failed 时返回 false(失败不登记节点,无需刷新)', () => {
    const before = [task('t1', 'running')];
    const after = [task('t1', 'failed')];
    expect(hasNewlySucceededTask(before, after)).toBe(false);
  });

  it('首轮就直达 succeeded(上一轮无该任务)时返回 true(错过 running 也要刷新)', () => {
    const before: DeploymentTask[] = [];
    const after = [task('t1', 'succeeded')];
    expect(hasNewlySucceededTask(before, after)).toBe(true);
  });

  it('多任务中只要有一个新成功即返回 true(装好一个即刷新,不必等全部终态)', () => {
    const before = [task('t1', 'running'), task('t2', 'running')];
    const after = [task('t1', 'succeeded'), task('t2', 'running')];
    expect(hasNewlySucceededTask(before, after)).toBe(true);
  });
});
