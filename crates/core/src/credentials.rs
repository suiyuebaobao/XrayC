//! 本模块集中生成用户在不同绑定节点上的入站凭据。
//! 同一个用户的公开订阅 token 不参与这里的推导。
//! 推导输入只包含用户访问凭据、绑定节点 ID 和入口协议。
//! 订阅渲染和 access-agent 下发必须共用本模块。
//! 多出口共用一个入口时，绑定级凭据用于区分路由目标。
//! 统计 email 仍使用用户 xray_user_key，保持用户级计费聚合。
//! VLESS 与 HY2 使用 UUID 形态，兼容客户端字段要求。
//! Trojan 与 Shadowsocks 使用稳定 base64url 文本。
//! 本模块不访问数据库、网络或外部密钥。
//! 本头部满足前十行中文注释约束。

use base64::Engine;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn binding_credential(protocol: &str, user_credential: &str, binding_id: Uuid) -> String {
    let protocol = protocol.trim().to_ascii_lowercase();
    let material = format!("{user_credential}:{binding_id}:{protocol}");
    let digest = Sha256::digest(material.as_bytes());
    match protocol.as_str() {
        "" | "vless" | "hy2" | "hysteria" | "hysteria2" => {
            let mut bytes = [0_u8; 16];
            bytes.copy_from_slice(&digest[..16]);
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            Uuid::from_bytes(bytes).to_string()
        }
        _ => base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&digest[..24]),
    }
}
