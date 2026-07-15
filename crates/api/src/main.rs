//! 本文件是 XrayC API 进程入口。
//! 启动 Axum 路由，提供 REST API 和 Clash/mihomo 订阅下载能力。

use std::net::SocketAddr;
use xrayc_api::{app, AppState};
use xrayc_db::PgStore;

const DEFAULT_JWT_SECRET: &str = "dev-jwt-secret-change-me";
const DEFAULT_PAYMENT_RECEIVE_ADDRESS: &str = "xrayc-demo-payment-address";
const PROD_MIN_ACCESS_TOKEN_TTL_SECONDS: i64 = 60;
const PROD_MAX_ACCESS_TOKEN_TTL_SECONDS: i64 = 30 * 60;
const PROD_MIN_REFRESH_TOKEN_TTL_SECONDS: i64 = 60 * 60;
const PROD_MAX_REFRESH_TOKEN_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeMode {
    Development,
    Production,
}

impl RuntimeMode {
    fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "xrayc_api=info,tower_http=info".into()),
        )
        .init();

    let runtime_mode = runtime_mode()?;
    let production = runtime_mode.is_production();
    if production {
        validate_production_env()?;
    }

    let addr: SocketAddr = std::env::var("API_BIND")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()?;
    let state = match std::env::var("DATABASE_URL") {
        Ok(database_url) if !database_url.trim().is_empty() => {
            let pg = PgStore::connect(&database_url).await?;
            let migrations_dir =
                std::env::var("MIGRATIONS_DIR").unwrap_or_else(|_| "migrations".to_string());
            pg.migrate(&migrations_dir).await?;
            // install 自带管理员:设了 XRAYC_BOOTSTRAP_ADMIN_PASSWORD 就幂等建账号 admin。
            // 与 SEED_DEMO_DATA 无关、生产模式也走,故装完即可用 admin 登录后台。
            if let Ok(bootstrap_pw) = std::env::var("XRAYC_BOOTSTRAP_ADMIN_PASSWORD") {
                if !bootstrap_pw.trim().is_empty() {
                    match pg.bootstrap_admin(&bootstrap_pw).await {
                        Ok(true) => tracing::info!("已创建初始管理员账号 admin"),
                        Ok(false) => tracing::info!("已存在管理员,跳过 bootstrap"),
                        Err(e) => tracing::warn!("bootstrap 管理员失败: {e}"),
                    }
                }
            }
            let seed_default = if production { "false" } else { "true" };
            let will_seed = std::env::var("SEED_DEMO_DATA")
                .unwrap_or_else(|_| seed_default.to_string())
                != "false";
            if will_seed {
                pg.seed_demo_data().await?;
            } else {
                // 不播 demo 时确保有默认基础套餐,否则用户注册/worker maintenance 报 DefaultPlanNotFound。
                match pg.bootstrap_default_plan().await {
                    Ok(true) => tracing::info!("已创建默认基础套餐"),
                    Ok(false) => {}
                    Err(e) => tracing::warn!("bootstrap 默认套餐失败: {e}"),
                }
            }
            tracing::info!("postgresql store initialized");
            AppState::with_pg(pg)
        }
        _ => {
            if production {
                anyhow::bail!("DATABASE_URL must be set in production");
            }
            tracing::warn!("DATABASE_URL is not set; using in-memory store");
            AppState::default()
        }
    };
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "starting xrayc api");
    axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn runtime_mode() -> anyhow::Result<RuntimeMode> {
    let value = ["XRAYC_ENV", "APP_ENV", "RUST_ENV"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.trim().is_empty());
    parse_runtime_mode(value.as_deref())
}

fn parse_runtime_mode(value: Option<&str>) -> anyhow::Result<RuntimeMode> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        anyhow::bail!("XRAYC_ENV must be explicitly set to development or production");
    };
    match value.to_ascii_lowercase().as_str() {
        "development" | "dev" | "local" => Ok(RuntimeMode::Development),
        "production" | "prod" => Ok(RuntimeMode::Production),
        _ => anyhow::bail!("XRAYC_ENV must be development or production"),
    }
}

fn validate_production_env() -> anyhow::Result<()> {
    let database_url = require_non_empty("DATABASE_URL")?;
    if database_url.contains("change-me") {
        anyhow::bail!("DATABASE_URL must not use the development placeholder in production");
    }
    let jwt_secret = require_non_empty("JWT_SECRET")?;
    if jwt_secret.len() < 32 || jwt_secret == DEFAULT_JWT_SECRET {
        anyhow::bail!("JWT_SECRET must be a strong production secret");
    }
    if production_payment_enabled()? {
        validate_production_payment_env()?;
    }
    let access_token_ttl = std::env::var("JWT_EXPIRES_IN").ok();
    let refresh_token_ttl = std::env::var("JWT_REFRESH_EXPIRES_IN").ok();
    validate_production_ttl_values(access_token_ttl.as_deref(), refresh_token_ttl.as_deref())?;
    if parse_bool_flag(
        &std::env::var("XRAYC_REFRESH_COOKIE_SECURE").unwrap_or_else(|_| "true".to_string()),
    ) == Some(false)
    {
        anyhow::bail!("XRAYC_REFRESH_COOKIE_SECURE must not be false in production");
    }
    if std::env::var("SEED_DEMO_DATA").unwrap_or_else(|_| "false".to_string()) != "false" {
        anyhow::bail!("SEED_DEMO_DATA must be false in production");
    }
    Ok(())
}

fn require_non_empty(name: &str) -> anyhow::Result<String> {
    let value = std::env::var(name).unwrap_or_default();
    if value.trim().is_empty() {
        anyhow::bail!("{name} must be set in production");
    }
    Ok(value)
}

fn production_payment_enabled() -> anyhow::Result<bool> {
    let value = std::env::var("XRAYC_PAYMENT_ENABLED").unwrap_or_else(|_| "false".to_string());
    parse_bool_flag(&value)
        .ok_or_else(|| anyhow::anyhow!("XRAYC_PAYMENT_ENABLED must be true or false"))
}

fn validate_production_payment_env() -> anyhow::Result<()> {
    let payment_secret = require_non_empty("PAYMENT_CALLBACK_SECRET")?;
    let payment_address = require_non_empty("PAYMENT_RECEIVE_ADDRESS")?;
    validate_production_payment_values(&payment_secret, &payment_address)
}

fn validate_production_payment_values(
    payment_secret: &str,
    payment_address: &str,
) -> anyhow::Result<()> {
    if payment_secret.len() < 32 || payment_secret.contains("change-me") {
        anyhow::bail!("PAYMENT_CALLBACK_SECRET must be a strong production secret");
    }
    if payment_address == DEFAULT_PAYMENT_RECEIVE_ADDRESS || payment_address.contains("change-me") {
        anyhow::bail!("PAYMENT_RECEIVE_ADDRESS must not use the development placeholder");
    }
    Ok(())
}

fn validate_production_ttl_values(
    access_token_ttl: Option<&str>,
    refresh_token_ttl: Option<&str>,
) -> anyhow::Result<()> {
    let access_seconds = parse_required_duration_seconds("JWT_EXPIRES_IN", access_token_ttl)?;
    if !(PROD_MIN_ACCESS_TOKEN_TTL_SECONDS..=PROD_MAX_ACCESS_TOKEN_TTL_SECONDS)
        .contains(&access_seconds)
    {
        anyhow::bail!("JWT_EXPIRES_IN must be between 60s and 30m in production");
    }

    let refresh_seconds =
        parse_required_duration_seconds("JWT_REFRESH_EXPIRES_IN", refresh_token_ttl)?;
    if !(PROD_MIN_REFRESH_TOKEN_TTL_SECONDS..=PROD_MAX_REFRESH_TOKEN_TTL_SECONDS)
        .contains(&refresh_seconds)
    {
        anyhow::bail!("JWT_REFRESH_EXPIRES_IN must be between 1h and 30d in production");
    }
    Ok(())
}

fn parse_required_duration_seconds(name: &str, value: Option<&str>) -> anyhow::Result<i64> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        anyhow::bail!("{name} must be set in production");
    };
    parse_duration_seconds(value).ok_or_else(|| {
        anyhow::anyhow!("{name} must be a positive duration with optional s/m/h/d suffix")
    })
}

fn parse_duration_seconds(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (number, multiplier) = match value.as_bytes().last().copied() {
        Some(b's' | b'S') => (&value[..value.len() - 1], 1_i64),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 60_i64),
        Some(b'h' | b'H') => (&value[..value.len() - 1], 60_i64 * 60),
        Some(b'd' | b'D') => (&value[..value.len() - 1], 24_i64 * 60 * 60),
        _ => (value, 1_i64),
    };
    let seconds = number.trim().parse::<i64>().ok()?.checked_mul(multiplier)?;
    (seconds > 0).then_some(seconds)
}

fn parse_bool_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_mode_requires_explicit_environment() {
        assert!(parse_runtime_mode(None).is_err());
        assert_eq!(
            parse_runtime_mode(Some("development")).unwrap(),
            RuntimeMode::Development
        );
        assert_eq!(
            parse_runtime_mode(Some("production")).unwrap(),
            RuntimeMode::Production
        );
        assert!(parse_runtime_mode(Some("staging")).is_err());
    }

    #[test]
    fn test_production_ttl_validation_rejects_unsafe_bounds() {
        assert!(validate_production_ttl_values(Some("30m"), Some("7d")).is_ok());
        assert!(validate_production_ttl_values(None, Some("7d")).is_err());
        assert!(validate_production_ttl_values(Some("30m"), None).is_err());
        assert!(validate_production_ttl_values(Some("bad"), Some("7d")).is_err());
        assert!(validate_production_ttl_values(Some("30m"), Some("bad")).is_err());
        assert!(validate_production_ttl_values(Some("24h"), Some("7d")).is_err());
        assert!(validate_production_ttl_values(Some("10s"), Some("7d")).is_err());
        assert!(validate_production_ttl_values(Some("30m"), Some("31d")).is_err());
        assert!(validate_production_ttl_values(Some("30m"), Some("30m")).is_err());
    }

    #[test]
    fn test_production_payment_validation_is_only_strong_config() {
        assert!(validate_production_payment_values(
            "0123456789abcdef0123456789abcdef",
            "xrayc-production-payment-address",
        )
        .is_ok());
        assert!(
            validate_production_payment_values("short", "xrayc-production-payment-address",)
                .is_err()
        );
        assert!(validate_production_payment_values(
            "change-me-0123456789abcdef0123456789abcdef",
            "xrayc-production-payment-address",
        )
        .is_err());
        assert!(validate_production_payment_values(
            "0123456789abcdef0123456789abcdef",
            DEFAULT_PAYMENT_RECEIVE_ADDRESS,
        )
        .is_err());
    }

    #[test]
    fn test_production_payment_enabled_defaults_false_and_requires_bool() {
        let _unset = EnvVarGuard::unset("XRAYC_PAYMENT_ENABLED");
        assert!(!production_payment_enabled().unwrap());

        let enabled = EnvVarGuard::set("XRAYC_PAYMENT_ENABLED", "true");
        assert!(production_payment_enabled().unwrap());
        drop(enabled);

        let invalid = EnvVarGuard::set("XRAYC_PAYMENT_ENABLED", "maybe");
        assert!(production_payment_enabled().is_err());
        drop(invalid);

        let _disabled = EnvVarGuard::set("XRAYC_PAYMENT_ENABLED", "false");
        assert!(!production_payment_enabled().unwrap());
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}
