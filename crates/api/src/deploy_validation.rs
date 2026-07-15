//! Agent 安装说明输入校验模块。
//! 本文件由原 API 入口按路由域拆分而来。
//! 只保留后台生成安装说明需要的字段校验 helper。
//! 模块保持 crate 内可见，供 lib.rs 路由装配使用。
//! 响应体、cookie、token 与审计摘要沿用原实现。
//! 数据库访问仍通过既有 PgStore 方法完成。
//! 内存模式回退逻辑保持原有分支。
//! 新增代码控制在 500 行以内便于审阅。
//! 中文注释位于文件前十行满足仓库约束。
//! 请勿在此写入部署主机、密钥或其它敏感信息。

use super::*;

pub(crate) fn require_nonzero_port(port: u16, field: &str) -> Result<u16, String> {
    if port == 0 {
        Err(format!("{field} 必须在 1-65535 之间"))
    } else {
        Ok(port)
    }
}

pub(crate) fn validate_expected_listen_ports(ports: Vec<u16>) -> Result<Vec<u16>, String> {
    let mut normalized = Vec::new();
    for port in ports {
        normalized.push(require_nonzero_port(port, "expected_listen_ports")?);
    }
    normalized.sort_unstable();
    normalized.dedup();
    Ok(normalized)
}

pub(crate) fn normalize_tls_cert_domains(
    requested: Vec<String>,
    inferred: Vec<String>,
) -> Result<Vec<String>, String> {
    let explicit = !requested.is_empty();
    let source = if requested.is_empty() {
        inferred
    } else {
        requested
    };
    let mut domains = Vec::new();
    for raw in source {
        let domain = validate_request_text(raw, "tls_cert_domains", 255)?;
        if domain.is_empty() {
            continue;
        }
        if !looks_like_tls_domain(&domain) {
            if !explicit {
                continue;
            }
            return Err("tls_cert_domains 只能填写可申请证书的域名".to_string());
        }
        domains.push(domain);
    }
    domains.sort();
    domains.dedup();
    Ok(domains)
}

pub(crate) fn looks_like_tls_domain(value: &str) -> bool {
    let value = value.trim().trim_end_matches('.');
    value.contains('.')
        && !value.eq_ignore_ascii_case("localhost")
        && value
            .chars()
            .all(|item| item.is_ascii_alphanumeric() || matches!(item, '-' | '.'))
        && value
            .split('.')
            .all(|label| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-'))
        && !value.split('.').all(|label| label.parse::<u8>().is_ok())
}

pub(crate) fn optional_request_text(
    value: Option<String>,
    field: &str,
    max_len: usize,
) -> Result<Option<String>, String> {
    value
        .map(|value| validate_request_text(value, field, max_len))
        .transpose()
        .map(|value| value.filter(|value| !value.is_empty()))
}

pub(crate) fn optional_multiline_secret_text(
    value: Option<String>,
    field: &str,
    max_len: usize,
) -> Result<Option<String>, String> {
    value
        .map(|value| validate_multiline_secret_text(value, field, max_len))
        .transpose()
        .map(|value| value.filter(|value| !value.is_empty()))
}

pub(crate) fn validate_request_text(
    value: String,
    field: &str,
    max_len: usize,
) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.len() > max_len {
        return Err(format!("{field} 长度不能超过 {max_len} 字符"));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(format!("{field} 不能包含换行或 NUL 字符"));
    }
    Ok(value)
}

pub(crate) fn validate_multiline_secret_text(
    value: String,
    field: &str,
    max_len: usize,
) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.len() > max_len {
        return Err(format!("{field} 长度不能超过 {max_len} 字符"));
    }
    if value.contains('\0') {
        return Err(format!("{field} 不能包含 NUL 字符"));
    }
    Ok(value)
}

pub(crate) fn validate_acme_email(value: Option<String>) -> Result<String, String> {
    let email = optional_request_text(value, "acme_email", 255)?.unwrap_or_default();
    if !email.is_empty() && (!email.contains('@') || email.starts_with('@') || email.ends_with('@'))
    {
        return Err("acme_email 格式无效".to_string());
    }
    Ok(email)
}

pub(crate) fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn infer_control_plane_url(headers: &HeaderMap) -> Option<String> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| *value == "http" || *value == "https")
        .unwrap_or("http");
    Some(format!("{proto}://{host}"))
}
