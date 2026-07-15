/// 数据库测试分片 49。
// 本文件是测试模块的 include 分片，保持原测试逻辑。
// 分片只用于满足单文件五百行限制，不改变测试作用域。
// 所有测试项在父级 tests 模块中按顺序拼接。
// 公共导入和内部 helper 由 tests/mod.rs 统一提供。
// 这里不保存环境变量、连接信息或外部服务器资料。
// 本分片承载 CF 协议护栏纯函数校验,无需 PostgreSQL。
// 断言对齐 spec §2 协议×CF 矩阵: 仅 VLESS-WS-TLS 可过 CF。
// 文件头部使用中文注释满足仓库拆分约束。
// 本头部满足前十行中文注释约束。

    #[test]
    fn test_cf_entry_requires_vless_ws() {
        // cdn_enabled=false 时任何协议都放行(不约束直连组合)。
        assert!(validate_protocol_cdn_combination("trojan", "tcp", "tls", false, None).is_ok());
        assert!(validate_protocol_cdn_combination("hysteria", "hysteria", "tls", false, None).is_ok());

        // cdn_enabled=true:VLESS + ws + tls → 唯一允许的过 CF 组合。
        assert!(
            validate_protocol_cdn_combination("vless", "ws", "tls", true, None).is_ok(),
            "VLESS-WS-TLS 应允许过 CF"
        );

        // cdn_enabled=true:非 VLESS-WS 协议一律拒绝,且错误文案带统一提示。
        for (protocol, transport, security) in [
            ("trojan", "tcp", "tls"),
            ("hysteria", "hysteria", "tls"),
            ("shadowsocks", "tcp", ""),
            // VLESS 但走 Reality(security=reality)→ 不能过 CF。
            ("vless", "tcp", "reality"),
            // VLESS-WS 但未走 TLS → 不能过 CF。
            ("vless", "ws", "none"),
        ] {
            let err = validate_protocol_cdn_combination(protocol, transport, security, true, None)
                .expect_err(&format!("{protocol}/{transport}/{security} 不应允许过 CF"));
            let message = err.to_string();
            assert!(
                message.contains("该协议不能过 CF"),
                "错误文案应提示不能过 CF, got: {message}"
            );
        }
    }
