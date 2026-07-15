//! 备份相关脱敏工具:从命令输出/错误文本里抹掉数据库连接串与口令,
//! 并收集需要脱敏的敏感词(连接串本身 + 从中解析出的口令)。
//! 任何写回状态、日志、错误摘要前都必须过这里,避免口令/连接信息外泄。
//! 输出统一压平换行、截断长度,空结果回退为通用提示。
//! 依赖 dump 子模块解析连接以取出口令。本头部满足中文说明约束。

use super::dump::PgDumpConnection;

pub(super) fn backup_secrets(database_url: &str) -> Vec<String> {
    let mut secrets = vec![database_url.to_string()];
    if let Ok(connection) = PgDumpConnection::parse(database_url) {
        if let Some(password) = connection.password {
            secrets.push(password);
        }
    }
    secrets
}

pub(super) fn sanitize_command_output(raw: &[u8], secrets: &[String]) -> String {
    let text = String::from_utf8_lossy(raw);
    sanitize_text(text.into_owned(), secrets)
}

pub(super) fn sanitize_text(text: String, secrets: &[String]) -> String {
    let mut text = text.replace(['\n', '\r'], " ");
    for secret in secrets {
        if !secret.is_empty() {
            text = text.replace(secret, "<redacted>");
        }
    }
    let text = text.trim();
    if text.is_empty() {
        "pg_dump exited with non-zero status".to_string()
    } else {
        text.chars().take(240).collect()
    }
}

/// 通用错误摘要脱敏(编排层给 offsite/wal/email 落状态用):抹掉连接串/口令、压平换行、
/// 截断长度。与 sanitize_text 的区别是**不套 pg_dump 语境的兜底文案**(空则回退通用提示),
/// 避免异地/WAL/邮件的状态里出现误导性的 "pg_dump" 字样。
pub(super) fn sanitize_message(text: &str, secrets: &[String]) -> String {
    let mut text = text.replace(['\n', '\r'], " ");
    for secret in secrets {
        if !secret.is_empty() {
            text = text.replace(secret, "<redacted>");
        }
    }
    let text = text.trim();
    if text.is_empty() {
        "命令执行失败".to_string()
    } else {
        text.chars().take(240).collect()
    }
}
