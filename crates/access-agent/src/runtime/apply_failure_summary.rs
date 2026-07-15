//! 本模块负责把配置命令（`xray -test`/reload）的失败输出压成**可诊断又脱敏**的摘要。
//! 运行边界：纯字符串处理、无副作用、无 I/O；只被 apply 模块在命令失败路径调用。
//! 核心用途（BUG-G 可观测性）：历史实现只回 "stdout redacted (N bytes)"，真因整段被丢，
//! 生产里校验失败无法定位。这里保留携带错误类型的行、剥掉 token/密码/证书私钥正文。
//! 中文文件头满足仓库约束；请勿在此回显真实凭据/证书正文。

/// 把命令失败输出压成可诊断又脱敏的摘要。
///
/// 保留携带错误类型的行（如 `failed to parse ...`/`invalid config`/`no such file`），
/// 同时剥掉 token/密码/证书私钥正文等敏感串。stderr/stdout 都纳入；都空时给可读非空兜底，绝不静默。
pub(super) fn command_failure_summary(stdout: &[u8], stderr: &[u8]) -> String {
    let mut kept: Vec<String> = Vec::new();
    // stderr 优先（reload/底层错误多在此），再补 stdout（xray -test 的错误进 stdout）。
    for raw in [stderr, stdout] {
        if raw.is_empty() {
            continue;
        }
        for line in String::from_utf8_lossy(raw).lines() {
            let line = line.trim();
            if line.is_empty() || !line_looks_like_error(line) {
                continue;
            }
            let scrubbed = scrub_sensitive_substrings(line);
            if !scrubbed.trim().is_empty() && !kept.iter().any(|existing| existing == &scrubbed) {
                kept.push(scrubbed);
            }
            // 摘要只取前若干条错误行，避免日志/上报过长。
            if kept.len() >= 5 {
                break;
            }
        }
        if kept.len() >= 5 {
            break;
        }
    }

    let total_bytes = stdout.len() + stderr.len();
    if kept.is_empty() {
        // 没有可识别的错误行（或两路都空）：给可读非空兜底，附原始字节量供判断量级。
        if total_bytes == 0 {
            return "命令退出状态非 0（无输出）".to_owned();
        }
        return format!("命令退出状态非 0（{total_bytes} 字节输出，无可识别错误行，已脱敏）");
    }
    format!("{}（原始 {total_bytes} 字节，已脱敏）", kept.join(" | "))
}

/// 判断一行是否像“错误/诊断”行：命中任一 Xray/系统常见错误标志即保留。
/// 仅保留错误类型行而非整段输出，是“可诊断”与“不泄露/不冗长”之间的折中。
fn line_looks_like_error(line: &str) -> bool {
    const MARKERS: [&str; 12] = [
        "error",
        "failed",
        "fail to",
        "invalid",
        "cannot",
        "unable",
        "no such file",
        "not found",
        "parse",
        "panic",
        "refused",
        "denied",
    ];
    let lower = line.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}

/// 剥掉一行里的敏感串：`key=value`/`key: value` 里敏感键的值、长十六进制/base64
/// 凭据块、PEM 私钥/证书正文。保留错误语义词，只把疑似密钥/口令替换成占位。
fn scrub_sensitive_substrings(line: &str) -> String {
    // PEM 正文行（含 BEGIN/END 或一长串纯 base64）整体替换，绝不回显证书/私钥。
    if line.contains("BEGIN ") || line.contains("END ") || is_probable_secret_blob(line) {
        return "[redacted-secret]".to_owned();
    }
    line.split_whitespace()
        .map(scrub_token)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 处理单个以空白分隔的片段：若是 `敏感键=值`/`敏感键:值` 形态则只替换值；
/// 若整体是长十六进制/base64 凭据则整体替换；否则原样保留（保住错误语义）。
fn scrub_token(token: &str) -> String {
    for sep in ['=', ':'] {
        if let Some(pos) = token.find(sep) {
            let (key, rest) = token.split_at(pos);
            let value = &rest[1..];
            if key_is_sensitive(key) && !value.is_empty() {
                return format!("{key}{sep}[redacted]");
            }
        }
    }
    if is_probable_secret_blob(token) {
        return "[redacted-secret]".to_owned();
    }
    token.to_owned()
}

/// 敏感键名判定（大小写不敏感、容忍前后标点）：token/password/secret/auth/psk/key 等。
fn key_is_sensitive(key: &str) -> bool {
    const SENSITIVE: [&str; 9] = [
        "token",
        "password",
        "passwd",
        "secret",
        "auth",
        "psk",
        "apikey",
        "api_key",
        "credential",
    ];
    let key = key.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
    let lower = key.to_ascii_lowercase();
    SENSITIVE.iter().any(|name| lower.contains(name))
        // “key”单独成键才算敏感（避免命中 monkey/keyword 等错误语义词）。
        || lower == "key"
        || lower.ends_with("_key")
}

/// 判断一段连续字符是否像凭据 blob：足够长且只由 base64/hex 字符组成。
/// 阈值取 24，避免误伤普通英文单词与路径；订阅 token/密钥/证书段都远超此长度。
fn is_probable_secret_blob(token: &str) -> bool {
    let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '+' && c != '/');
    if token.len() < 24 {
        return false;
    }
    token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        // 至少含一个数字，排除纯字母的长英文短语。
        && token.chars().any(|c| c.is_ascii_digit())
}
