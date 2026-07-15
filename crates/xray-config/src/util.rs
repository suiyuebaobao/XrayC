//! 本模块提供配置编译过程共用的小工具函数。
//! 工具函数只在包内部使用，避免把校验细节暴露到公开接口。
//! 错误构造逻辑集中在这里，方便入站和出站编译器复用。

use crate::XrayConfigError;

pub(crate) fn default_if_empty<'a>(value: &'a str, default: &'a str) -> &'a str {
    let value = value.trim();
    if value.is_empty() {
        default
    } else {
        value
    }
}

pub(crate) fn default_if_empty_option<'a>(value: Option<&'a str>, default: &'a str) -> &'a str {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
}

pub(crate) fn non_empty_option(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub(crate) fn require_field<'a>(
    endpoint_id: &str,
    field: &'static str,
    value: &'a str,
) -> Result<&'a str, XrayConfigError> {
    if value.trim().is_empty() {
        Err(XrayConfigError::MissingExitField(
            endpoint_id.to_owned(),
            field,
        ))
    } else {
        Ok(value)
    }
}

pub(crate) fn require_port(endpoint_id: &str, port: u16) -> Result<u16, XrayConfigError> {
    if port == 0 {
        Err(XrayConfigError::MissingExitField(
            endpoint_id.to_owned(),
            "port",
        ))
    } else {
        Ok(port)
    }
}

pub(crate) fn require_access_field<'a>(
    line_id: &str,
    field: &'static str,
    value: &'a str,
) -> Result<&'a str, XrayConfigError> {
    if value.trim().is_empty() {
        Err(XrayConfigError::UnsupportedAccessProtocol(format!(
            "{line_id} missing {field}"
        )))
    } else {
        Ok(value)
    }
}
