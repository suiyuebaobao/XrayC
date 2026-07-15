//! 支付宝当面付(扫码 / `alipay.trade.precreate`)接入 provider。
//! 负责 RSA2(SHA256withRSA)请求签名、异步通知验签、网关调用。
//! 配置(APPID/应用私钥/支付宝公钥/网关/notify_url)由环境变量明文注入。
//! 请求签名待签串含 sign_type、排除 sign;异步通知验签排除 sign 与 sign_type。
//! 沙箱网关默认 openapi-sandbox.dl.alipaydev.com,生产由 env 覆盖。
//! 本模块只做协议与签名,订单落库/开通续期仍走既有 PgStore 闭环。
//! 私钥/密钥材料只在内存持有,不落库、不入日志、不进审计。
//! precreate 返回 qr_code 字符串,由前端渲染成二维码供买家扫码。
//! query 用于回调丢失时主动对账兜底。
//! 本文件前十行中文注释满足仓库约束。

use std::collections::BTreeMap;

use base64::Engine;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::DecodePublicKey;
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};

use crate::payment::{yuan_from_cents, yuan_to_cents, PaidNotice, PayResult};

/// 沙箱网关(新版;旧版 openapi.alipaydev.com 已停用)。
const SANDBOX_GATEWAY: &str = "https://openapi-sandbox.dl.alipaydev.com/gateway.do";
/// 生产网关(`environment=production` 时用)。
const PRODUCTION_GATEWAY: &str = "https://openapi.alipay.com/gateway.do";

/// 支付宝当面付配置 + 密钥句柄。
pub(crate) struct AlipayConfig {
    app_id: String,
    private_key: RsaPrivateKey,
    alipay_public_key: RsaPublicKey,
    gateway: String,
    notify_url: String,
}

impl AlipayConfig {
    /// 显式构造:私钥按 PKCS1 DER、支付宝公钥按 SPKI DER 解析(均为单行 base64)。
    pub(crate) fn new(
        app_id: &str,
        private_key_b64: &str,
        public_key_b64: &str,
        gateway: String,
        notify_url: String,
    ) -> Result<Self, String> {
        let private_der = base64::engine::general_purpose::STANDARD
            .decode(private_key_b64.trim())
            .map_err(|error| format!("应用私钥 base64 无效: {error}"))?;
        let private_key = RsaPrivateKey::from_pkcs1_der(&private_der)
            .map_err(|error| format!("应用私钥 PKCS1 解析失败: {error}"))?;
        let public_der = base64::engine::general_purpose::STANDARD
            .decode(public_key_b64.trim())
            .map_err(|error| format!("支付宝公钥 base64 无效: {error}"))?;
        let alipay_public_key = RsaPublicKey::from_public_key_der(&public_der)
            .map_err(|error| format!("支付宝公钥 SPKI 解析失败: {error}"))?;
        Ok(Self {
            app_id: app_id.to_string(),
            private_key,
            alipay_public_key,
            gateway,
            notify_url,
        })
    }

    /// 请求待签串:按 key 升序、排除 sign 与空值、`k=v` 以 `&` 连接(原始值,不做 URL 编码)。
    fn request_sign_source(params: &BTreeMap<String, String>) -> String {
        params
            .iter()
            .filter(|(key, value)| key.as_str() != "sign" && !value.is_empty())
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// 异步通知验签待签串:在请求规则基础上**额外排除 sign_type**(支付宝异步通知约定)。
    fn notify_sign_source(params: &BTreeMap<String, String>) -> String {
        params
            .iter()
            .filter(|(key, value)| {
                key.as_str() != "sign" && key.as_str() != "sign_type" && !value.is_empty()
            })
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// 用应用私钥对原始串做 RSA2 签名,返回 base64。
    fn sign_raw(&self, message: &str) -> Result<String, String> {
        let signing_key = SigningKey::<Sha256>::new(self.private_key.clone());
        let signature = signing_key
            .try_sign(message.as_bytes())
            .map_err(|error| format!("RSA2 签名失败: {error}"))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()))
    }

    /// 用支付宝公钥验签原始串;任何解析/验证失败都返回 false。
    fn verify_raw(&self, message: &str, sign_b64: &str) -> bool {
        let Ok(signature_bytes) = base64::engine::general_purpose::STANDARD.decode(sign_b64) else {
            return false;
        };
        let Ok(signature) = Signature::try_from(signature_bytes.as_slice()) else {
            return false;
        };
        let verifying_key = VerifyingKey::<Sha256>::new(self.alipay_public_key.clone());
        verifying_key.verify(message.as_bytes(), &signature).is_ok()
    }

    /// 给请求参数签名(待签串排除 sign)。
    pub(crate) fn sign(&self, params: &BTreeMap<String, String>) -> Result<String, String> {
        self.sign_raw(&Self::request_sign_source(params))
    }

    /// 校验异步通知参数签名(待签串排除 sign 与 sign_type)。
    pub(crate) fn verify_notify(&self, params: &BTreeMap<String, String>) -> bool {
        let Some(sign) = params.get("sign") else {
            return false;
        };
        self.verify_raw(&Self::notify_sign_source(params), sign)
    }

    /// 配置的应用 APPID,用于异步通知来源校验(防别家商户通知串入)。
    pub(crate) fn app_id(&self) -> &str {
        &self.app_id
    }

    /// 组装公共参数(sign 未填),调用方补 method/biz_content 后再签名。
    fn base_params(&self, method: &str, biz_content: String) -> BTreeMap<String, String> {
        let mut params = BTreeMap::new();
        params.insert("app_id".to_string(), self.app_id.clone());
        params.insert("method".to_string(), method.to_string());
        params.insert("format".to_string(), "JSON".to_string());
        params.insert("charset".to_string(), "utf-8".to_string());
        params.insert("sign_type".to_string(), "RSA2".to_string());
        params.insert("timestamp".to_string(), beijing_timestamp());
        params.insert("version".to_string(), "1.0".to_string());
        if !self.notify_url.is_empty() {
            params.insert("notify_url".to_string(), self.notify_url.clone());
        }
        params.insert("biz_content".to_string(), biz_content);
        params
    }

    /// 调 `alipay.trade.precreate` 预下单,成功返回二维码链接 `qr_code`。
    pub(crate) async fn precreate(
        &self,
        out_trade_no: &str,
        total_amount: &str,
        subject: &str,
    ) -> Result<String, String> {
        let biz = serde_json::json!({
            "out_trade_no": out_trade_no,
            "total_amount": total_amount,
            "subject": subject,
        });
        let node = self
            .invoke(
                "alipay.trade.precreate",
                biz.to_string(),
                "alipay_trade_precreate_response",
            )
            .await?;
        node.get("qr_code")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "precreate 响应缺少 qr_code".to_string())
    }

    /// 调 `alipay.trade.query` 查询交易状态,返回 `trade_status`(如 TRADE_SUCCESS)。
    /// 保留作异步通知丢失时的对账兜底入口(后续可接订单状态查询端点)。
    #[allow(dead_code)]
    pub(crate) async fn query_trade_status(&self, out_trade_no: &str) -> Result<String, String> {
        let biz = serde_json::json!({ "out_trade_no": out_trade_no });
        let node = self
            .invoke(
                "alipay.trade.query",
                biz.to_string(),
                "alipay_trade_query_response",
            )
            .await?;
        node.get("trade_status")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "query 响应缺少 trade_status".to_string())
    }

    /// 通用网关调用:签名 → 表单 POST → 取出 `<response_key>` 节点并校验 code=10000。
    async fn invoke(
        &self,
        method: &str,
        biz_content: String,
        response_key: &str,
    ) -> Result<serde_json::Value, String> {
        let mut params = self.base_params(method, biz_content);
        let sign = self.sign(&params)?;
        params.insert("sign".to_string(), sign);
        // 必须显式声明 charset=utf-8:reqwest 的 .form() 不带 charset,支付宝会按 GBK 解码
        // 含中文的 biz_content,导致重建的待签串字节与本地签名不一致 → "验签出错"(code 40002)。
        let body = {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (key, value) in &params {
                serializer.append_pair(key.as_str(), value.as_str());
            }
            serializer.finish()
        };
        let response = reqwest::Client::new()
            .post(&self.gateway)
            .header(
                "content-type",
                "application/x-www-form-urlencoded;charset=utf-8",
            )
            .body(body)
            .send()
            .await
            .map_err(|error| format!("网关请求失败: {error}"))?;
        let text = response
            .text()
            .await
            .map_err(|error| format!("读取网关响应失败: {error}"))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| format!("网关响应非 JSON: {error}"))?;
        let node = value
            .get(response_key)
            .cloned()
            .ok_or_else(|| format!("网关响应缺少 {response_key}"))?;
        let code = node
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if code != "10000" {
            return Err(format!(
                "{method} 失败 code={code} msg={} sub_msg={}",
                node.get("msg")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
                node.get("sub_msg")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            ));
        }
        Ok(node)
    }
}

/// 北京时间(UTC+8)时间戳,格式 `YYYY-MM-DD HH:MM:SS`(支付宝要求商户本地时间)。
fn beijing_timestamp() -> String {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).expect("+0800 偏移合法");
    chrono::Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

impl AlipayConfig {
    /// 从 DB `providers.alipay` 段装配;必填项缺失返回 None;`environment` 决定网关。
    pub(crate) fn from_settings(provider: &serde_json::Value) -> Option<Self> {
        let text = |key: &str| {
            provider
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let app_id = text("app_id");
        let private_key = text("app_private_key");
        let public_key = text("alipay_public_key");
        if app_id.is_empty() || private_key.is_empty() || public_key.is_empty() {
            return None;
        }
        let gateway = if text("environment") == "production" {
            PRODUCTION_GATEWAY
        } else {
            SANDBOX_GATEWAY
        }
        .to_string();
        Self::new(
            &app_id,
            &private_key,
            &public_key,
            gateway,
            text("notify_url"),
        )
        .ok()
    }

    /// 统一下单接口:当面付走 precreate 出二维码。
    pub(crate) async fn create_payment(
        &self,
        out_trade_no: &str,
        amount_cents: i64,
        subject: &str,
    ) -> Result<PayResult, String> {
        let qr_code = self
            .precreate(out_trade_no, &yuan_from_cents(amount_cents), subject)
            .await?;
        Ok(PayResult {
            qr_code: Some(qr_code),
            pay_url: None,
        })
    }

    /// 异步通知验签 + app_id/成功态校验后抽出已付信息(渠道无关)。
    pub(crate) fn verify_notify_notice(
        &self,
        params: &BTreeMap<String, String>,
    ) -> Option<PaidNotice> {
        if !self.verify_notify(params) {
            return None;
        }
        if params.get("app_id").map(String::as_str) != Some(self.app_id()) {
            return None;
        }
        let status = params
            .get("trade_status")
            .map(String::as_str)
            .unwrap_or_default();
        if status != "TRADE_SUCCESS" && status != "TRADE_FINISHED" {
            return None;
        }
        Some(PaidNotice {
            out_trade_no: params.get("out_trade_no")?.clone(),
            channel_tx_id: params.get("trade_no").cloned().unwrap_or_default(),
            amount_cents: yuan_to_cents(
                params
                    .get("total_amount")
                    .map(String::as_str)
                    .unwrap_or_default(),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::EncodePublicKey;

    /// 用运行时生成的密钥对构造一个 self-consistent 配置(私钥 ↔ 支付宝公钥同源),
    /// 以便单测覆盖 sign/verify 机制本身(真实互通由沙箱联调验证)。
    fn test_config() -> AlipayConfig {
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("生成测试私钥");
        let public_key = RsaPublicKey::from(&private_key);
        let private_b64 = base64::engine::general_purpose::STANDARD
            .encode(private_key.to_pkcs1_der().unwrap().as_bytes());
        let public_b64 = base64::engine::general_purpose::STANDARD
            .encode(public_key.to_public_key_der().unwrap().as_bytes());
        AlipayConfig::new(
            "9021000000000000",
            &private_b64,
            &public_b64,
            SANDBOX_GATEWAY.to_string(),
            String::new(),
        )
        .expect("测试配置应可构造")
    }

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_request_sign_source_is_sorted_excludes_sign_and_empty() {
        let p = params(&[
            ("app_id", "2021"),
            ("method", "alipay.trade.precreate"),
            ("sign", "SHOULD_BE_EXCLUDED"),
            ("notify_url", ""),
            ("charset", "utf-8"),
        ]);
        // 按 key 升序:app_id, charset, method;排除 sign 与空 notify_url。
        assert_eq!(
            AlipayConfig::request_sign_source(&p),
            "app_id=2021&charset=utf-8&method=alipay.trade.precreate"
        );
    }

    #[test]
    fn test_notify_sign_source_excludes_sign_and_sign_type() {
        let p = params(&[
            ("out_trade_no", "ORD1"),
            ("trade_status", "TRADE_SUCCESS"),
            ("sign", "X"),
            ("sign_type", "RSA2"),
        ]);
        assert_eq!(
            AlipayConfig::notify_sign_source(&p),
            "out_trade_no=ORD1&trade_status=TRADE_SUCCESS"
        );
    }

    #[test]
    fn test_sign_then_verify_roundtrip_and_reject_tamper() {
        let config = test_config();
        let message = "out_trade_no=ORD1&total_amount=0.01";
        let sign = config.sign_raw(message).expect("签名成功");
        assert!(config.verify_raw(message, &sign), "同源公钥应验签通过");
        assert!(
            !config.verify_raw("out_trade_no=ORD1&total_amount=9.99", &sign),
            "篡改报文后验签必须失败"
        );
        assert!(
            !config.verify_raw(message, "bm90LWEtc2ln"),
            "无效签名必须失败"
        );
    }

    #[test]
    fn test_verify_notify_uses_notify_source() {
        let config = test_config();
        let mut p = params(&[
            ("out_trade_no", "ORD9"),
            ("trade_status", "TRADE_SUCCESS"),
            ("sign_type", "RSA2"),
        ]);
        // 用同源私钥按"通知待签串"签名,再走 verify_notify 应通过。
        let sign = config
            .sign_raw(&AlipayConfig::notify_sign_source(&p))
            .expect("签名成功");
        p.insert("sign".to_string(), sign);
        assert!(config.verify_notify(&p), "合法通知应验签通过");
        p.insert("trade_status".to_string(), "WAIT_BUYER_PAY".to_string());
        assert!(!config.verify_notify(&p), "篡改通知字段后必须验签失败");
    }
}
