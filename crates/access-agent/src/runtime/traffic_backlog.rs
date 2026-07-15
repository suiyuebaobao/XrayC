//! 本模块负责未成功上报流量的本地积压。
//! 当控制面暂时不可用时，运行时会把待上报快照落盘，下次上报前与
//! 当前快照合并，避免节点短时断连造成计费数据丢失。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use tracing::warn;

use crate::client::TrafficSnapshot;

#[derive(Debug, Default)]
pub(super) struct TrafficBacklog {
    path: PathBuf,
    snapshots: Vec<TrafficSnapshot>,
}

impl TrafficBacklog {
    pub(super) fn load(path: &Path) -> Self {
        let snapshots = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<TrafficSnapshot>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            snapshots,
        }
    }

    pub(super) fn merge_with_current(&self, current: Vec<TrafficSnapshot>) -> Vec<TrafficSnapshot> {
        let mut merged = self.snapshots.clone();
        merged.extend(current);
        merged
    }

    pub(super) fn save(&mut self, snapshots: &[TrafficSnapshot]) -> anyhow::Result<()> {
        self.snapshots = snapshots
            .iter()
            .rev()
            .take(2_000)
            .cloned()
            .collect::<Vec<_>>();
        self.snapshots.reverse();
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).context("create traffic backlog dir")?;
        }
        let bytes =
            serde_json::to_vec_pretty(&self.snapshots).context("serialize traffic backlog")?;
        fs::write(&self.path, bytes).context("write traffic backlog")?;
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.snapshots.clear();
        if self.path.exists() {
            if let Err(error) = fs::remove_file(&self.path) {
                warn!(%error, "remove traffic backlog failed");
            }
        }
    }
}
