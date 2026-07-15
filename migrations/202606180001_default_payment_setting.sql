-- 支付配置默认值落库。
-- 仿照站点设置其它默认值，把 payment 默认结构写入 site_settings；
-- 已存在则不覆盖（ON CONFLICT DO NOTHING），避免回退管理员已保存的配置。
-- 敏感凭据字段默认空字符串，由后台保存时按 write-only 规则写入。

INSERT INTO site_settings (setting_key, setting_value, updated_at)
VALUES (
    'payment',
    '{
        "enabled": false,
        "default_channel": "",
        "providers": {
            "alipay": {
                "enabled": false,
                "environment": "sandbox",
                "app_id": "",
                "app_private_key": "",
                "alipay_public_key": "",
                "notify_url": ""
            },
            "epay": {
                "enabled": false,
                "api_url": "",
                "pid": "",
                "key": "",
                "notify_url": ""
            },
            "wechat": {
                "enabled": false,
                "mch_id": "",
                "app_id": "",
                "api_v3_key": "",
                "cert_serial_no": "",
                "private_key": "",
                "notify_url": ""
            }
        }
    }'::jsonb,
    now()
)
ON CONFLICT (setting_key) DO NOTHING;
