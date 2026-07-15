-- 默认订阅规则改为中国地区直连、其他流量走首个代理分组。
-- 只迁移仍为空规则的旧配置，管理员已经自定义的规则不覆盖。

WITH default_rules AS (
    SELECT jsonb_build_array(
        'DOMAIN-SUFFIX,local,DIRECT',
        'DOMAIN-SUFFIX,localhost,DIRECT',
        'DOMAIN,localhost,DIRECT',
        'IP-CIDR,127.0.0.0/8,DIRECT,no-resolve',
        'IP-CIDR,10.0.0.0/8,DIRECT,no-resolve',
        'IP-CIDR,172.16.0.0/12,DIRECT,no-resolve',
        'IP-CIDR,192.168.0.0/16,DIRECT,no-resolve',
        'IP-CIDR,100.64.0.0/10,DIRECT,no-resolve',
        'IP-CIDR,224.0.0.0/4,DIRECT,no-resolve',
        'IP-CIDR6,::1/128,DIRECT,no-resolve',
        'IP-CIDR6,fc00::/7,DIRECT,no-resolve',
        'IP-CIDR6,fe80::/10,DIRECT,no-resolve',
        'DOMAIN-SUFFIX,cn,DIRECT',
        'DOMAIN-KEYWORD,-cn,DIRECT',
        'DOMAIN-SUFFIX,qq.com,DIRECT',
        'DOMAIN-SUFFIX,weixin.qq.com,DIRECT',
        'DOMAIN-SUFFIX,gtimg.com,DIRECT',
        'DOMAIN-SUFFIX,baidu.com,DIRECT',
        'DOMAIN-SUFFIX,bdstatic.com,DIRECT',
        'DOMAIN-SUFFIX,taobao.com,DIRECT',
        'DOMAIN-SUFFIX,tmall.com,DIRECT',
        'DOMAIN-SUFFIX,alicdn.com,DIRECT',
        'DOMAIN-SUFFIX,alipay.com,DIRECT',
        'DOMAIN-SUFFIX,jd.com,DIRECT',
        'DOMAIN-SUFFIX,360buyimg.com,DIRECT',
        'DOMAIN-SUFFIX,bilibili.com,DIRECT',
        'DOMAIN-SUFFIX,bilivideo.com,DIRECT',
        'DOMAIN-SUFFIX,douyin.com,DIRECT',
        'DOMAIN-SUFFIX,byteimg.com,DIRECT',
        'DOMAIN-SUFFIX,ixigua.com,DIRECT',
        'DOMAIN-SUFFIX,163.com,DIRECT',
        'DOMAIN-SUFFIX,126.com,DIRECT',
        'DOMAIN-SUFFIX,netease.com,DIRECT',
        'DOMAIN-SUFFIX,meituan.com,DIRECT',
        'DOMAIN-SUFFIX,dianping.com,DIRECT',
        'DOMAIN-SUFFIX,amap.com,DIRECT',
        'DOMAIN-SUFFIX,autonavi.com,DIRECT',
        'DOMAIN-SUFFIX,mi.com,DIRECT',
        'DOMAIN-SUFFIX,xiaomi.com,DIRECT',
        'DOMAIN-SUFFIX,huawei.com,DIRECT',
        'DOMAIN-SUFFIX,zhihu.com,DIRECT',
        'DOMAIN-SUFFIX,zhimg.com,DIRECT',
        'DOMAIN-SUFFIX,weibo.com,DIRECT',
        'DOMAIN-SUFFIX,sina.com.cn,DIRECT',
        'DOMAIN-SUFFIX,csdn.net,DIRECT',
        'DOMAIN-SUFFIX,aliyun.com,DIRECT',
        'DOMAIN-SUFFIX,tencent.com,DIRECT',
        'DOMAIN-SUFFIX,qcloud.com,DIRECT',
        'GEOSITE,CN,DIRECT',
        'GEOIP,CN,DIRECT,no-resolve',
        'MATCH,PROXY'
    ) AS rules
)
UPDATE site_settings
SET setting_value = jsonb_set(
        jsonb_set(COALESCE(setting_value, '{}'::jsonb), '{rules}', default_rules.rules, true),
        '{default_rules}',
        default_rules.rules,
        true
    ),
    updated_at = now()
FROM default_rules
WHERE setting_key = 'subscription_config'
  AND CASE
          WHEN jsonb_typeof(setting_value->'rules') = 'array'
          THEN jsonb_array_length(setting_value->'rules')
          ELSE 0
      END = 0
  AND CASE
          WHEN jsonb_typeof(setting_value->'default_rules') = 'array'
          THEN jsonb_array_length(setting_value->'default_rules')
          ELSE 0
      END = 0;

UPDATE site_settings
SET setting_value = jsonb_set(
        setting_value,
        '{default_rules}',
        setting_value->'rules',
        true
    ),
    updated_at = now()
WHERE setting_key = 'subscription_config'
  AND CASE
          WHEN jsonb_typeof(setting_value->'rules') = 'array'
          THEN jsonb_array_length(setting_value->'rules')
          ELSE 0
      END > 0
  AND CASE
          WHEN jsonb_typeof(setting_value->'default_rules') = 'array'
          THEN jsonb_array_length(setting_value->'default_rules')
          ELSE 0
      END = 0;

UPDATE site_settings
SET setting_value = jsonb_set(
        setting_value,
        '{rules}',
        setting_value->'default_rules',
        true
    ),
    updated_at = now()
WHERE setting_key = 'subscription_config'
  AND CASE
          WHEN jsonb_typeof(setting_value->'rules') = 'array'
          THEN jsonb_array_length(setting_value->'rules')
          ELSE 0
      END = 0
  AND CASE
          WHEN jsonb_typeof(setting_value->'default_rules') = 'array'
          THEN jsonb_array_length(setting_value->'default_rules')
          ELSE 0
      END > 0;
