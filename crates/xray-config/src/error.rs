//! 本模块定义配置编译阶段的错误类型。
//! 错误枚举保持原有公开接口，供上层工作器和测试直接匹配。
//! 每个变体对应一个可定位的配置输入问题。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum XrayConfigError {
    #[error("access line `{0}` has no users")]
    EmptyUsers(String),
    #[error("exit endpoint `{0}` is missing required field `{1}`")]
    MissingExitField(String, &'static str),
    #[error("exit endpoint `{0}` uses disabled direct protocol")]
    DirectExitDisabled(String),
    #[error("unsupported access protocol `{0}`")]
    UnsupportedAccessProtocol(String),
    #[error("unsupported exit protocol `{0}`")]
    UnsupportedExitProtocol(String),
}
