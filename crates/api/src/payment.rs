//! 多渠道支付的共享类型 + 渠道枚举分发 + 工厂。
//! `PayResult` 是下单产物:二维码链接(扫码)或跳转 URL(网页收银台),二选一。
//! `PaidNotice` 是某渠道异步通知验签通过后抽出的"已付"信息,供统一入账闭环使用。
//! `PaymentChannel` 枚举按渠道分发到各 provider(支付宝/易支付),屏蔽协议差异;微信暂不实现→工厂返回 None。
//! `payment_channel(channel, settings)` 从 DB 支付设置按渠道装配,渠道未启用/配置缺失返回 None。
//! 金额统一用分(i64),元字符串换算 helper 在此集中,供各 provider 复用。
//! 本文件不持有密钥外的网络逻辑;具体协议在 alipay/epay provider 内。
//! 字段命名与订单入账(`apply_payment_callback_json`)对齐。
//! 类型/函数保持 crate 内可见。
//! 本头部满足前十行中文注释约束。

use std::collections::BTreeMap;

use crate::alipay::AlipayConfig;
use crate::epay::EpayConfig;

/// 下单产物:二维码链接 或 跳转收银台 URL。
pub(crate) struct PayResult {
    pub(crate) qr_code: Option<String>,
    pub(crate) pay_url: Option<String>,
}

/// 异步通知验签通过后的已付信息(渠道无关)。
pub(crate) struct PaidNotice {
    pub(crate) out_trade_no: String,
    pub(crate) channel_tx_id: String,
    pub(crate) amount_cents: i64,
}

/// 已装配的支付渠道(按渠道分发)。
/// 支付宝变体持有两把 RSA 密钥,体积远大于易支付,Box 化避免枚举整体膨胀。
pub(crate) enum PaymentChannel {
    Alipay(Box<AlipayConfig>),
    Epay(EpayConfig),
}

impl PaymentChannel {
    pub(crate) async fn create_payment(
        &self,
        out_trade_no: &str,
        amount_cents: i64,
        subject: &str,
    ) -> Result<PayResult, String> {
        match self {
            PaymentChannel::Alipay(config) => {
                config
                    .create_payment(out_trade_no, amount_cents, subject)
                    .await
            }
            PaymentChannel::Epay(config) => {
                config
                    .create_payment(out_trade_no, amount_cents, subject)
                    .await
            }
        }
    }

    pub(crate) fn verify_notify(&self, params: &BTreeMap<String, String>) -> Option<PaidNotice> {
        match self {
            PaymentChannel::Alipay(config) => config.verify_notify_notice(params),
            PaymentChannel::Epay(config) => config.verify_notify(params),
        }
    }
}

/// 从 DB 支付设置按渠道装配 provider;渠道未启用或必填项缺失返回 None(微信暂未实现也返回 None)。
pub(crate) fn payment_channel(
    channel: &str,
    settings: &serde_json::Value,
) -> Option<PaymentChannel> {
    let provider = settings.get("providers")?.get(channel)?;
    if !provider
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    match channel {
        "alipay" => AlipayConfig::from_settings(provider)
            .map(|config| PaymentChannel::Alipay(Box::new(config))),
        "epay" => EpayConfig::from_settings(provider).map(PaymentChannel::Epay),
        _ => None,
    }
}

/// 分 → 元字符串(两位小数),如 1 → "0.01"。
pub(crate) fn yuan_from_cents(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, (cents % 100).abs())
}

/// 元字符串 → 分,只取两位小数,如 "0.01" → 1。
pub(crate) fn yuan_to_cents(yuan: &str) -> i64 {
    let trimmed = yuan.trim();
    let (integer_part, fraction_part) = trimmed.split_once('.').unwrap_or((trimmed, ""));
    let integer: i64 = integer_part.parse().unwrap_or(0);
    let mut fraction = fraction_part.to_string();
    while fraction.len() < 2 {
        fraction.push('0');
    }
    fraction.truncate(2);
    integer * 100 + fraction.parse::<i64>().unwrap_or(0)
}
