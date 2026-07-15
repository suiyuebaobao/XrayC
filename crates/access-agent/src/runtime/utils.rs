//! 本模块提供运行时共享的轻量工具。
//! 这里集中放置主循环、配置应用和上报逻辑都会使用的时间、主机名
//! 与临时文件路径生成函数，避免子模块之间产生循环依赖。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn hostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned())
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn candidate_config_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("config.json");
    target.with_file_name(format!("{file_name}.pending"))
}
