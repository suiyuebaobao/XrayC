-- 补齐 V2 第一版用户、订单、兑换码和支付闭环所需的业务字段。
-- 这些字段保持向后兼容，不破坏已有 demo 数据和接入控制面表结构。

ALTER TABLE plans
    ADD COLUMN IF NOT EXISTS price_cents BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'USDT',
    ADD COLUMN IF NOT EXISTS duration_days INTEGER NOT NULL DEFAULT 30,
    ADD COLUMN IF NOT EXISTS sort_weight INTEGER NOT NULL DEFAULT 100,
    ADD COLUMN IF NOT EXISTS is_deleted BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_no TEXT NOT NULL UNIQUE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plan_id UUID NOT NULL REFERENCES plans(id),
    amount_cents BIGINT NOT NULL DEFAULT 0,
    currency TEXT NOT NULL DEFAULT 'USDT',
    status TEXT NOT NULL DEFAULT 'pending',
    payment_address TEXT NOT NULL DEFAULT '',
    expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '30 minutes'),
    paid_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS payment_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    tx_id TEXT NOT NULL UNIQUE,
    amount_cents BIGINT NOT NULL DEFAULT 0,
    currency TEXT NOT NULL DEFAULT 'USDT',
    raw_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    confirmed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS redeem_codes (
    code TEXT PRIMARY KEY,
    plan_id UUID NOT NULL REFERENCES plans(id),
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    traffic_bytes BIGINT NULL,
    duration_days INTEGER NULL,
    is_used BOOLEAN NOT NULL DEFAULT FALSE,
    used_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    used_at TIMESTAMPTZ NULL,
    expires_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_orders_user_created ON orders(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_orders_status_created ON orders(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_redeem_codes_plan ON redeem_codes(plan_id);
CREATE INDEX IF NOT EXISTS idx_redeem_codes_used ON redeem_codes(is_used, created_at DESC);
