//! 本模块是监控中心宿主指标采集层(阶段B),组装心跳上报的 node_metrics。
//! NodeMetricsState 跨心跳保存上次 CPU 采样,collect 用两次 /proc/stat 样本算占用千分比。
//! 内存读 /proc/meminfo、磁盘对 host 挂载点(settings.host_fs_root,缺省 /hostfs)跑 statvfs。
//! 容器内 /proc/stat、/proc/meminfo 默认即宿主直接读不挂载;只有磁盘读挂进来的 host 根。
//! 任一子项失败一律软失败(warn+跳过,返回 None),绝不 panic、绝不让心跳/apply bail(仿 kernel.rs)。
//! 纯计算复用 xrayc-core::host_metrics,本层只做读文件/系统调用与软失败包装。
//! host_fs_root 指向的挂载点不存在时回退 "/",保证磁盘子项仍可读到宿主或容器根。
//! 首次采样无上次基线时按 0% CPU 处理,仍返回内存/磁盘真实值的有效报告。
//! 采集时刻取本机 unix 秒,字段口径见 client::NodeMetricsReport。
//! 本文件前十行中文注释满足仓库源码头部约束。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::warn;
use xrayc_core::host_metrics::{
    cpu_pct_milli, parse_proc_meminfo, parse_proc_stat, statvfs_usage, ProcStatSample,
};

use crate::client::NodeMetricsReport;
use crate::config::AgentSettings;

/// 跨心跳保存上次 CPU 采样的采集状态(在主循环 loop 外声明,逐心跳复用)。
/// prev_cpu 为 None 即首次采样:本轮 CPU 按 0% 处理,只把当前样本存下供下轮算差。
#[derive(Debug, Default)]
pub(super) struct NodeMetricsState {
    prev_cpu: Option<ProcStatSample>,
}

impl NodeMetricsState {
    /// 采集一次宿主 CPU/内存/磁盘用量;任一子项失败软失败返回 None,绝不 panic/bail。
    ///
    /// CPU:读 /proc/stat 解析当前样本,与上次样本算占用千分比;无上次样本(首采)按 0% 处理。
    /// 无论本轮成败,只要当前样本解析成功就更新 prev_cpu,保证下一轮能算差。
    /// 内存:读 /proc/meminfo 解析 used/total。磁盘:对 host 挂载点跑 statvfs。
    pub(super) fn collect(&mut self, settings: &AgentSettings) -> Option<NodeMetricsReport> {
        // —— CPU:解析当前 /proc/stat 样本,失败软失败 —— //
        let stat_text = match std::fs::read_to_string("/proc/stat") {
            Ok(text) => text,
            Err(error) => {
                warn!(%error, "读取 /proc/stat 失败,跳过本轮节点指标采集");
                return None;
            }
        };
        let Some(cur_cpu) = parse_proc_stat(&stat_text) else {
            warn!("解析 /proc/stat 失败,跳过本轮节点指标采集");
            return None;
        };
        // 首采无上次基线则 CPU 读 0(无 delta);有基线则算占用千分比。无论如何都更新基线。
        let cpu_pct = self
            .prev_cpu
            .map(|prev| cpu_pct_milli(prev, cur_cpu))
            .unwrap_or(0);
        self.prev_cpu = Some(cur_cpu);

        // —— 内存:解析 /proc/meminfo,失败软失败 —— //
        let meminfo_text = match std::fs::read_to_string("/proc/meminfo") {
            Ok(text) => text,
            Err(error) => {
                warn!(%error, "读取 /proc/meminfo 失败,跳过本轮节点指标采集");
                return None;
            }
        };
        let (mem_used_bytes, mem_total_bytes) = parse_proc_meminfo(&meminfo_text);
        if mem_total_bytes == 0 {
            warn!("解析 /proc/meminfo 得到内存总量 0,跳过本轮节点指标采集");
            return None;
        }

        // —— 磁盘:对 host 挂载点跑 statvfs,挂载点不存在回退 "/" —— //
        // collect_disk_usage 内部已做软失败 warn;失败返回 None 即整轮跳过,绝不 bail。
        let (disk_used_bytes, disk_total_bytes) = self.collect_disk_usage(settings)?;

        Some(NodeMetricsReport {
            cpu_pct_milli: cpu_pct,
            mem_used_bytes,
            mem_total_bytes,
            disk_used_bytes,
            disk_total_bytes,
            collected_at_unix: unix_now_secs(),
        })
    }

    /// 读 host 磁盘用量:优先 settings.host_fs_root(默认 /hostfs),
    /// 该路径不存在时回退 "/";statvfs 失败软失败返回 None。
    fn collect_disk_usage(&self, settings: &AgentSettings) -> Option<(u64, u64)> {
        let configured = settings.host_fs_root.trim();
        // 挂载点存在就用它(部署把宿主 / 只读挂在这);不存在则回退容器/宿主根 "/"。
        let mount_point = if !configured.is_empty() && Path::new(configured).exists() {
            configured
        } else {
            "/"
        };
        match statvfs_usage(mount_point) {
            Ok((used, total)) if total > 0 => Some((used, total)),
            Ok(_) => {
                warn!("statvfs 读到磁盘总量 0,跳过本轮节点指标采集");
                None
            }
            Err(error) => {
                warn!(%error, "statvfs 读取磁盘用量失败,跳过本轮节点指标采集");
                None
            }
        }
    }
}

/// 取本机当前 unix 秒作采集时刻;系统时间早于 UNIX_EPOCH 的极端情形按 0 兜底。
fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
