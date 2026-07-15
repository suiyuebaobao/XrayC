//! 本模块集中存放 API 共享状态与轻量限流器。
//! 这里只保留状态结构、认证响应策略和内存频控桶。
//! 不放路由注册或业务处理器，避免拆分时改变接口行为。
//! PostgreSQL 共享频控失败时仍由这里的内存限流器兜底。

use chrono::Duration;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, Instant};
use xrayc_core::{MemoryStore, TrafficService};
use xrayc_db::PgStore;

use crate::request_defaults::{
    env_bool, env_duration_seconds, env_requests_secure_cookie, DEFAULT_ACCESS_TOKEN_TTL_SECONDS,
    DEFAULT_REFRESH_TOKEN_TTL_SECONDS, REFRESH_COOKIE_SECURE_ENV,
};

pub(crate) const RATE_LIMIT_AUTH_PER_MINUTE: u32 = 30;
pub(crate) const RATE_LIMIT_SUBSCRIPTION_PER_MINUTE: u32 = 120;
pub(crate) const RATE_LIMIT_PAYMENT_PER_MINUTE: u32 = 60;

#[derive(Debug, Clone)]
pub struct AppState {
    pub store: MemoryStore,
    pub traffic: TrafficService,
    pub pg: Option<PgStore>,
    pub jwt_secret: String,
    pub access_token_ttl: Duration,
    pub refresh_token_ttl: Duration,
    pub refresh_cookie_secure: bool,
    pub(crate) rate_limiter: Arc<RateLimiter>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AuthResponsePolicy {
    pub(crate) access_token_ttl: Duration,
    pub(crate) refresh_token_ttl: Duration,
    pub(crate) refresh_cookie_secure: bool,
}

#[derive(Debug)]
pub(crate) struct RateLimiter {
    buckets: Mutex<HashMap<String, RateLimitBucket>>,
}

#[derive(Debug)]
struct RateLimitBucket {
    window_started_at: Instant,
    count: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RateLimitScope {
    Auth,
    Subscription,
    PaymentCallback,
}

impl Default for AppState {
    fn default() -> Self {
        let store = MemoryStore::seeded();
        let traffic = TrafficService::new(store.clone());
        Self {
            store,
            traffic,
            pg: None,
            jwt_secret: std::env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-jwt-secret-change-me".to_string()),
            access_token_ttl: Duration::seconds(env_duration_seconds(
                "JWT_EXPIRES_IN",
                DEFAULT_ACCESS_TOKEN_TTL_SECONDS,
            )),
            refresh_token_ttl: Duration::seconds(env_duration_seconds(
                "JWT_REFRESH_EXPIRES_IN",
                DEFAULT_REFRESH_TOKEN_TTL_SECONDS,
            )),
            refresh_cookie_secure: env_bool(REFRESH_COOKIE_SECURE_ENV)
                .unwrap_or_else(env_requests_secure_cookie),
            rate_limiter: Arc::new(RateLimiter::new()),
        }
    }
}

impl AppState {
    pub fn with_pg(pg: PgStore) -> Self {
        Self {
            pg: Some(pg),
            ..Self::default()
        }
    }

    pub(crate) fn auth_response_policy(&self) -> AuthResponsePolicy {
        AuthResponsePolicy {
            access_token_ttl: self.access_token_ttl,
            refresh_token_ttl: self.refresh_token_ttl,
            refresh_cookie_secure: self.refresh_cookie_secure,
        }
    }
}

impl RateLimiter {
    pub(crate) fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn allow(&self, scope: RateLimitScope, identity: &str) -> bool {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        if buckets.len() > 4096 {
            buckets.retain(|_, bucket| {
                now.duration_since(bucket.window_started_at) < StdDuration::from_secs(120)
            });
        }
        let key = format!("{}:{identity}", scope.name());
        let bucket = buckets.entry(key).or_insert(RateLimitBucket {
            window_started_at: now,
            count: 0,
        });
        if now.duration_since(bucket.window_started_at) >= scope.window() {
            bucket.window_started_at = now;
            bucket.count = 0;
        }
        if bucket.count >= scope.limit() {
            return false;
        }
        bucket.count += 1;
        true
    }
}

impl RateLimitScope {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Subscription => "subscription",
            Self::PaymentCallback => "payment_callback",
        }
    }

    pub(crate) fn limit(self) -> u32 {
        match self {
            Self::Auth => RATE_LIMIT_AUTH_PER_MINUTE,
            Self::Subscription => RATE_LIMIT_SUBSCRIPTION_PER_MINUTE,
            Self::PaymentCallback => RATE_LIMIT_PAYMENT_PER_MINUTE,
        }
    }

    pub(crate) fn window(self) -> StdDuration {
        StdDuration::from_secs(60)
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Auth => "认证请求过于频繁，请稍后再试",
            Self::Subscription => "订阅下载过于频繁，请稍后再试",
            Self::PaymentCallback => "支付回调过于频繁，请稍后再试",
        }
    }
}
