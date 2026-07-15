//! 异地同步模块:rsync 增量把本地 `/backups/` 等目录推到异地机 + 按保留期清理异地旧备份。
//! 走 SSH 密钥免密(`-e "ssh -i <私钥> ..."`),日常同步不碰 SSH 密码;失败不 panic、只回脱敏
//! 消息(抹掉 host),绝不阻塞本地备份。异地保留清理只删文件名前缀 `xrayc-postgres-*`(双重约束
//! 防误删)。命令参数组装抽纯函数便于单测,不真连服务器即可校验精确性。
//! `sync_offsite` 由 run.rs 的 apply_full_offsite / apply_wal_offsite 按 full/wal 的 `destination`
//! (both/offsite)触发调用;异地不再是独立模式,故本模块只提供同步能力与存储位置决策纯函数。
//! 装公钥 provision 复用共享 crate `xrayc_backup::provision_offsite`(api 侧一次性 SSH)。
//! 本头部满足中文说明约束。

use std::path::Path;

use serde_json::Value;

/// 异地备份文件名前缀:保留清理只匹配它,双重约束(前缀 + mtime)防误删非备份文件。
const OFFSITE_FILE_PREFIX: &str = "xrayc-postgres-";
/// 单条命令输出脱敏后的截断上限。
const MAX_OUTPUT_CHARS: usize = 240;

/// 一次异地同步的结果快照:是否已同步、脱敏后的可展示消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsiteSyncReport {
    pub synced: bool,
    pub message: String,
}

/// 某备份方式(全量/WAL)的存储位置决策:由 `destination` 字段派生,决定「该不该推异地」
/// 与「异地成功后该不该删本机」。异地不再是独立模式,故这里是执行层的唯一异地触发依据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DestinationPlan {
    /// local:只留本机,不推异地。
    LocalOnly,
    /// both:推异地,但本机也保留(两处都留)。
    PushKeepLocal,
    /// offsite:推异地,推成功后删本机(仅异地);推失败保留本机(绝不无备份)。
    PushThenDeleteLocal,
}

/// 把 `destination` 字符串映射为存储位置决策:local/缺失/非法 → LocalOnly、both → PushKeepLocal、
/// offsite → PushThenDeleteLocal。非法值保守回退 LocalOnly(绝不因脏配置误删本机)。
pub(super) fn destination_plan(destination: &str) -> DestinationPlan {
    match destination {
        "both" => DestinationPlan::PushKeepLocal,
        "offsite" => DestinationPlan::PushThenDeleteLocal,
        _ => DestinationPlan::LocalOnly,
    }
}

impl DestinationPlan {
    /// 本方式是否需要推异地(both / offsite 需要,local 不需要)。
    pub(super) fn should_push(self) -> bool {
        matches!(self, Self::PushKeepLocal | Self::PushThenDeleteLocal)
    }
}

/// 异地推送后是否删本机:**仅** destination=offsite 且已确认推送成功(synced=true)才删。
/// 红线:both/local 一律保留本机;offsite 但同步失败(synced=false)也保留本机(绝不无备份)。
pub(super) fn should_delete_local_after_sync(plan: DestinationPlan, synced: bool) -> bool {
    matches!(plan, DestinationPlan::PushThenDeleteLocal) && synced
}

/// 组装 rsync 的 `-e` SSH 传输串(密钥免密)。
/// 对应配方:`ssh -i <keypath> -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -p <port>`。
fn rsync_ssh_option(key_path: &Path, port: u16) -> String {
    format!(
        "ssh -i {} -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -p {}",
        key_path.display(),
        port
    )
}

/// 组装单个源目录 rsync 增量推送的参数(不含二进制名)。
/// 对应配方:`rsync -az --partial -e "ssh -i..." <src>/ <user>@<host>:<remote_dir>/`。
/// 源与远端均带尾斜杠:rsync 语义为「同步目录内容」而非把整目录再套一层。
fn rsync_args(
    key_path: &Path,
    port: u16,
    source_dir: &Path,
    user: &str,
    host: &str,
    remote_dir: &str,
) -> Vec<String> {
    vec![
        "-az".to_string(),
        "--partial".to_string(),
        "-e".to_string(),
        rsync_ssh_option(key_path, port),
        format!(
            "{}/",
            source_dir.display().to_string().trim_end_matches('/')
        ),
        format!("{user}@{host}:{}/", remote_dir.trim_end_matches('/')),
    ]
}

/// 组装异地保留清理的远端 `find` 命令(只删备份前缀 + mtime 超期)。
/// 对应配方:`find <remote_dir> -maxdepth 1 -type f -name 'xrayc-postgres-*' -mtime +<N> -delete`。
/// 双重约束:前缀 `xrayc-postgres-*` + `-mtime +N`,绝不无差别删远端目录内其它文件。
fn retention_find_command(remote_dir: &str, retention_days: i64) -> String {
    format!(
        "find {} -maxdepth 1 -type f -name '{}*' -mtime +{} -delete",
        remote_dir,
        OFFSITE_FILE_PREFIX,
        retention_days.max(1)
    )
}

/// 组装异地保留清理用的 `ssh` 参数(密钥免密,不含二进制名)。
fn retention_ssh_args(
    key_path: &Path,
    port: u16,
    user: &str,
    host: &str,
    remote_dir: &str,
    retention_days: i64,
) -> Vec<String> {
    vec![
        "-i".to_string(),
        key_path.display().to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
        "-p".to_string(),
        port.to_string(),
        format!("{user}@{host}"),
        retention_find_command(remote_dir, retention_days),
    ]
}

/// 脱敏命令输出:抹掉 host,压平换行并截断;空结果回退通用提示。
fn sanitize_output(raw: &[u8], host: &str) -> String {
    let mut text = String::from_utf8_lossy(raw).replace(['\n', '\r'], " ");
    if !host.is_empty() {
        text = text.replace(host, "<host>");
    }
    let text = text.trim();
    if text.is_empty() {
        "rsync/ssh command failed".to_string()
    } else {
        text.chars().take(MAX_OUTPUT_CHARS).collect()
    }
}

/// 从 offsite JSON 取字符串字段(去空白),空则回退默认值。
fn offsite_str(offsite: &Value, key: &str, default: &str) -> String {
    offsite
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(default)
        .to_string()
}

/// 异地同步入口:公钥未装 / 缺 host / 缺私钥即跳过;否则 rsync 各源目录 → 远端,再按保留期清理。
/// 是否调用本函数由上层按 full/wal 的 `destination`(both/offsite)决定,offsite 段不再有 enabled;
/// 失败一律返回受控 `OffsiteSyncReport{synced:false, message: 脱敏}`,不 panic、不阻塞本地备份;
/// rsync 全部成功即 `synced=true`(数据已在异地),保留清理失败只降级记消息、不翻转 synced。
pub async fn sync_offsite(
    offsite: &Value,
    key_dir: &Path,
    source_dirs: &[&Path],
) -> anyhow::Result<OffsiteSyncReport> {
    // 公钥未装则无法免密同步,跳过(装公钥走 provision,另有入口)。
    if !offsite
        .get("pubkey_installed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return skip("异地机公钥未安装,跳过异地同步");
    }

    let host = offsite_str(offsite, "ssh_host", "");
    if host.is_empty() {
        return skip("未配置异地 SSH 主机,跳过");
    }
    let user = offsite_str(offsite, "ssh_user", "root");
    let remote_dir = offsite_str(offsite, "remote_dir", "/var/backups/xrayc");
    let port = offsite
        .get("ssh_port")
        .and_then(Value::as_i64)
        .unwrap_or(22)
        .clamp(1, 65535) as u16;
    let retention_days = offsite
        .get("retention_days")
        .and_then(Value::as_i64)
        .unwrap_or(30);

    let key_path = key_dir.join("id_ed25519");
    if !key_path.exists() {
        return skip("备份私钥不存在,无法免密同步,跳过");
    }

    // 逐个源目录 rsync 增量推送;不存在的源目录跳过(容错,不当失败)。
    let mut synced_any = false;
    for source_dir in source_dirs {
        if !source_dir.exists() {
            continue;
        }
        let args = rsync_args(&key_path, port, source_dir, &user, &host, &remote_dir);
        let output = match tokio::process::Command::new("rsync")
            .args(&args)
            .kill_on_drop(true)
            .output()
            .await
        {
            Ok(output) => output,
            Err(error) => {
                return Ok(OffsiteSyncReport {
                    synced: false,
                    message: format!(
                        "rsync 执行失败: {}",
                        sanitize_output(error.to_string().as_bytes(), &host)
                    ),
                });
            }
        };
        if !output.status.success() {
            return Ok(OffsiteSyncReport {
                synced: false,
                message: format!("rsync 同步失败: {}", sanitize_output(&output.stderr, &host)),
            });
        }
        synced_any = true;
    }

    if !synced_any {
        return skip("无可同步的源目录(均不存在),跳过");
    }

    // 数据已推到异地:再按保留期清理异地旧备份(仅前缀匹配);清理失败只降级,不翻转 synced。
    let retention_args =
        retention_ssh_args(&key_path, port, &user, &host, &remote_dir, retention_days);
    match tokio::process::Command::new("ssh")
        .args(&retention_args)
        .kill_on_drop(true)
        .output()
        .await
    {
        Ok(output) if output.status.success() => Ok(OffsiteSyncReport {
            synced: true,
            message: "异地同步完成,保留清理成功".to_string(),
        }),
        Ok(output) => Ok(OffsiteSyncReport {
            synced: true,
            message: format!(
                "异地同步完成,但异地保留清理失败: {}",
                sanitize_output(&output.stderr, &host)
            ),
        }),
        Err(error) => Ok(OffsiteSyncReport {
            synced: true,
            message: format!(
                "异地同步完成,但异地保留清理执行失败: {}",
                sanitize_output(error.to_string().as_bytes(), &host)
            ),
        }),
    }
}

/// 跳过分支的统一构造:synced=false + 说明消息。
fn skip(message: &str) -> anyhow::Result<OffsiteSyncReport> {
    Ok(OffsiteSyncReport {
        synced: false,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn rsync_ssh_option_matches_recipe() {
        // 对应配方:ssh -i <keypath> -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -p <port>
        assert_eq!(
            rsync_ssh_option(Path::new("/keys/id_ed25519"), 2222),
            "ssh -i /keys/id_ed25519 -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -p 2222"
        );
    }

    #[test]
    fn rsync_args_are_exact_with_trailing_slashes() {
        // 对应配方:rsync -az --partial -e "ssh -i ... -p <port>" <src>/ <user>@<host>:<remote_dir>/
        let args = rsync_args(
            Path::new("/keys/id_ed25519"),
            2222,
            Path::new("/backups"),
            "root",
            "203.0.113.9",
            "/var/backups/xrayc",
        );
        assert_eq!(args[0], "-az");
        assert_eq!(args[1], "--partial");
        assert_eq!(args[2], "-e");
        assert_eq!(
            args[3],
            "ssh -i /keys/id_ed25519 -o StrictHostKeyChecking=accept-new -o IdentitiesOnly=yes -p 2222"
        );
        // 源目录必须带尾斜杠(rsync 语义:同步目录内容而非整目录本身)。
        assert_eq!(args[4], "/backups/");
        // 远端目标也带尾斜杠。
        assert_eq!(args[5], "root@203.0.113.9:/var/backups/xrayc/");
    }

    #[test]
    fn rsync_args_support_multiple_sources_independently() {
        // 多源目录:各自组装成独立 rsync 调用,远端目标一致。
        let a = rsync_args(
            Path::new("/k"),
            22,
            Path::new("/backups"),
            "u",
            "198.51.100.7",
            "/remote",
        );
        let b = rsync_args(
            Path::new("/k"),
            22,
            Path::new("/wal-archive"),
            "u",
            "198.51.100.7",
            "/remote",
        );
        assert_eq!(a[4], "/backups/");
        assert_eq!(b[4], "/wal-archive/");
        assert_eq!(a[5], "u@198.51.100.7:/remote/");
        assert_eq!(b[5], "u@198.51.100.7:/remote/");
    }

    #[test]
    fn retention_find_only_matches_backup_prefix() {
        // 对应配方:find <remote_dir> -maxdepth 1 -type f -name 'xrayc-postgres-*' -mtime +<N> -delete
        let cmd = retention_find_command("/var/backups/xrayc", 30);
        assert!(
            cmd.contains("find /var/backups/xrayc"),
            "应对指定远端目录 find"
        );
        assert!(cmd.contains("-maxdepth 1"), "限定单层,不递归");
        assert!(cmd.contains("-type f"), "只删普通文件");
        assert!(
            cmd.contains("-name 'xrayc-postgres-*'"),
            "只匹配备份前缀,防误删非备份文件"
        );
        assert!(cmd.contains("-mtime +30"), "按保留天数按 mtime 判超期");
        assert!(cmd.contains("-delete"), "匹配到才删除");
        // 绝不能出现无差别通配删除。
        assert!(!cmd.contains("-name '*'"), "不得无差别匹配所有文件");
    }

    #[test]
    fn retention_ssh_args_are_exact() {
        let args = retention_ssh_args(
            Path::new("/keys/id_ed25519"),
            2222,
            "root",
            "203.0.113.9",
            "/var/backups/xrayc",
            7,
        );
        assert_eq!(args[0], "-i");
        assert_eq!(args[1], "/keys/id_ed25519");
        assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_string()));
        assert!(args.contains(&"IdentitiesOnly=yes".to_string()));
        let p_idx = args.iter().position(|a| a == "-p").expect("应含 -p");
        assert_eq!(args[p_idx + 1], "2222");
        assert_eq!(args[p_idx + 2], "root@203.0.113.9");
        assert_eq!(
            args.last().unwrap(),
            &retention_find_command("/var/backups/xrayc", 7)
        );
    }

    #[test]
    fn sanitize_output_masks_host_and_flattens() {
        let cleaned = sanitize_output(
            b"rsync: connection to 203.0.113.9\n timed out",
            "203.0.113.9",
        );
        assert!(!cleaned.contains("203.0.113.9"), "脱敏后不得含 host");
        assert!(!cleaned.contains('\n'), "应压平换行");
        assert!(cleaned.contains("timed out"), "应保留非敏感错误信息");
    }

    #[test]
    fn destination_plan_maps_three_values_and_defaults_local() {
        // local/缺失/非法 → LocalOnly;both → PushKeepLocal;offsite → PushThenDeleteLocal。
        assert_eq!(destination_plan("local"), DestinationPlan::LocalOnly);
        assert_eq!(destination_plan("both"), DestinationPlan::PushKeepLocal);
        assert_eq!(
            destination_plan("offsite"),
            DestinationPlan::PushThenDeleteLocal
        );
        // 非法/脏值保守回退 LocalOnly(绝不误推/误删)。
        assert_eq!(destination_plan("weird"), DestinationPlan::LocalOnly);
        assert_eq!(destination_plan(""), DestinationPlan::LocalOnly);
    }

    #[test]
    fn should_push_only_for_both_and_offsite() {
        assert!(!DestinationPlan::LocalOnly.should_push(), "local 不推异地");
        assert!(DestinationPlan::PushKeepLocal.should_push(), "both 推异地");
        assert!(
            DestinationPlan::PushThenDeleteLocal.should_push(),
            "offsite 推异地"
        );
    }

    #[test]
    fn delete_local_only_for_offsite_after_successful_sync() {
        // 红线:仅 destination=offsite 且已确认同步成功才删本机;其它组合一律保留。
        assert!(should_delete_local_after_sync(
            DestinationPlan::PushThenDeleteLocal,
            true
        ));
        // offsite 但同步失败 → 保留本机(绝不无备份)。
        assert!(!should_delete_local_after_sync(
            DestinationPlan::PushThenDeleteLocal,
            false
        ));
        // both/local 即便同步成功也保留本机。
        assert!(!should_delete_local_after_sync(
            DestinationPlan::PushKeepLocal,
            true
        ));
        assert!(!should_delete_local_after_sync(
            DestinationPlan::LocalOnly,
            true
        ));
    }

    #[tokio::test]
    async fn sync_offsite_skips_when_pubkey_not_installed() {
        let offsite =
            json!({ "enabled": true, "pubkey_installed": false, "ssh_host": "203.0.113.9" });
        let report = sync_offsite(&offsite, Path::new("/keys"), &[Path::new("/backups")])
            .await
            .expect("跳过不应 Err");
        assert!(!report.synced, "公钥未装应跳过");
        assert!(report.message.contains("公钥") || report.message.contains("跳过"));
    }
}
