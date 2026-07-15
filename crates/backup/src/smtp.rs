//! SMTP 发送核心:配置解析、邮件构造(可带附件)、异步发送与错误脱敏。
//!
//! 与旧 `guards.rs` 口径保持一致:传输默认走 `relay()` 隐式 TLS(现网口径),
//! 发件人字段沿用 `smtp_from`(可含显示名,直接解析成 Mailbox)。
//! 无附件时发纯文本单 body,与旧行为等价;有附件时发 `multipart/mixed`(正文 + 附件)。
//! 任何发送失败的错误摘要都过 `sanitize_error` 脱敏,绝不带出 SMTP 明文密码。

use anyhow::{bail, Context, Result};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Message, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

/// SMTP 发送所需的连接与发件人配置。
///
/// 字段口径对齐旧 `guards.rs`:`from_address` 取 `smtp_from`(为空回退用户名),
/// 可直接是 `name <addr>` 形态;`from_name` 为可选显示名(旧 JSON 无此字段,恒为 None,
/// 保证与旧行为逐字节等价)。`use_tls=true` 表示走 `relay()` 隐式 TLS(现网口径);
/// `starttls=true` 且 `use_tls=false` 时走 `starttls_relay()`;两者皆 false 才明文直连。
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub from_name: Option<String>,
    pub use_tls: bool,
    pub starttls: bool,
}

/// 邮件附件:文件名、原始字节与 MIME 类型。
#[derive(Debug, Clone)]
pub struct EmailAttachment {
    pub filename: String,
    pub content: Vec<u8>,
    pub content_type: String,
}

/// 从 `auth_security.email_verification` 的 JSON 值解析出 `SmtpConfig`。
///
/// 入参是 `email_verification` 子对象本身;字段名对齐旧 `guards.rs` 的读法
/// (`smtp_host`/`smtp_port`/`smtp_username`/`smtp_password`/`smtp_from`)。
/// 关键字段(host/username/password)缺失或为空时返回 None,交由调用方给出
/// 「SMTP 配置不完整」提示,不在此处报错。
pub fn smtp_config_from_email_verification(value: &serde_json::Value) -> Option<SmtpConfig> {
    // 字段读法逐字对齐旧 guards.rs:host/username 去空白,password 原样取(不 trim),
    // 端口缺失回退 587 并 clamp 到合法范围,from 取 smtp_from(去空白且非空)否则回退用户名。
    let host = value
        .get("smtp_host")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let port = value
        .get("smtp_port")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(587)
        .clamp(1, 65535) as u16;
    let username = value
        .get("smtp_username")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let password = value
        .get("smtp_password")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let from_address = value
        .get("smtp_from")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(username.as_str())
        .to_string();

    // 与旧口径一致:host/username/password/from 任一为空即视为配置不完整,返回 None。
    if host.is_empty() || username.is_empty() || password.is_empty() || from_address.is_empty() {
        return None;
    }

    Some(SmtpConfig {
        host,
        port,
        username,
        password,
        from_address,
        // 旧 JSON 无独立显示名字段,恒为 None,保证发件人 From 头与旧行为逐字节等价。
        from_name: None,
        // 旧口径固定走 relay() 隐式 TLS。
        use_tls: true,
        starttls: false,
    })
}

/// 纯构造函数:根据配置与内容拼出一封 lettre 邮件,便于单测断言、不触网。
///
/// 无附件时返回纯文本单 body;有附件时返回 `multipart/mixed`(正文 + 附件)。
pub fn build_message(
    smtp: &SmtpConfig,
    recipients: &[String],
    subject: &str,
    body_text: &str,
    attachment: Option<&EmailAttachment>,
) -> Result<Message> {
    if recipients.is_empty() {
        bail!("收件人列表为空");
    }

    let mut builder = Message::builder()
        .from(from_mailbox(smtp)?)
        .subject(subject);
    // 逐个追加收件人:lettre 的 to() 会 join 进同一个 To 头,不会互相覆盖。
    for recipient in recipients {
        let mailbox = recipient
            .parse::<Mailbox>()
            .with_context(|| format!("收件邮箱无效: {recipient}"))?;
        builder = builder.to(mailbox);
    }

    let message = match attachment {
        // 无附件:纯文本单 body,与旧 guards.rs 行为等价。
        None => builder
            .body(body_text.to_string())
            .context("邮件内容构建失败")?,
        // 有附件:multipart/mixed,第一段正文纯文本,第二段为附件。
        Some(att) => {
            let content_type = ContentType::parse(&att.content_type)
                .with_context(|| format!("附件类型无效: {}", att.content_type))?;
            let body_part = SinglePart::plain(body_text.to_string());
            let attach_part =
                Attachment::new(att.filename.clone()).body(att.content.clone(), content_type);
            let multipart = MultiPart::mixed()
                .singlepart(body_part)
                .singlepart(attach_part);
            builder.multipart(multipart).context("邮件内容构建失败")?
        }
    };

    Ok(message)
}

/// 构造发件人 Mailbox:有显示名则 `Mailbox::new(name, addr)`,否则整串按 Mailbox 解析
/// (与旧 guards.rs 的 `from.parse::<Mailbox>()` 一致,支持 `Name <addr>` 形态)。
fn from_mailbox(smtp: &SmtpConfig) -> Result<Mailbox> {
    match smtp.from_name.as_deref() {
        Some(name) if !name.trim().is_empty() => {
            let address = smtp
                .from_address
                .parse::<Address>()
                .with_context(|| format!("SMTP 发件人无效: {}", smtp.from_address))?;
            Ok(Mailbox::new(Some(name.to_string()), address))
        }
        _ => smtp
            .from_address
            .parse::<Mailbox>()
            .with_context(|| format!("SMTP 发件人无效: {}", smtp.from_address)),
    }
}

/// 脱敏错误摘要:把原始错误串里出现的 SMTP 明文密码替换为掩码。
/// 密码为空时原样返回,避免把整串误抹成掩码。
pub fn sanitize_error(smtp: &SmtpConfig, raw: &str) -> String {
    if smtp.password.is_empty() {
        return raw.to_string();
    }
    raw.replace(&smtp.password, "***")
}

/// 异步发送邮件。无附件发纯文本(与旧行为等价),有附件发 `multipart/mixed`。
///
/// 传输构造沿用旧口径(`relay(host).port(port).credentials(..)`);失败错误摘要脱敏。
pub async fn send_email(
    smtp: &SmtpConfig,
    recipients: &[String],
    subject: &str,
    body_text: &str,
    attachment: Option<EmailAttachment>,
) -> Result<()> {
    let message = build_message(smtp, recipients, subject, body_text, attachment.as_ref())
        .map_err(|error| anyhow::anyhow!(sanitize_error(smtp, &error.to_string())))?;

    let credentials = Credentials::new(smtp.username.clone(), smtp.password.clone());
    // 传输选择沿用旧口径:默认 relay() 隐式 TLS;仅当显式关 use_tls 时才走 STARTTLS 或明文。
    let builder = if smtp.use_tls {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
    } else if smtp.starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
    } else {
        Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            &smtp.host,
        ))
    };
    let mailer = builder
        .map_err(|error| {
            anyhow::anyhow!(sanitize_error(smtp, &format!("SMTP 连接配置失败: {error}")))
        })?
        .port(smtp.port)
        .credentials(credentials)
        .build();

    mailer
        .send(message)
        .await
        // 发送失败/凭据错误的错误摘要必须脱敏,绝不带出明文密码。
        .map_err(|error| anyhow::anyhow!(sanitize_error(smtp, &error.to_string())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_config() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.example.test".to_string(),
            port: 587,
            username: "noreply@example.test".to_string(),
            password: "smtp-secret-placeholder".to_string(),
            from_address: "noreply@example.test".to_string(),
            from_name: None,
            use_tls: true,
            starttls: false,
        }
    }

    // 完整 JSON 应解析出全部字段,use_tls 默认为 true(对齐 relay() 现网口径)。
    #[test]
    fn parse_full_email_verification_config() {
        let value = json!({
            "enabled": true,
            "smtp_host": "smtp.example.test",
            "smtp_port": 465,
            "smtp_username": "noreply@example.test",
            "smtp_from": "XrayC <noreply@example.test>",
            "smtp_password": "smtp-secret-placeholder"
        });
        let cfg = smtp_config_from_email_verification(&value).expect("应解析出配置");
        assert_eq!(cfg.host, "smtp.example.test");
        assert_eq!(cfg.port, 465);
        assert_eq!(cfg.username, "noreply@example.test");
        assert_eq!(cfg.password, "smtp-secret-placeholder");
        assert_eq!(cfg.from_address, "XrayC <noreply@example.test>");
        assert!(cfg.use_tls, "默认应走隐式 TLS(relay)");
    }

    // smtp_from 缺失时,发件人回退用户名,端口缺失回退 587。
    #[test]
    fn parse_defaults_from_to_username_and_port_587() {
        let value = json!({
            "smtp_host": "smtp.example.test",
            "smtp_username": "noreply@example.test",
            "smtp_password": "smtp-secret-placeholder"
        });
        let cfg = smtp_config_from_email_verification(&value).expect("应解析出配置");
        assert_eq!(cfg.port, 587, "端口缺失应回退 587");
        assert_eq!(
            cfg.from_address, "noreply@example.test",
            "发件人缺失应回退用户名"
        );
    }

    // 缺 host 关键字段应返回 None。
    #[test]
    fn parse_returns_none_when_host_missing() {
        let value = json!({
            "smtp_username": "noreply@example.test",
            "smtp_password": "smtp-secret-placeholder"
        });
        assert!(smtp_config_from_email_verification(&value).is_none());
    }

    // 缺密码关键字段应返回 None。
    #[test]
    fn parse_returns_none_when_password_missing() {
        let value = json!({
            "smtp_host": "smtp.example.test",
            "smtp_username": "noreply@example.test"
        });
        assert!(smtp_config_from_email_verification(&value).is_none());
    }

    // 无附件:构造出的邮件是单 body(非 multipart),正文原样嵌入。
    // 说明:lettre 的 MessageBuilder::body() 只写 Content-Transfer-Encoding、不写 Content-Type
    // (与旧 guards.rs 一致),故这里用 ASCII 正文(7bit 原样)断言 body 内容而非 text/plain 头。
    #[test]
    fn build_message_without_attachment_is_single_plain_body() {
        let cfg = sample_config();
        let msg = build_message(
            &cfg,
            &["rcpt@example.test".to_string()],
            "主题",
            "single-plain-body-marker",
            None,
        )
        .expect("应构造成功");
        let formatted = String::from_utf8_lossy(&msg.formatted()).to_string();
        assert!(
            formatted.contains("single-plain-body-marker"),
            "应原样含单 body 正文"
        );
        assert!(!formatted.contains("multipart"), "无附件不应出现 multipart");
    }

    // 有附件:构造出 multipart/mixed,含附件文件名与 content-type。
    #[test]
    fn build_message_with_attachment_is_multipart_mixed() {
        let cfg = sample_config();
        let attachment = EmailAttachment {
            filename: "xrayc-backup.sql.enc".to_string(),
            content: b"fake-dump-bytes".to_vec(),
            content_type: "application/octet-stream".to_string(),
        };
        let msg = build_message(
            &cfg,
            &["rcpt@example.test".to_string()],
            "备份",
            "备份正文",
            Some(&attachment),
        )
        .expect("应构造成功");
        let formatted = String::from_utf8_lossy(&msg.formatted()).to_string();
        assert!(
            formatted.contains("multipart/mixed"),
            "有附件应为 multipart/mixed"
        );
        assert!(formatted.contains("xrayc-backup.sql.enc"), "应含附件文件名");
        assert!(
            formatted.contains("application/octet-stream"),
            "应含附件 content-type"
        );
        assert!(formatted.contains("text/plain"), "仍应含正文纯文本部分");
    }

    // 多收件人都应出现在 To 头。
    #[test]
    fn build_message_lists_all_recipients_in_to() {
        let cfg = sample_config();
        let msg = build_message(
            &cfg,
            &[
                "first@example.test".to_string(),
                "second@example.test".to_string(),
            ],
            "主题",
            "正文",
            None,
        )
        .expect("应构造成功");
        let formatted = String::from_utf8_lossy(&msg.formatted()).to_string();
        assert!(formatted.contains("first@example.test"), "缺第一个收件人");
        assert!(formatted.contains("second@example.test"), "缺第二个收件人");
    }

    // 空收件人应报错(无法构造有效邮件)。
    #[test]
    fn build_message_errors_on_empty_recipients() {
        let cfg = sample_config();
        let result = build_message(&cfg, &[], "主题", "正文", None);
        assert!(result.is_err(), "空收件人应返回错误");
    }

    // 错误脱敏:原始错误里的明文密码必须被替换为掩码。
    #[test]
    fn sanitize_error_redacts_password() {
        let cfg = sample_config();
        let raw = format!(
            "auth failed for user {} with password {}",
            cfg.username, cfg.password
        );
        let cleaned = sanitize_error(&cfg, &raw);
        assert!(
            !cleaned.contains("smtp-secret-placeholder"),
            "脱敏后不得包含明文密码"
        );
        assert!(cleaned.contains("***"), "应以掩码替换密码");
    }

    // 密码为空时脱敏不应误伤(不把整串抹成掩码)。
    #[test]
    fn sanitize_error_keeps_text_when_password_empty() {
        let mut cfg = sample_config();
        cfg.password = String::new();
        let raw = "connection refused";
        assert_eq!(sanitize_error(&cfg, raw), "connection refused");
    }
}
