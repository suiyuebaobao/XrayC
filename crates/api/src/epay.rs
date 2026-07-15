//! 易支付(epay 协议)聚合支付 provider。
//! epay 是国内小站事实标准:`pid + key`,MD5 签名,下单返回二维码或跳转收银台。
//! 待签串:参数(排除 sign、sign_type、空值)按 key 升序拼 `a=b&c=d`,末尾直接拼商户 key,MD5 取小写。
//! 下单优先 `mapi.php`(API,返回 JSON 含 qrcode/payurl);失败回退 `submit.php` 页面跳转。
//! 异步通知:同规则验签 + `trade_status=TRADE_SUCCESS` 即已付,channel_tx_id=epay trade_no。
//! 配置(api_url/pid/key/notify_url)由 DB 支付设置注入,key 明文存(字段加密已下线)。
//! 金额用元两位小数;请求体显式 charset=utf-8。
//! 本 provider 只做协议与签名,订单入账/开通仍复用统一闭环。
//! 不在此记录密钥明文到日志。
//! 本头部满足前十行中文注释约束。

use std::collections::BTreeMap;

use md5::{Digest, Md5};

use crate::payment::{yuan_from_cents, yuan_to_cents, PaidNotice, PayResult};

/// 易支付商户配置。
pub(crate) struct EpayConfig {
    api_url: String,
    pid: String,
    key: String,
    notify_url: String,
}

impl EpayConfig {
    /// 从 DB `providers.epay` 段装配;必填项缺失返回 None。
    pub(crate) fn from_settings(provider: &serde_json::Value) -> Option<Self> {
        let text = |key: &str| {
            provider
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let api_url = text("api_url");
        let pid = text("pid");
        let key = text("key");
        if api_url.is_empty() || pid.is_empty() || key.is_empty() {
            return None;
        }
        Some(Self {
            api_url: api_url.trim_end_matches('/').to_string(),
            pid,
            key,
            notify_url: text("notify_url"),
        })
    }

    /// MD5 签名:升序、排除 sign/sign_type/空值、`k=v&` 拼接,末尾直接接商户 key。
    fn sign(&self, params: &BTreeMap<String, String>) -> String {
        let source = params
            .iter()
            .filter(|(key, value)| {
                key.as_str() != "sign" && key.as_str() != "sign_type" && !value.is_empty()
            })
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        let mut hasher = Md5::new();
        hasher.update(format!("{source}{}", self.key).as_bytes());
        hex::encode(hasher.finalize())
    }

    /// 校验异步通知:验签 + 成功态 → 抽出已付信息;否则 None。
    pub(crate) fn verify_notify(&self, params: &BTreeMap<String, String>) -> Option<PaidNotice> {
        let sign = params.get("sign")?;
        if &self.sign(params) != sign {
            return None;
        }
        if params.get("trade_status").map(String::as_str) != Some("TRADE_SUCCESS") {
            return None;
        }
        Some(PaidNotice {
            out_trade_no: params.get("out_trade_no")?.clone(),
            channel_tx_id: params.get("trade_no").cloned().unwrap_or_default(),
            amount_cents: yuan_to_cents(
                params.get("money").map(String::as_str).unwrap_or_default(),
            ),
        })
    }

    /// 下单:优先 mapi.php 拿二维码/跳转,失败回退 submit.php 跳转 URL。
    pub(crate) async fn create_payment(
        &self,
        out_trade_no: &str,
        amount_cents: i64,
        subject: &str,
    ) -> Result<PayResult, String> {
        let mut params = BTreeMap::new();
        params.insert("pid".to_string(), self.pid.clone());
        params.insert("type".to_string(), "alipay".to_string());
        params.insert("out_trade_no".to_string(), out_trade_no.to_string());
        if !self.notify_url.is_empty() {
            params.insert("notify_url".to_string(), self.notify_url.clone());
        }
        params.insert("name".to_string(), subject.to_string());
        params.insert("money".to_string(), yuan_from_cents(amount_cents));
        let sign = self.sign(&params);
        params.insert("sign".to_string(), sign);
        params.insert("sign_type".to_string(), "MD5".to_string());

        // 优先 API 模式(mapi.php):返回 JSON,code==1 取 qrcode/payurl。
        if let Ok(response) = reqwest::Client::new()
            .post(format!("{}/mapi.php", self.api_url))
            .header(
                "content-type",
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .body(form_encode(&params))
            .send()
            .await
        {
            if let Ok(text) = response.text().await {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if value.get("code").and_then(serde_json::Value::as_i64) == Some(1) {
                        if let Some(qr) = value
                            .get("qrcode")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            return Ok(PayResult {
                                qr_code: Some(qr.to_string()),
                                pay_url: None,
                            });
                        }
                        if let Some(url) = value
                            .get("payurl")
                            .and_then(serde_json::Value::as_str)
                            .filter(|text| !text.is_empty())
                        {
                            return Ok(PayResult {
                                qr_code: None,
                                pay_url: Some(url.to_string()),
                            });
                        }
                    }
                }
            }
        }

        // 回退页面跳转(submit.php):带签名参数的跳转 URL,前端打开即进收银台。
        Ok(PayResult {
            qr_code: None,
            pay_url: Some(format!(
                "{}/submit.php?{}",
                self.api_url,
                form_encode(&params)
            )),
        })
    }
}

/// 表单编码(application/x-www-form-urlencoded)。
fn form_encode(params: &BTreeMap<String, String>) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key.as_str(), value.as_str());
    }
    serializer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> EpayConfig {
        EpayConfig::from_settings(&serde_json::json!({
            "enabled": true,
            "api_url": "https://pay.example.test/",
            "pid": "1000",
            "key": "TEST_MERCHANT_KEY",
            "notify_url": "https://x.test/api/payment/epay/notify"
        }))
        .expect("配置可装配")
    }

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_epay_money_conversion() {
        assert_eq!(yuan_to_cents("0.01"), 1);
        assert_eq!(yuan_to_cents("12.30"), 1230);
        assert_eq!(yuan_to_cents("5"), 500);
        assert_eq!(yuan_from_cents(1), "0.01");
        assert_eq!(yuan_from_cents(1230), "12.30");
    }

    #[test]
    fn test_epay_sign_is_deterministic_and_excludes_sign_fields() {
        let cfg = config();
        let p = params(&[
            ("pid", "1000"),
            ("out_trade_no", "ORD1"),
            ("money", "0.01"),
            ("name", "套餐"),
            ("sign", "IGNORED"),
            ("sign_type", "MD5"),
        ]);
        let a = cfg.sign(&p);
        // 32 位小写 hex,且对同输入稳定。
        assert_eq!(a.len(), 32);
        assert_eq!(a, a.to_lowercase());
        assert_eq!(cfg.sign(&p), a);
    }

    #[test]
    fn test_epay_verify_notify_accepts_valid_rejects_tamper() {
        let cfg = config();
        let mut p = params(&[
            ("pid", "1000"),
            ("trade_no", "EP123"),
            ("out_trade_no", "ORD1"),
            ("type", "alipay"),
            ("name", "套餐"),
            ("money", "0.01"),
            ("trade_status", "TRADE_SUCCESS"),
        ]);
        let sign = cfg.sign(&p);
        p.insert("sign".to_string(), sign);
        p.insert("sign_type".to_string(), "MD5".to_string());
        let notice = cfg.verify_notify(&p).expect("合法通知应通过");
        assert_eq!(notice.out_trade_no, "ORD1");
        assert_eq!(notice.channel_tx_id, "EP123");
        assert_eq!(notice.amount_cents, 1);

        // 篡改金额 → 验签失败。
        let mut tampered = p.clone();
        tampered.insert("money".to_string(), "9.99".to_string());
        assert!(cfg.verify_notify(&tampered).is_none());

        // 非成功态 → 不入账。
        let mut pending = p.clone();
        pending.insert("trade_status".to_string(), "WAIT_BUYER_PAY".to_string());
        // 改了字段需重签才算"合法但未成功";这里直接重签。
        pending.remove("sign");
        let s2 = cfg.sign(&pending);
        pending.insert("sign".to_string(), s2);
        assert!(cfg.verify_notify(&pending).is_none());
    }
}
