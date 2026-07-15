//! 本模块负责运行时活动配置缓存。
//! 缓存保存最近一次成功应用的控制面配置，进程重启后可恢复内存态
//! 并继续回报已应用版本，避免短暂离线导致旧配置丢失。

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::warn;
use xrayc_xray_config::AccessConfig;

use crate::config::AgentSettings;

use super::utils::candidate_config_path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ActiveConfigCache {
    pub(super) node_id: String,
    pub(super) config_version: Option<String>,
    pub(super) config: Option<AccessConfig>,
}

pub(super) fn load_active_config_cache(
    settings: &AgentSettings,
) -> anyhow::Result<Option<ActiveConfigCache>> {
    let path = active_config_cache_path(settings);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {:?}", path)),
    };
    let cache = serde_json::from_str::<ActiveConfigCache>(&contents)
        .with_context(|| format!("parse {:?}", path))?;
    if cache.node_id != settings.node_id {
        warn!("ignored active config cache for different node");
        return Ok(None);
    }
    if cache
        .config
        .as_ref()
        .is_some_and(|config| config.node_id != settings.node_id)
    {
        warn!("ignored active config cache with mismatched config node");
        return Ok(None);
    }
    Ok(Some(cache))
}

pub(super) fn persist_active_config_cache(
    settings: &AgentSettings,
    config_version: Option<String>,
    config: Option<AccessConfig>,
) {
    let cache = ActiveConfigCache {
        node_id: settings.node_id.clone(),
        config_version,
        config,
    };
    if let Err(error) = write_active_config_cache(settings, &cache) {
        warn!(%error, "active config cache persist failed");
    }
}

fn write_active_config_cache(
    settings: &AgentSettings,
    cache: &ActiveConfigCache,
) -> anyhow::Result<()> {
    let path = active_config_cache_path(settings);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {:?}", parent))?;
    }

    let candidate = candidate_config_path(&path);
    let rendered = serde_json::to_vec_pretty(cache)?;
    fs::write(&candidate, rendered).with_context(|| format!("write {:?}", candidate))?;
    set_private_file_permissions(&candidate)?;
    fs::rename(&candidate, &path).or_else(|_| {
        fs::copy(&candidate, &path)?;
        fs::remove_file(&candidate)
    })?;
    set_private_file_permissions(&path)?;
    Ok(())
}

fn active_config_cache_path(settings: &AgentSettings) -> PathBuf {
    settings.agent_state_path.clone()
}

fn set_private_file_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
