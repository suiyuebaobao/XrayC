// 数据库测试分片 25。
// 本文件覆盖 Shadowsocks 2022 密钥写库校验。
// 2022 方法要求服务端根密码是 base64 后的固定长度密钥。
// 测试直接调用校验层，避免真实密码进入日志。
// 合法样例使用固定字节生成，便于稳定复现。
// 非 2022 方法保持兼容，不强制 base64 密钥。
// 出口端点和中转入口都必须覆盖。
// 该分片不连接远端服务器，不访问外部网络。
// 文件行数保持低于五百行。
// 本头部满足前十行中文注释约束。

use base64::{engine::general_purpose, Engine as _};

#[test]
fn test_shadowsocks_2022_inbound_rejects_invalid_root_password() {
    let err = validate_access_inbound_config(
        "shadowsocks",
        &json!({
            "method": "2022-blake3-aes-128-gcm",
            "password": "not-base64"
        }),
        "",
    )
    .expect_err("invalid 2022 inbound password should fail");
    assert!(err.to_string().contains("base64"));

    let valid = general_purpose::STANDARD.encode([7_u8; 16]);
    validate_access_inbound_config(
        "shadowsocks",
        &json!({
            "method": "2022-blake3-aes-128-gcm",
            "password": valid
        }),
        "",
    )
    .expect("valid 2022 inbound password should pass");
}
#[test]
fn test_shadowsocks_2022_endpoint_rejects_wrong_key_length() {
    let short_key = general_purpose::STANDARD.encode([3_u8; 16]);
    let err = validate_exit_endpoint_protocol_config(
        "shadowsocks",
        &json!({
            "method": "2022-blake3-aes-256-gcm",
            "password": short_key
        }),
    )
    .expect_err("wrong 2022 endpoint key length should fail");
    assert!(err.to_string().contains("32 字节"));

    let valid = general_purpose::STANDARD.encode([9_u8; 32]);
    validate_exit_endpoint_protocol_config(
        "shadowsocks",
        &json!({
            "method": "2022-blake3-aes-256-gcm",
            "password": valid
        }),
    )
    .expect("valid 2022 endpoint key should pass");
}
