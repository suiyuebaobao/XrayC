//! 本模块是「监控中心」宿主指标的共享纯函数层(阶段B)。
//! 只做无副作用的解析与一次 statvfs 系统调用,不持有跨调用状态、不起后台任务,
//! 供 access-agent 采集层组装 CPU/内存/磁盘上报字段,api/前端各自复用同一口径。
//! CPU 千分比由两次 `/proc/stat` 样本算差,内存由 `/proc/meminfo` 解析,
//! 磁盘由宿主挂载点 `libc::statvfs` 读取(总量/可用→已用)。
//! 容器内 `/proc/stat`、`/proc/meminfo` 默认即宿主,直接读不挂载;只有磁盘读 host 挂载点。
//! 不引重依赖(不用 sysinfo),只用已在依赖树的 libc,且仅一小段 unsafe 调 statvfs。
//! 任一项失败由调用方按软失败处理,本层只返回纯计算结果或 io::Result。
//! 数值约定:CPU 千分比 u32,内存/磁盘 used/total 为字节 u64。
//! 本文件前十行中文注释满足仓库源码头部约束。

use std::io;

/// `/proc/stat` 第一行解析出的 CPU 累计时间样本(各档位为内核 jiffies 累计值)。
/// busy = total - (idle + iowait),两次样本作差算占用率,与上游 `/proc/stat` 口径一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcStatSample {
    /// total = user+nice+system+idle+iowait+irq+softirq+steal,所有档位之和。
    pub total: u64,
    /// 空闲部分 = idle + iowait,占用率计算里要从 total 减掉的部分。
    pub idle: u64,
}

/// 解析 `/proc/stat` 文本首行 `cpu user nice system idle iowait irq softirq steal ...`,
/// 还原为累计样本。缺字段按 0 计,多余字段忽略;首行非 `cpu` 前缀视为格式异常返回 None。
pub fn parse_proc_stat(text: &str) -> Option<ProcStatSample> {
    let first_line = text.lines().next()?;
    let mut fields = first_line.split_whitespace();
    // 第一个 token 必须是聚合行标签 `cpu`(单核行是 cpu0/cpu1,这里只取整机聚合)。
    if fields.next()? != "cpu" {
        return None;
    }
    // 依次取 user nice system idle iowait irq softirq steal,缺位按 0,保证短行不 panic。
    let values: Vec<u64> = fields
        .take(8)
        .map(|v| v.parse::<u64>().unwrap_or(0))
        .collect();
    if values.is_empty() {
        return None;
    }
    let user = values.first().copied().unwrap_or(0);
    let nice = values.get(1).copied().unwrap_or(0);
    let system = values.get(2).copied().unwrap_or(0);
    let idle = values.get(3).copied().unwrap_or(0);
    let iowait = values.get(4).copied().unwrap_or(0);
    let irq = values.get(5).copied().unwrap_or(0);
    let softirq = values.get(6).copied().unwrap_or(0);
    let steal = values.get(7).copied().unwrap_or(0);
    let total = user + nice + system + idle + iowait + irq + softirq + steal;
    Some(ProcStatSample {
        total,
        idle: idle + iowait,
    })
}

/// 由两次 `/proc/stat` 样本算 CPU 占用千分比:
/// busy_delta = (total_cur-busy_idle_cur) - (total_prev-busy_idle_prev) 经 total/idle 差换算,
/// pct_milli = busy_delta * 100000 / total_delta。total_delta 为 0(两样本相同)时返回 0,
/// 并把结果钳制在 [0,100000] 防止时钟回绕/异常样本产生越界值。
pub fn cpu_pct_milli(prev: ProcStatSample, cur: ProcStatSample) -> u32 {
    let total_delta = cur.total.saturating_sub(prev.total);
    if total_delta == 0 {
        return 0;
    }
    let idle_delta = cur.idle.saturating_sub(prev.idle);
    let busy_delta = total_delta.saturating_sub(idle_delta);
    let pct_milli = busy_delta.saturating_mul(100_000) / total_delta;
    pct_milli.min(100_000) as u32
}

/// 解析 `/proc/meminfo` 文本,取 MemTotal、MemAvailable(单位 kB)换算为字节,
/// 返回 `(used_bytes, total_bytes)`。used = total - available(available 多于 total 时钳为 0)。
/// 缺 MemAvailable 时按 0 可用处理(used=total),缺 MemTotal 则返回 (0,0)。
pub fn parse_proc_meminfo(text: &str) -> (u64, u64) {
    let total_kb = read_meminfo_kb(text, "MemTotal:").unwrap_or(0);
    let available_kb = read_meminfo_kb(text, "MemAvailable:").unwrap_or(0);
    let total_bytes = total_kb.saturating_mul(1024);
    let available_bytes = available_kb.saturating_mul(1024);
    let used_bytes = total_bytes.saturating_sub(available_bytes);
    (used_bytes, total_bytes)
}

/// 从 `/proc/meminfo` 取某个键(如 `MemTotal:`)的 kB 数值;键不存在返回 None。
/// 行形如 `MemTotal:       8192000 kB`,取键后第一个数字 token 即可。
fn read_meminfo_kb(text: &str, key: &str) -> Option<u64> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            return rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    None
}

/// 读取宿主挂载点的磁盘用量,返回 `(used_bytes, total_bytes)`。
/// 用 `libc::statvfs` 系统调用:total = f_blocks*f_frsize,avail = f_bavail*f_frsize,
/// used = total - avail(用 f_bavail 即非特权可用块,与 `df` 对普通用户口径一致)。
/// 挂载点不存在/无权限时 statvfs 返回非 0,转成 io::Error 上抛由调用方软失败处理。
pub fn statvfs_usage(mount_point: &str) -> io::Result<(u64, u64)> {
    // statvfs 需要 C 字符串路径;路径含内部 NUL 时视为非法输入。
    let c_path = std::ffi::CString::new(mount_point)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "mount path contains NUL"))?;
    // SAFETY: 仅向 statvfs 传入合法 C 路径与一块零初始化的 statvfs 缓冲区,
    // 调用后按返回码判定成功,不读未初始化字段;这是读取磁盘用量的唯一 unsafe 边界。
    let stats = unsafe {
        let mut stats: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stats) != 0 {
            return Err(io::Error::last_os_error());
        }
        stats
    };
    // f_frsize 为基础块大小(字节);total 用全部块、avail 用非特权可用块。
    let frsize = stats.f_frsize as u64;
    let total = (stats.f_blocks as u64).saturating_mul(frsize);
    let avail = (stats.f_bavail as u64).saturating_mul(frsize);
    let used = total.saturating_sub(avail);
    Ok((used, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_pct_milli_from_two_proc_stat_samples() {
        // 两段固定 /proc/stat:
        //   prev: total=1000、idle+iowait=800 → busy=200。
        //   cur:  total=2000、idle+iowait=1100 → busy=900。
        // total_delta=1000,idle_delta=300,busy_delta=700 → 700*100000/1000 = 70000(即 70.000%)。
        let prev = parse_proc_stat("cpu 100 0 100 700 100 0 0 0\ncpu0 1 2 3 4 5 6 7 8\n")
            .expect("prev sample parses");
        let cur = parse_proc_stat("cpu 600 0 300 1000 100 0 0 0\ncpu0 1 2 3 4 5 6 7 8\n")
            .expect("cur sample parses");

        assert_eq!(cpu_pct_milli(prev, cur), 70_000);
    }

    #[test]
    fn test_cpu_pct_milli_zero_total_delta_is_zero() {
        // 两样本完全相同(无 tick 流逝):total_delta=0,不能除零,占用率读 0。
        let sample = parse_proc_stat("cpu 100 0 100 700 100 0 0 0").expect("sample parses");
        assert_eq!(cpu_pct_milli(sample, sample), 0);
    }

    #[test]
    fn test_parse_proc_stat_rejects_non_cpu_first_line() {
        // 首行不是聚合 cpu 行:视为格式异常返回 None,交调用方软失败。
        assert!(parse_proc_stat("intr 12345 0 0\ncpu 1 2 3 4").is_none());
    }

    #[test]
    fn test_parse_proc_meminfo_used_and_total() {
        // MemTotal 8192000 kB、MemAvailable 2048000 kB:
        // total=8192000*1024=8388608000,avail=2048000*1024=2097152000,
        // used=total-avail=6291456000。
        let text = "MemTotal:        8192000 kB\n\
                    MemFree:          512000 kB\n\
                    MemAvailable:    2048000 kB\n\
                    Buffers:          100000 kB\n";
        let (used, total) = parse_proc_meminfo(text);
        assert_eq!(total, 8_388_608_000);
        assert_eq!(used, 6_291_456_000);
    }

    #[test]
    fn test_parse_proc_meminfo_missing_available_uses_total_as_used() {
        // 缺 MemAvailable:按 0 可用处理,used=total,不 panic。
        let text = "MemTotal:        1024 kB\nMemFree:  100 kB\n";
        let (used, total) = parse_proc_meminfo(text);
        assert_eq!(total, 1024 * 1024);
        assert_eq!(used, 1024 * 1024);
    }

    #[test]
    fn test_statvfs_usage_on_root_is_real_and_non_panicking() {
        // 对真实 "/" 跑一次 statvfs:必须成功、不 panic,且 used<=total、total>0。
        let (used, total) = statvfs_usage("/").expect("statvfs on / succeeds");
        assert!(total > 0, "根分区总量应为正");
        assert!(used <= total, "已用不应超过总量");
    }

    #[test]
    fn test_statvfs_usage_missing_mount_returns_err() {
        // 不存在的挂载点:statvfs 失败转 io::Error 上抛,绝不 panic。
        assert!(statvfs_usage("/xrayc-nonexistent-mount-point-12345").is_err());
    }
}
