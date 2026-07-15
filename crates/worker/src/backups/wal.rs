//! WAL / PITR 备份侧:plain WAL 归档 + `pg_basebackup` base 备份(面向 PITR 恢复)。
//! 本模块只落地「备份产出侧」:worker 跑 `pg_basebackup` 产 base backup(走网络复制协议,
//! 连接参数走 env、口令只写 0600 临时 pgpass 用后即删,绝不进 argv/日志)、按保留期清理旧 base、
//! 保守清理早于「最老保留 base」的 WAL 归档段(靠 backup_label 起始 WAL 锚定,解析不到就不删)、
//! 把归档卷 + base 仓库卷一并 rsync 推异地(复用 offsite.sync_offsite,不重复造 rsync 轮子)。
//! archive_command 由 postgres 容器只写本地卷(永不等网络、防爆盘),worker 只读 + 清理。
//! 恢复演练(restore base + recovery_target_time 重放)属 Task13 隔离库,本 task 不涉及。
//! 备份侧的 run_base_backup/prune_wal_archive/sync_wal_offsite 已由编排层 `run_all_if_due`
//! (经 run.rs 的 run_wal)在 WAL 模式触发时调用。本头部满足中文说明约束。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::process::Command;

use super::dump::{write_pgpass_file, PgDumpConnection, PGPASS_REPLICATION_DATABASE};
use super::offsite::{sync_offsite, OffsiteSyncReport};

/// base backup 目录名前缀:保留清理/锚定只匹配它,双重约束(前缀 + mtime)防误删非 base 目录。
const BASE_PREFIX: &str = "base-";
/// pg_basebackup 二进制名(worker 镜像 postgres16 客户端已装,走 PATH)。
const PG_BASEBACKUP_BIN: &str = "pg_basebackup";
/// pg_archivecleanup 二进制名(worker 镜像已装,走 PATH)。
const PG_ARCHIVECLEANUP_BIN: &str = "pg_archivecleanup";
/// WAL 归档卷默认路径(postgres archive_command 写入、worker 只读 + 清理),compose 注入同名 env。
const DEFAULT_WAL_ARCHIVE_DIR: &str = "/wal-archive";
/// base 备份仓库卷默认路径(worker 写),compose 注入同名 env。
const DEFAULT_WAL_REPO_DIR: &str = "/wal-repo";

/// 一次 base 备份的结果快照:本次 base 目录名 + 顺带清理掉的旧 base 数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalBackupReport {
    pub base_dir_name: String,
    pub pruned_bases: u64,
}

/// 组装 pg_basebackup 参数(抽纯函数便于单测)。
/// 对应配方:`-D <out_dir> -X stream -c fast --no-password`;
/// **host/port/db/user 一律走 env(PGHOST/PGPORT/PGDATABASE/PGUSER)、口令走 PGPASSFILE**,
/// 绝不进 argv 明文——故本函数返回的参数里不含任何连接/口令要素(单测断言)。
/// `conn` 入参保留是为与 run_base_backup 组装口径对齐(env 侧同源解析),此处刻意不落进 argv。
pub(super) fn pg_basebackup_args(_conn: &PgDumpConnection, out_dir: &Path) -> Vec<String> {
    vec![
        "-D".to_string(),
        out_dir.to_string_lossy().into_owned(),
        "-X".to_string(),
        "stream".to_string(),
        "-c".to_string(),
        "fast".to_string(),
        "--no-password".to_string(),
    ]
}

/// 生成 base 备份目录名:`base-<YYYYmmddTHHMMSSZ>`(UTC 时间戳,天然可字典序排序)。
pub(super) fn base_backup_dir_name(now: DateTime<Utc>) -> String {
    format!("{}{}", BASE_PREFIX, now.format("%Y%m%dT%H%M%SZ"))
}

/// WAL 归档卷路径(读 compose 注入的 XRAYC_WAL_ARCHIVE_DIR,缺省回退默认)。
fn wal_archive_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("XRAYC_WAL_ARCHIVE_DIR")
            .unwrap_or_else(|_| DEFAULT_WAL_ARCHIVE_DIR.to_string()),
    )
}

/// base 备份仓库卷路径(读 compose 注入的 XRAYC_WAL_REPO_DIR,缺省回退默认)。
fn wal_repo_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("XRAYC_WAL_REPO_DIR").unwrap_or_else(|_| DEFAULT_WAL_REPO_DIR.to_string()),
    )
}

/// 跑一次 base 备份:pg_basebackup 到 `repo_dir/base-<ts>`,成功后按保留期清理旧 base。
/// 连接走 env(同 pg_dump)、口令写 0600 临时 pgpass(复用 dump 的 write_pgpass_file)用后即删;
/// 失败清掉半成品 base 目录不留坏档;错误里带 stderr 供编排层(Task11)脱敏后落状态。
pub async fn run_base_backup(
    conn: &PgDumpConnection,
    repo_dir: &Path,
    retention_full: i64,
) -> anyhow::Result<WalBackupReport> {
    fs::create_dir_all(repo_dir).with_context(|| format!("create wal repo dir {:?}", repo_dir))?;
    let base_name = base_backup_dir_name(Utc::now());
    let out_dir = repo_dir.join(&base_name);

    // 口令只落 0600 临时 pgpass(写进 repo_dir),pg_basebackup 经 PGPASSFILE 读取,不进 argv/env 明文。
    // pg_basebackup 走复制协议:pgpass 的 database 段必须是 `replication`(而非实际库名),
    // 否则 libpq 匹配不上该行、报"no password supplied"(真机踩到的根因)。PGDATABASE 仍用实际库名、
    // 不影响这条 pgpass 的 database 段匹配(匹配只看那一行的 database 段)。
    let pgpass_path = write_pgpass_file(repo_dir, conn, PGPASS_REPLICATION_DATABASE)?;
    let args = pg_basebackup_args(conn, &out_dir);
    let mut command = Command::new(PG_BASEBACKUP_BIN);
    command
        .kill_on_drop(true)
        .env_remove("DATABASE_URL")
        .env("PGHOST", &conn.host)
        .env("PGPORT", conn.port.to_string())
        .env("PGDATABASE", &conn.database)
        .env("PGUSER", &conn.username)
        .args(&args);
    if let Some(sslmode) = &conn.sslmode {
        command.env("PGSSLMODE", sslmode);
    }
    if let Some(pgpass_path) = &pgpass_path {
        command.env("PGPASSFILE", pgpass_path);
    }

    let run_result = command.output().await;
    // 无论成败都先删临时 pgpass,口令不留盘。
    if let Some(pgpass_path) = &pgpass_path {
        let _ = fs::remove_file(pgpass_path);
    }
    let output = run_result.with_context(|| format!("run {PG_BASEBACKUP_BIN}"))?;
    if !output.status.success() {
        // 失败清掉半成品 base 目录,防止残档被后续误当有效 base 锚定。
        let _ = fs::remove_dir_all(&out_dir);
        return Err(anyhow!(
            "pg_basebackup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // 成功后清理旧 base:只碰 base- 前缀目录、按 mtime 保最新 retention_full 个(双重约束防误删)。
    let pruned_bases = prune_old_base_backups(repo_dir, retention_full)?;
    Ok(WalBackupReport {
        base_dir_name: base_name,
        pruned_bases,
    })
}

/// 选出该被清理的旧 base 目录名(纯函数便于单测)。
/// 只考虑 `base-` 前缀项(其它一律不碰),按 mtime 降序(新在前)保最新 `retention_full` 个、
/// 其余返回待删;`retention_full` 至少保 1 个(防配 0 把全部 base 删光)。mtime 相同以名字降序兜底稳定。
fn select_bases_to_prune(entries: &[(String, SystemTime)], retention_full: i64) -> Vec<String> {
    let keep = retention_full.max(1) as usize;
    let mut bases: Vec<&(String, SystemTime)> = entries
        .iter()
        .filter(|(name, _)| name.starts_with(BASE_PREFIX))
        .collect();
    // 按 (mtime, name) 降序:最新的排前面,保前 keep 个。
    bases.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    bases
        .into_iter()
        .skip(keep)
        .map(|(name, _)| name.clone())
        .collect()
}

/// 扫描 repo_dir 里的 base- 目录并按保留期清理,返回实际删除的 base 数。
fn prune_old_base_backups(repo_dir: &Path, retention_full: i64) -> anyhow::Result<u64> {
    let entries = list_base_dirs(repo_dir)?;
    let to_prune = select_bases_to_prune(&entries, retention_full);
    let mut removed = 0;
    for name in to_prune {
        // 双重约束再核一次前缀,绝不误删非 base 目录。
        if !name.starts_with(BASE_PREFIX) {
            continue;
        }
        fs::remove_dir_all(repo_dir.join(&name))
            .with_context(|| format!("remove old base {name}"))?;
        removed += 1;
    }
    Ok(removed)
}

/// 列出 repo_dir 下所有 `base-` 前缀的目录及其 mtime(其它项过滤掉)。
fn list_base_dirs(repo_dir: &Path) -> anyhow::Result<Vec<(String, SystemTime)>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(repo_dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(BASE_PREFIX) {
            continue;
        }
        let mtime = meta.modified().unwrap_or_else(|_| SystemTime::now());
        entries.push((name, mtime));
    }
    Ok(entries)
}

/// 找 repo_dir 里最老的保留 base 目录(按 (mtime,name) 取最小);读不到/无 base 返回 None。
/// 容错:repo 目录不存在也返回 None(交由上层保守不删),不 bail。
fn oldest_base_dir(repo_dir: &Path) -> Option<PathBuf> {
    let read = fs::read_dir(repo_dir).ok()?;
    let mut best: Option<(SystemTime, String)> = None;
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(BASE_PREFIX) {
            continue;
        }
        let mtime = meta.modified().unwrap_or_else(|_| SystemTime::now());
        let cand = (mtime, name);
        if best.as_ref().is_none_or(|b| &cand < b) {
            best = Some(cand);
        }
    }
    best.map(|(_, name)| repo_dir.join(name))
}

/// 从 backup_label 文本解析「起始 WAL 段文件名」(纯函数便于单测)。
/// backup_label 形如:`START WAL LOCATION: 0/2000028 (file 000000010000000000000002)`,
/// pg_archivecleanup 要的正是括号里那 24 位十六进制 WAL 段名。解析不到一律 None(上层据此保守不删)。
fn parse_backup_label_start_wal(label_text: &str) -> Option<String> {
    for line in label_text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("START WAL LOCATION:") else {
            continue;
        };
        // 取 `(file <名字>)` 括号里的 WAL 段名。
        let start = rest.find("(file ")? + "(file ".len();
        let tail = &rest[start..];
        let end = tail.find(')')?;
        let wal = tail[..end].trim().to_string();
        if wal.is_empty() {
            return None;
        }
        return Some(wal);
    }
    None
}

/// 保守清理 WAL 归档:**绝不删任一被保留 base 仍需要的 WAL**。
/// 以「最老保留 base 的 backup_label 起始 WAL」为界,跑 `pg_archivecleanup <archive_dir> <起始WAL>`
/// 删早于该段的归档;无 base / 读不到 backup_label / 解析不到起始 WAL 一律不删(宁可留多)。
/// 返回本次清理掉的归档文件数(以清理前后归档目录文件数之差估算)。
pub async fn prune_wal_archive(archive_dir: &Path, repo_dir: &Path) -> anyhow::Result<u64> {
    // 找最老保留 base 作锚点:没有 base 就没有可安全删除的下界,保守不删。
    let Some(oldest_base) = oldest_base_dir(repo_dir) else {
        return Ok(0);
    };
    let label_text = match fs::read_to_string(oldest_base.join("backup_label")) {
        Ok(text) => text,
        Err(_) => return Ok(0), // 读不到 backup_label:保守不删。
    };
    let Some(start_wal) = parse_backup_label_start_wal(&label_text) else {
        return Ok(0); // 解析不到起始 WAL:保守不删。
    };

    let before = count_files(archive_dir);
    let output = Command::new(PG_ARCHIVECLEANUP_BIN)
        .arg(archive_dir)
        .arg(&start_wal)
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("run {PG_ARCHIVECLEANUP_BIN}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "pg_archivecleanup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let after = count_files(archive_dir);
    Ok(before.saturating_sub(after))
}

/// 统计目录下普通文件数(容错:目录不存在/读失败按 0);用于估算 pg_archivecleanup 删了多少段。
fn count_files(dir: &Path) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    read.flatten()
        .filter(|entry| entry.metadata().map(|m| m.is_file()).unwrap_or(false))
        .count() as u64
}

/// 递归求目录下所有普通文件的字节总和(供 WAL 历史记录取「本次 base 目录总字节」)。
/// 不引第三方 crate:手写递归遍历,遇子目录下潜、遇普通文件累加其 len;容错——读失败/
/// 目录不存在/取不到 metadata 一律按 0 跳过,绝不 bail、绝不 panic(历史记录不该拖垮备份)。
pub(super) fn dir_total_bytes(dir: &Path) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_total_bytes(&entry.path());
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

/// 把 WAL 归档卷 + base 仓库卷一并 rsync 推异地(薄封装,复用 offsite.sync_offsite 不重复造轮子)。
/// 两目录取自 compose 注入的 XRAYC_WAL_ARCHIVE_DIR / XRAYC_WAL_REPO_DIR;失败不 panic、按 offsite 语义回受控报告。
pub async fn sync_wal_offsite(
    offsite: &Value,
    key_dir: &Path,
) -> anyhow::Result<OffsiteSyncReport> {
    let archive_dir = wal_archive_dir();
    let repo_dir = wal_repo_dir();
    sync_offsite(
        offsite,
        key_dir,
        &[archive_dir.as_path(), repo_dir.as_path()],
    )
    .await
}

/// 仅异地(destination=offsite)且异地推送成功后清本机 WAL(best-effort):
/// 删 `repo_dir` 下所有 `base-` 前缀目录(双重约束防误删非 base 目录)+ `archive_dir` 下所有普通文件。
/// 单个删除失败只跳过、不 bail、不 panic;返回 (清掉的 base 数, 清掉的归档文件数)。
/// **红线**:只在 rsync 成功后调用(调用方 `should_delete_local_after_sync` 已守住),绝不无备份删本机。
pub async fn clear_local_wal(repo_dir: &Path, archive_dir: &Path) -> (u64, u64) {
    let mut bases = 0;
    if let Ok(read) = fs::read_dir(repo_dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // 只碰 base- 前缀目录,绝不误删仓库卷里的其它内容。
            if !name.starts_with(BASE_PREFIX) {
                continue;
            }
            if entry.metadata().map(|meta| meta.is_dir()).unwrap_or(false)
                && fs::remove_dir_all(entry.path()).is_ok()
            {
                bases += 1;
            }
        }
    }
    let mut archives = 0;
    if let Ok(read) = fs::read_dir(archive_dir) {
        for entry in read.flatten() {
            if entry.metadata().map(|meta| meta.is_file()).unwrap_or(false)
                && fs::remove_file(entry.path()).is_ok()
            {
                archives += 1;
            }
        }
    }
    (bases, archives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ts(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn sample_conn() -> PgDumpConnection {
        PgDumpConnection {
            host: "postgres".to_string(),
            port: 15432,
            database: "xraycprod".to_string(),
            username: "xraycuser".to_string(),
            password: Some("s3cr3t-pass".to_string()),
            sslmode: Some("require".to_string()),
        }
    }

    #[test]
    fn pg_basebackup_args_are_exact_and_hide_connection() {
        // 参数精确匹配配方,且 host/口令/用户名/库名绝不进 argv(全走 env/PGPASSFILE)。
        let conn = sample_conn();
        let args = pg_basebackup_args(&conn, Path::new("/wal-repo/base-20260701T080000Z"));
        assert_eq!(
            args,
            vec![
                "-D".to_string(),
                "/wal-repo/base-20260701T080000Z".to_string(),
                "-X".to_string(),
                "stream".to_string(),
                "-c".to_string(),
                "fast".to_string(),
                "--no-password".to_string(),
            ]
        );
        for arg in &args {
            assert!(!arg.contains("postgres"), "host 不得进 argv");
            assert!(!arg.contains("s3cr3t"), "口令绝不得进 argv");
            assert!(!arg.contains("xraycuser"), "用户名不得进 argv");
            assert!(!arg.contains("xraycprod"), "库名不得进 argv");
            assert!(!arg.contains("15432"), "端口不得进 argv");
        }
    }

    #[test]
    fn base_backup_dir_name_uses_stable_prefix_and_utc_timestamp() {
        let now = DateTime::parse_from_rfc3339("2026-07-01T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(base_backup_dir_name(now), "base-20260701T080000Z");
    }

    #[test]
    fn select_bases_to_prune_keeps_newest_n_and_deletes_older() {
        // 三个 base、保 2 个:删掉 mtime 最老的那个,保最新两个。
        let entries = vec![
            ("base-20260701T010000Z".to_string(), ts(100)),
            ("base-20260701T020000Z".to_string(), ts(200)),
            ("base-20260701T030000Z".to_string(), ts(300)),
        ];
        let pruned = select_bases_to_prune(&entries, 2);
        assert_eq!(pruned, vec!["base-20260701T010000Z".to_string()]);
    }

    #[test]
    fn select_bases_to_prune_only_touches_base_prefix() {
        // 混入非 base- 项(误删红线):无论保留数多小,非 base- 前缀绝不进待删列表。
        let entries = vec![
            ("base-a".to_string(), ts(100)),
            ("base-b".to_string(), ts(200)),
            ("xrayc-postgres-20260701T010000Z.dump".to_string(), ts(1)),
            ("random-dir".to_string(), ts(2)),
            (".hidden".to_string(), ts(3)),
        ];
        let pruned = select_bases_to_prune(&entries, 1);
        // 只保最新 1 个 base(base-b),删更老的 base-a;非 base- 项一个都不能出现。
        assert_eq!(pruned, vec!["base-a".to_string()]);
        for name in &pruned {
            assert!(name.starts_with(BASE_PREFIX));
        }
    }

    #[test]
    fn select_bases_to_prune_retention_floors_at_one() {
        // retention_full=0 也至少保 1 个,绝不把全部 base 删光。
        let entries = vec![
            ("base-old".to_string(), ts(100)),
            ("base-new".to_string(), ts(200)),
        ];
        let pruned = select_bases_to_prune(&entries, 0);
        assert_eq!(pruned, vec!["base-old".to_string()]);
    }

    #[test]
    fn select_bases_to_prune_stable_when_mtime_ties() {
        // mtime 相同以名字降序兜底:保名字更大的(时间戳更新),删名字更小的。
        let entries = vec![
            ("base-20260701T010000Z".to_string(), ts(500)),
            ("base-20260701T020000Z".to_string(), ts(500)),
        ];
        let pruned = select_bases_to_prune(&entries, 1);
        assert_eq!(pruned, vec!["base-20260701T010000Z".to_string()]);
    }

    #[test]
    fn dir_total_bytes_sums_files_recursively_and_zero_when_missing() {
        // 递归求和:根下文件 + 子目录里文件都计入;不存在的目录返回 0。
        let unique = format!(
            "xrayc-wal-dirsize-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let root = std::env::temp_dir().join(unique);
        let nested = root.join("sub");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("a.bin"), vec![0u8; 100]).unwrap();
        fs::write(nested.join("b.bin"), vec![0u8; 23]).unwrap();

        assert_eq!(dir_total_bytes(&root), 123, "根 100 + 子目录 23 = 123");
        // 清理临时目录,不留残档。
        let _ = fs::remove_dir_all(&root);
        assert_eq!(dir_total_bytes(&root), 0, "目录不存在返回 0");
    }

    #[test]
    fn parse_backup_label_start_wal_extracts_segment_name() {
        // 标准 backup_label:取 START WAL LOCATION 行括号里的 24 位 WAL 段名。
        let label = "\
START WAL LOCATION: 0/2000028 (file 000000010000000000000002)
CHECKPOINT LOCATION: 0/2000060
BACKUP METHOD: streamed
BACKUP FROM: primary
START TIME: 2026-07-01 08:00:00 UTC
LABEL: pg_basebackup base backup
";
        assert_eq!(
            parse_backup_label_start_wal(label),
            Some("000000010000000000000002".to_string())
        );
    }

    #[test]
    fn parse_backup_label_start_wal_none_when_no_file_token() {
        // 有 START WAL LOCATION 行但缺 `(file ...)`:解析失败 → None(上层保守不删)。
        let label = "START WAL LOCATION: 0/2000028\nCHECKPOINT LOCATION: 0/2000060\n";
        assert_eq!(parse_backup_label_start_wal(label), None);
    }

    #[test]
    fn parse_backup_label_start_wal_none_when_missing_or_empty() {
        // 完全没有起始 WAL 行 / 空文本 / 括号内为空:一律 None。
        assert_eq!(parse_backup_label_start_wal(""), None);
        assert_eq!(
            parse_backup_label_start_wal("CHECKPOINT LOCATION: 0/2000060\n"),
            None
        );
        assert_eq!(
            parse_backup_label_start_wal("START WAL LOCATION: 0/2000028 (file )\n"),
            None
        );
    }
}
