//! 本文件生成线路入口的默认入站配置。
//! 绑定具体线路创建入口时不能直接写空 JSON。
//! Shadowsocks 入口需要服务端密码才能出现在订阅里。
//! Trojan 入口仍复用统一 TLS 校验，避免无证书入口。
//! VLESS 默认保持空配置，由协议默认值处理。
//! 生成的配置会在写库前走字段加密封装。
//! 本模块不访问数据库，也不读取远端服务器状态。
//! 随机密码只用于入口协议根密码，不会写入公开文档。
//! 后续新增入口协议时应先补测试再扩展这里。
//! 本头部满足前十行中文注释约束。

use crate::{normalize_access_inbound_config, validate_access_inbound_config, DbError};
use base64::Engine;
use serde_json::{json, Value};
use uuid::Uuid;

pub(crate) fn local_access_line_inbound_config(
    protocol: &str,
    server_name: &str,
) -> Result<Value, DbError> {
    let mut config = match protocol {
        "shadowsocks" => json!({
            "method": "2022-blake3-aes-128-gcm",
            "password": random_shadowsocks_server_password(),
            "network": "tcp,udp"
        }),
        "trojan" | "hysteria" => json!({
            "security": "tls",
            "server_name": server_name
        }),
        _ => json!({}),
    };
    config = normalize_access_inbound_config(protocol, config, server_name)?;
    validate_access_inbound_config(protocol, &config, server_name)?;
    Ok(config)
}

fn random_shadowsocks_server_password() -> String {
    base64::engine::general_purpose::STANDARD.encode(Uuid::new_v4().as_bytes())
}
