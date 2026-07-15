-- 下线官方客户端专属订阅/设备绑定链路。
-- 当前只保留传统 /sub/{token} 和用户订阅接口；历史迁移保留，本迁移清理运行表与配置。
DROP TABLE IF EXISTS client_devices;

DELETE FROM site_settings
 WHERE setting_key = 'client_config';
