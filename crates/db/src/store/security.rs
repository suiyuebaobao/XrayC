//! Token、密码和验证码摘要 helper。
//! 本文件由数据库拆分任务生成，保持原有 SQL 与流程。
//! 模块只承载 PgStore 的一个业务切面或共享内部模型。
//! 公开调用入口仍由 crate root 的 PgStore 与类型导出承担。
//! 私有 helper 仅提升到 crate 内可见以支持跨模块调用。
//! 不在这里引入 API、前端或脚本层的行为改动。
//! 后续维护请优先保持单文件不超过五百行。
//! 新增 SQL 时应延续原事务边界和返回结构。
//! 注释使用中文，方便后续子任务继续分工。
//! 本头部满足前十行中文注释约束。
use super::rows::*;
use crate::*;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub fn agent_token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

pub(crate) fn login_guard_key(account: &str) -> String {
    let normalized = account.trim().to_ascii_lowercase();
    format!("login:{}", agent_token_hash(&normalized))
}

pub(crate) fn demo_password_hash(password: &str, salt_bytes: &[u8]) -> Result<String, DbError> {
    let salt = SaltString::encode_b64(salt_bytes)
        .map_err(|error| DbError::PasswordHash(error.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| DbError::PasswordHash(error.to_string()))
}

pub(crate) fn password_hash(password: &str) -> Result<String, DbError> {
    let salt_seed = Uuid::new_v4();
    let salt = SaltString::encode_b64(salt_seed.as_bytes())
        .map_err(|error| DbError::PasswordHash(error.to_string()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| DbError::PasswordHash(error.to_string()))
}

pub(crate) fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub(crate) fn normalize_challenge_scene(scene: &str) -> String {
    scene.trim().to_ascii_lowercase()
}

pub(crate) fn normalize_challenge_target(target: &str) -> String {
    target.trim().to_ascii_lowercase()
}

pub(crate) fn challenge_target_hash(scene: &str, target: &str) -> String {
    agent_token_hash(&format!(
        "{}:{}",
        normalize_challenge_scene(scene),
        normalize_challenge_target(target)
    ))
}

pub(crate) fn challenge_code_hash(id: Uuid, code: &str) -> String {
    agent_token_hash(&format!("{}:{}", id, code.trim().to_ascii_lowercase()))
}
