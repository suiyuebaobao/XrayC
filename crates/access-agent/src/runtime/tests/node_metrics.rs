//! 本模块测试监控中心宿主指标采集层(阶段B)。
//! 钉死口径:NodeMetricsState 跨心跳保存上次 CPU 采样,collect() 任一子项失败软失败返回 None,
//! 绝不 panic、绝不让心跳/apply bail。首次采样无上次基线时按 0% CPU 处理仍返回有效报告。
//! 测试只在真实容器/宿主上跑 /proc 与 statvfs,不 mock 系统接口,验证非 panic 与数值边界。
//! 磁盘挂载点用 settings.host_fs_root,测试构造器默认 "/",不依赖 /hostfs 挂载。
//! 不存在的挂载点回退 "/" 的行为由采集层保证,这里覆盖首采与二次采样两条路径。
//! 采集结果字段:CPU 千分比 0..=100000、内存/磁盘 used<=total。
//! 新增采集子项时同步在此补软失败用例。
//! test_settings 已含 host_fs_root,无需额外构造。
//! 本文件前十行中文注释满足仓库源码头部约束。

use tempfile::tempdir;

use super::super::node_metrics::NodeMetricsState;
use super::support::test_settings;

#[test]
fn test_node_metrics_first_sample_does_not_panic_and_reports() {
    // 首次采样:NodeMetricsState 无上次 CPU 基线,collect 不应 panic,
    // 应给出 CPU=0(无 delta)且内存/磁盘为真实非零的报告。
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    let mut state = NodeMetricsState::default();

    let report = state
        .collect(&settings)
        .expect("first collect returns a report on a real host");

    assert!(report.cpu_pct_milli <= 100_000, "CPU 千分比不应越界");
    assert!(report.mem_total_bytes > 0, "内存总量应为正");
    assert!(
        report.mem_used_bytes <= report.mem_total_bytes,
        "内存已用不超总量"
    );
    assert!(report.disk_total_bytes > 0, "磁盘总量应为正");
    assert!(
        report.disk_used_bytes <= report.disk_total_bytes,
        "磁盘已用不超总量"
    );
    assert!(report.collected_at_unix > 0, "采集时刻应为正 unix 秒");
}

#[test]
fn test_node_metrics_second_sample_keeps_cpu_in_range() {
    // 二次采样:已有上次 CPU 基线,collect 再次成功且 CPU 千分比仍在合法区间,不 panic。
    let dir = tempdir().expect("tempdir");
    let settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    let mut state = NodeMetricsState::default();

    let _first = state.collect(&settings).expect("first collect");
    let second = state.collect(&settings).expect("second collect");

    assert!(second.cpu_pct_milli <= 100_000);
    assert!(second.mem_used_bytes <= second.mem_total_bytes);
    assert!(second.disk_used_bytes <= second.disk_total_bytes);
}

#[test]
fn test_node_metrics_falls_back_to_root_when_host_fs_root_missing() {
    // host_fs_root 指向不存在的挂载点:采集层应回退 "/" 跑 statvfs,仍给出有效磁盘用量,
    // 绝不因挂载点缺失而软失败丢掉整份报告(磁盘子项有可用回退)。
    let dir = tempdir().expect("tempdir");
    let mut settings = test_settings("http://127.0.0.1:1".to_string(), dir.path());
    settings.host_fs_root = "/xrayc-nonexistent-mount-9af3".to_string();
    let mut state = NodeMetricsState::default();

    let report = state
        .collect(&settings)
        .expect("collect still succeeds via / fallback");

    assert!(report.disk_total_bytes > 0, "回退 / 后磁盘总量应为正");
    assert!(report.disk_used_bytes <= report.disk_total_bytes);
}
