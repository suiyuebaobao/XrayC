-- 海外 AI 服务必须优先走代理。
-- 迁移只把缺失的 AI 规则前置到现有订阅规则中，不覆盖管理员已有的其他自定义规则。

WITH ai_rules(rule_order, rule) AS (
    VALUES
        (1, 'DOMAIN-SUFFIX,chatgpt.com,PROXY'),
        (2, 'DOMAIN-SUFFIX,chat.com,PROXY'),
        (3, 'DOMAIN-SUFFIX,openai.com,PROXY'),
        (4, 'DOMAIN-SUFFIX,openaiapi.com,PROXY'),
        (5, 'DOMAIN-SUFFIX,oaiusercontent.com,PROXY'),
        (6, 'DOMAIN-SUFFIX,oaistatic.com,PROXY'),
        (7, 'DOMAIN-SUFFIX,claude.ai,PROXY'),
        (8, 'DOMAIN-SUFFIX,anthropic.com,PROXY'),
        (9, 'DOMAIN-SUFFIX,grok.com,PROXY'),
        (10, 'DOMAIN-SUFFIX,x.ai,PROXY'),
        (11, 'DOMAIN-SUFFIX,gemini.google.com,PROXY'),
        (12, 'DOMAIN-SUFFIX,aistudio.google.com,PROXY'),
        (13, 'DOMAIN-SUFFIX,ai.google.dev,PROXY'),
        (14, 'DOMAIN-SUFFIX,generativelanguage.googleapis.com,PROXY'),
        (15, 'DOMAIN-SUFFIX,makersuite.google.com,PROXY'),
        (16, 'DOMAIN-SUFFIX,bard.google.com,PROXY'),
        (17, 'DOMAIN-SUFFIX,perplexity.ai,PROXY'),
        (18, 'DOMAIN-SUFFIX,poe.com,PROXY'),
        (19, 'DOMAIN-SUFFIX,cursor.com,PROXY'),
        (20, 'DOMAIN-SUFFIX,codeium.com,PROXY'),
        (21, 'DOMAIN-SUFFIX,windsurf.com,PROXY')
),
ai_json AS (
    SELECT jsonb_agg(to_jsonb(rule) ORDER BY rule_order) AS rules
    FROM ai_rules
),
subscription_settings AS (
    SELECT
        setting_key,
        setting_value,
        CASE
            WHEN jsonb_typeof(setting_value->'rules') = 'array'
            THEN setting_value->'rules'
            ELSE '[]'::jsonb
        END AS rules,
        CASE
            WHEN jsonb_typeof(setting_value->'default_rules') = 'array'
            THEN setting_value->'default_rules'
            ELSE '[]'::jsonb
        END AS default_rules
    FROM site_settings
    WHERE setting_key = 'subscription_config'
),
normalized AS (
    SELECT
        setting_key,
        setting_value,
        ai_json.rules || COALESCE(
            (
                SELECT jsonb_agg(rule_item.value)
                FROM jsonb_array_elements(subscription_settings.rules) AS rule_item(value)
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM ai_rules
                    WHERE ai_rules.rule = rule_item.value #>> '{}'
                )
            ),
            '[]'::jsonb
        ) AS rules,
        ai_json.rules || COALESCE(
            (
                SELECT jsonb_agg(rule_item.value)
                FROM jsonb_array_elements(subscription_settings.default_rules) AS rule_item(value)
                WHERE NOT EXISTS (
                    SELECT 1
                    FROM ai_rules
                    WHERE ai_rules.rule = rule_item.value #>> '{}'
                )
            ),
            '[]'::jsonb
        ) AS default_rules
    FROM subscription_settings
    CROSS JOIN ai_json
)
UPDATE site_settings
SET setting_value = jsonb_set(
        jsonb_set(
            COALESCE(normalized.setting_value, '{}'::jsonb),
            '{rules}',
            normalized.rules,
            true
        ),
        '{default_rules}',
        normalized.default_rules,
        true
    ),
    updated_at = now()
FROM normalized
WHERE site_settings.setting_key = normalized.setting_key;
