-- 订阅自动测速分组默认下线。
-- 旧版本把 auto_test_enabled 默认为 true，这里把既有配置收口为关闭；
-- 管理员后续仍可在后台订阅设置中手动重新开启。

UPDATE site_settings
SET setting_value = jsonb_set(
        COALESCE(setting_value, '{}'::jsonb),
        '{auto_test_enabled}',
        'false'::jsonb,
        true
    ),
    updated_at = now()
WHERE setting_key = 'subscription_config'
  AND COALESCE(setting_value->>'auto_test_enabled', 'true') = 'true';
