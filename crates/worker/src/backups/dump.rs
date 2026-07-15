//! 全量 pg_dump 备份执行细节:构造 pg_dump 命令、写临时 pgpass、
//! 解析 DATABASE_URL 连接要素、生成备份文件名、按保留期清理旧备份文件。
//! 该模块只落地"全量备份产物"相关逻辑;连接口令只写 0600 临时 pgpass、用完即删,
//! 绝不打印口令或连接串;文件清理靠文件名前缀 + 修改时间双重约束防误删。
//! create_dump 作为 DatabaseBackupRunner 的方法在此实现(读运行器私有字段),
//! 由编排层 mod.rs 调用。本头部满足中文说明约束。

use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration as StdDuration, SystemTime};

use anyhow::{anyhow, Context};
use chrono::{DateTime, Duration, Utc};
use percent_encoding::percent_decode_str;
use tokio::process::Command;
use tokio::time::timeout;
use url::Url;

use super::DatabaseBackupRunner;

const BACKUP_FILE_PREFIX: &str = "xrayc-postgres-";
const BACKUP_FILE_EXTENSION: &str = ".dump";

/// replication 连接在 `.pgpass` 里 database 段必须匹配的值(`replication` 或 `*`)。
/// pg_basebackup 走复制协议,`.pgpass` 匹配只看那一行的 database 段:传实际库名会匹配不上,
/// libpq 认为"没提供密码"(真机 `fe_sendauth: no password supplied` 的根因)。
/// pg_dump 走普通连接、不用此常量(仍传实际库名)。
pub(super) const PGPASS_REPLICATION_DATABASE: &str = "replication";

/// 排流量日志时要清空数据(保结构)的观测大表清单(spec §5.1,共 7 张)。
/// 只在此列的表会被 `--exclude-table-data` 排数据;计费/账务/订阅表绝不列入。
const TRAFFIC_LOG_TABLES: [&str; 7] = [
    "access_exit_probes",
    "access_exit_probe_states",
    "access_line_probes",
    "access_line_metric_snapshots",
    "node_runtime_metrics",
    "access_user_session_events",
    "subscription_pull_events",
];

/// 组装「排流量日志」对应的 pg_dump 参数(抽成纯函数便于单测)。
/// exclude=true 时对 7 张观测大表追加 `--exclude-table-data=<表>`——只删数据、保留表结构,
/// 恢复后表在但历史观测清空;exclude=false 时返回空,dump 保持全量。
pub(super) fn traffic_log_exclude_args(exclude: bool) -> Vec<String> {
    if !exclude {
        return Vec::new();
    }
    TRAFFIC_LOG_TABLES
        .iter()
        .map(|table| format!("--exclude-table-data={table}"))
        .collect()
}

impl DatabaseBackupRunner {
    pub(super) async fn create_dump(
        &self,
        backup_path: &Path,
        exclude_traffic_logs: bool,
    ) -> anyhow::Result<Output> {
        let connection = PgDumpConnection::parse(&self.database_url)?;
        // pg_dump 走普通连接:pgpass 的 database 段用实际库名,保持既有行为不变。
        let pgpass_path = write_pgpass_file(&self.backup_dir, &connection, &connection.database)?;
        let mut command = Command::new(&self.pg_dump_bin);
        command
            .kill_on_drop(true)
            .env_remove("DATABASE_URL")
            .env("PGHOST", &connection.host)
            .env("PGPORT", connection.port.to_string())
            .env("PGDATABASE", &connection.database)
            .env("PGUSER", &connection.username)
            .arg("--format=custom")
            .arg("--no-owner")
            .arg("--no-acl");
        // 排流量日志:对观测大表只删数据、保留结构(顺序无关,追加在通用参数后)。
        for arg in traffic_log_exclude_args(exclude_traffic_logs) {
            command.arg(arg);
        }
        command.arg("--file").arg(backup_path);
        if let Some(sslmode) = &connection.sslmode {
            command.env("PGSSLMODE", sslmode);
        }
        if let Some(pgpass_path) = &pgpass_path {
            command.env("PGPASSFILE", pgpass_path);
        }

        let output = timeout(self.dump_timeout, command.output()).await;
        if let Some(pgpass_path) = &pgpass_path {
            let _ = fs::remove_file(pgpass_path);
        }
        match output {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(error)) => Err(error).with_context(|| format!("run {}", self.pg_dump_bin)),
            Err(_) => Err(anyhow!(
                "{} timed out after {} seconds",
                self.pg_dump_bin,
                self.dump_timeout.as_secs()
            )),
        }
    }
}

pub(super) fn backup_file_name(now: DateTime<Utc>) -> String {
    format!(
        "{}{}{}",
        BACKUP_FILE_PREFIX,
        now.format("%Y%m%dT%H%M%SZ"),
        BACKUP_FILE_EXTENSION
    )
}

/// 判定文件名是否是「自动全量 dump」:严格匹配 `xrayc-postgres-<YYYYMMDDTHHMMSSZ>.dump`。
/// 时间戳段必须是 8 位数字 + 'T' + 6 位数字 + 'Z'(共 16 字符)。
/// 红线:backup_dir 里可能混有人工命名 dump(如 `xrayc-postgres-damaged-before-rebuild-…`、
/// `xrayc-postgres-before-restore-…`),前缀同为 `xrayc-postgres-` 但时间戳段非法——必须排除,
/// 否则其字母字典序 > 数字,会被 latest_dump_file 误当成「最新」而让邮件误发损坏/过期的人工备份。
fn is_automated_dump_name(name: &str) -> bool {
    let Some(middle) = name
        .strip_prefix(BACKUP_FILE_PREFIX)
        .and_then(|rest| rest.strip_suffix(BACKUP_FILE_EXTENSION))
    else {
        return false;
    };
    let bytes = middle.as_bytes();
    bytes.len() == 16
        && bytes[0..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'Z'
}

/// 从候选文件名里挑「最新自动全量 dump」:先按 is_automated_dump_name 严格过滤
/// (排除人工命名 dump),再取字典序最大——时间戳零填充,过滤后字典序最大即时间最新。抽纯函数便于单测。
fn pick_latest_dump_name<I: IntoIterator<Item = String>>(names: I) -> Option<String> {
    names
        .into_iter()
        .filter(|name| is_automated_dump_name(name))
        .max()
}

/// 找 backup_dir 里「最新」的自动全量 dump 文件(只认普通文件 + 严格时间戳命名)。
/// 邮件模式据此取最新 dump 作附件源(run_now==email 时可取上一轮已存在的产物)。无匹配返回 None。
pub(super) fn latest_dump_file(backup_dir: &Path) -> Option<PathBuf> {
    let read = fs::read_dir(backup_dir).ok()?;
    let names = read.flatten().filter_map(|entry| {
        if entry.metadata().map(|meta| meta.is_file()).unwrap_or(false) {
            Some(entry.file_name().to_string_lossy().into_owned())
        } else {
            None
        }
    });
    pick_latest_dump_name(names).map(|name| backup_dir.join(name))
}

pub(super) async fn prune_old_backup_files(
    backup_dir: &Path,
    retention_days: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<u64> {
    let cutoff = now - Duration::days(retention_days.max(1));
    let mut removed = 0;
    for entry in fs::read_dir(backup_dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(BACKUP_FILE_PREFIX) || !file_name.ends_with(BACKUP_FILE_EXTENSION)
        {
            continue;
        }
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let modified_at = metadata.modified().unwrap_or(SystemTime::now());
        let modified_at: DateTime<Utc> = modified_at.into();
        if modified_at < cutoff {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PgDumpConnection {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) database: String,
    pub(super) username: String,
    pub(super) password: Option<String>,
    pub(super) sslmode: Option<String>,
}

impl PgDumpConnection {
    pub(super) fn parse(database_url: &str) -> anyhow::Result<Self> {
        let url = Url::parse(database_url).context("parse DATABASE_URL for backup")?;
        if url.scheme() != "postgres" && url.scheme() != "postgresql" {
            return Err(anyhow!(
                "DATABASE_URL scheme must be postgres or postgresql"
            ));
        }
        let host = url
            .host_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("DATABASE_URL host is required"))?
            .to_string();
        let database = decode_url_component(url.path().trim_start_matches('/'));
        if database.is_empty() {
            return Err(anyhow!("DATABASE_URL database name is required"));
        }
        let username = decode_url_component(url.username());
        if username.is_empty() {
            return Err(anyhow!("DATABASE_URL username is required"));
        }
        let sslmode = url
            .query_pairs()
            .find_map(|(key, value)| (key == "sslmode").then(|| value.into_owned()))
            .filter(|value| !value.is_empty());
        Ok(Self {
            host,
            port: url.port().unwrap_or(5432),
            database,
            username,
            password: url.password().map(decode_url_component),
            sslmode,
        })
    }
}

// 提为 pub(super):WAL 侧 pg_basebackup 也要写同款 0600 临时 pgpass(口令不进 argv),
// 复用此实现避免重复造轮子;仅放开 backups 模块内可见,不外泄。
// `database_field` 由调用方按连接类型显式给:pg_dump 传实际库名(普通连接),
// pg_basebackup 传 PGPASS_REPLICATION_DATABASE(复制连接的 database 段只认 `replication`/`*`);
// 两条路各写各的 pgpass、互不影响,故 pg_dump 现有行为(实际库名)保持不变。
pub(super) fn write_pgpass_file(
    backup_dir: &Path,
    connection: &PgDumpConnection,
    database_field: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(password) = &connection.password else {
        return Ok(None);
    };
    let path = backup_dir.join(format!(
        ".xrayc-pgpass-{}-{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("create pgpass file {:?}", path))?;
    writeln!(
        file,
        "{}",
        pgpass_line(
            &connection.host,
            connection.port,
            database_field,
            &connection.username,
            password,
        )
    )?;
    Ok(Some(path))
}

/// 拼装单行 `.pgpass` 内容 `host:port:database:user:password`(抽纯函数便于单测)。
/// `database` 段由调用方按连接类型传入:pg_dump 走普通连接传实际库名;
/// pg_basebackup 走复制协议、`.pgpass` 的 database 段只认 `replication`(或 `*`),
/// 传实际库名会匹配不上、libpq 报"no password supplied"。
/// 各字段按 pgpass 规则转义反斜杠与冒号;口令只落此行、不进 argv/日志。
pub(super) fn pgpass_line(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        pgpass_escape(host),
        port,
        pgpass_escape(database),
        pgpass_escape(username),
        pgpass_escape(password)
    )
}

fn pgpass_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace(':', "\\:")
}

fn decode_url_component(value: &str) -> String {
    percent_decode_str(value).decode_utf8_lossy().into_owned()
}

pub(super) fn env_duration_seconds(name: &str, default: u64, min: u64, max: u64) -> StdDuration {
    let seconds = std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
        .clamp(min, max);
    StdDuration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_latest_dump_ignores_manually_named_dumps_and_takes_newest_timestamp() {
        // 真机踩到的 bug:backup_dir 里混有人工命名 dump(damaged-before-rebuild / before-restore),
        // 前缀同为 `xrayc-postgres-` 但时间戳段非法,且字母字典序 > 数字 → 旧实现 `&name > current`
        // 会把 `...damaged...` 当成最新,邮件误发损坏旧备份。修后只认严格时间戳段并取最新。
        let names = vec![
            "xrayc-postgres-20260701T111556Z.dump".to_string(),
            "xrayc-postgres-20260701T111659Z.dump".to_string(), // 最新自动全量 dump
            "xrayc-postgres-damaged-before-rebuild-20260609T143054Z.dump".to_string(),
            "xrayc-postgres-before-restore-20260615T042456Z.dump".to_string(),
            "unrelated.txt".to_string(),
        ];
        assert_eq!(
            pick_latest_dump_name(names),
            Some("xrayc-postgres-20260701T111659Z.dump".to_string())
        );
    }

    #[test]
    fn is_automated_dump_name_only_accepts_strict_timestamp_pattern() {
        assert!(is_automated_dump_name(
            "xrayc-postgres-20260701T111659Z.dump"
        ));
        // 人工命名(前缀同、时间戳段非法)一律排除
        assert!(!is_automated_dump_name(
            "xrayc-postgres-damaged-before-rebuild-20260609T143054Z.dump"
        ));
        assert!(!is_automated_dump_name(
            "xrayc-postgres-before-restore-20260615T042456Z.dump"
        ));
        // 段长不符 / 前缀不符 / 扩展名不符
        assert!(!is_automated_dump_name("xrayc-postgres-2026Z.dump"));
        assert!(!is_automated_dump_name("other-20260701T111659Z.dump"));
        assert!(!is_automated_dump_name(
            "xrayc-postgres-20260701T111659Z.txt"
        ));
        // 分隔符错位(T/Z 位置放数字或字母)
        assert!(!is_automated_dump_name(
            "xrayc-postgres-2026070XT111659Z.dump"
        ));
        assert!(!is_automated_dump_name(
            "xrayc-postgres-20260701X111659Z.dump"
        ));
    }

    #[test]
    fn backup_file_name_uses_stable_prefix_and_extension() {
        let now = DateTime::parse_from_rfc3339("2026-06-09T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            backup_file_name(now),
            "xrayc-postgres-20260609T080000Z.dump"
        );
    }

    #[test]
    fn pg_dump_connection_parses_database_url_without_exposing_password() {
        let connection =
            PgDumpConnection::parse("postgres://xrayc@postgres:15432/xrayc_prod?sslmode=require")
                .unwrap();

        assert_eq!(connection.host, "postgres");
        assert_eq!(connection.port, 15432);
        assert_eq!(connection.database, "xrayc_prod");
        assert_eq!(connection.username, "xrayc");
        assert_eq!(connection.password.as_deref(), None);
        assert_eq!(connection.sslmode.as_deref(), Some("require"));
    }

    #[test]
    fn pgpass_escape_escapes_colon_and_backslash() {
        assert_eq!(pgpass_escape(r"a:b\c"), r"a\:b\\c");
    }

    #[test]
    fn pgpass_line_uses_actual_database_field_for_pg_dump() {
        // pg_dump 走普通连接:.pgpass 的 database 段(第 3 段)必须是实际库名才能匹配上。
        let line = pgpass_line("postgres", 15432, "xrayc_prod", "xrayc", "s3cr3t");
        let fields: Vec<&str> = line.split(':').collect();
        assert_eq!(fields[2], "xrayc_prod");
        assert_eq!(line, "postgres:15432:xrayc_prod:xrayc:s3cr3t");
    }

    #[test]
    fn pgpass_line_uses_replication_database_field_for_pg_basebackup() {
        // pg_basebackup 走 replication 连接:database 段必须是 `replication`(或 `*`),
        // 传实际库名会匹配不上导致 libpq 报"no password supplied"(真机踩到的 bug)。
        let line = pgpass_line(
            "postgres",
            15432,
            PGPASS_REPLICATION_DATABASE,
            "xrayc",
            "s3cr3t",
        );
        let fields: Vec<&str> = line.split(':').collect();
        assert_eq!(fields[2], "replication");
        assert_eq!(PGPASS_REPLICATION_DATABASE, "replication");
        // 与 pg_dump 行的 database 段互不相同、彼此隔离。
        assert_ne!(fields[2], "xrayc_prod");
    }

    #[test]
    fn decode_url_component_decodes_reserved_characters() {
        assert_eq!(decode_url_component("p%40ss%3Aword"), "p@ss:word");
    }

    #[test]
    fn traffic_log_exclude_args_empty_when_disabled() {
        // 未开启排流量日志:不追加任何 --exclude-table-data,dump 保持全量。
        assert!(traffic_log_exclude_args(false).is_empty());
    }

    #[test]
    fn traffic_log_exclude_args_covers_exactly_seven_observation_tables() {
        // 开启后仅对 spec §5.1 的 7 张观测大表追加 --exclude-table-data(保结构只删数据)。
        let args = traffic_log_exclude_args(true);
        let expected = [
            "access_exit_probes",
            "access_exit_probe_states",
            "access_line_probes",
            "access_line_metric_snapshots",
            "node_runtime_metrics",
            "access_user_session_events",
            "subscription_pull_events",
        ];
        assert_eq!(args.len(), expected.len());
        for table in expected {
            assert!(
                args.contains(&format!("--exclude-table-data={table}")),
                "缺少观测表 {table} 的排除参数"
            );
        }
        // 每个参数都必须是 --exclude-table-data= 形态,不夹带其它 flag。
        for arg in &args {
            assert!(arg.starts_with("--exclude-table-data="));
        }
    }

    #[test]
    fn traffic_log_exclude_args_never_touches_billing_tables() {
        // 计费/账务/订阅必留表绝不能被排数据(否则恢复后计费错乱)。
        let args = traffic_log_exclude_args(true);
        let must_keep = [
            "usage_ledgers",
            "access_traffic_snapshots",
            "usage_daily_rollups",
            "usage_monthly_rollups",
            "user_subscriptions",
        ];
        for keep in must_keep {
            assert!(
                !args.iter().any(|arg| arg.contains(keep)),
                "必留表 {keep} 不得出现在排除列表"
            );
        }
        // 任何 rollup 汇总表都不得被排。
        assert!(!args.iter().any(|arg| arg.contains("rollup")));
    }
}
