//! xrayc-backup:备份相关的共享能力 crate,供 api 与 worker 复用。
//!
//! 首版只承载「支持附件的 SMTP 邮件发送」:把原本内嵌在
//! `crates/api/src/guards.rs` 的纯文本发送逻辑下沉到这里,并扩展为可带附件的
//! `multipart/mixed` 邮件,供 api(发验证码,无附件)与 worker(发备份文件,带附件)共用。
//!
//! 运行边界:本 crate 只负责构造与发送邮件,不读数据库、不碰站点设置的持久化。
//! SMTP 明文密码由调用方从 `site_settings.auth_security.email_verification` 取出后传入,
//! 本 crate 只在内存中使用、绝不写日志;发送失败的错误摘要统一脱敏(剔除密码)。
//! 后续备份编排(异地同步 / provision 装公钥等)再按 spec §8 扩充到本 crate。

mod provision;
mod smtp;

pub use provision::{ensure_backup_keypair, provision_offsite, ProvisionResult};
pub use smtp::{
    build_message, sanitize_error, send_email, smtp_config_from_email_verification,
    EmailAttachment, SmtpConfig,
};
