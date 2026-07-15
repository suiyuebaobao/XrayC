//! 本模块负责从 Xray 访问日志推导用户会话。
//! 会话状态保存客户端 IP 和哈希，供管理员用户级流量日志追溯。
//! 控制面只在管理员页面展示明细，不在公开日志输出这些地址。

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::Context;
use sha2::{Digest, Sha256};
use tracing::warn;
use xrayc_xray_config::{parse_stats_user_email, AccessConfig};

use crate::client::AccessUserSession;
use crate::config::AgentSettings;

#[derive(Debug, Default)]
pub(super) struct SessionState {
    log_offset: u64,
    sessions: HashMap<String, SessionObservation>,
}

#[derive(Debug, Clone)]
struct SessionObservation {
    access_line_id: String,
    xray_user_key: String,
    client_ip: String,
    client_ip_hash: String,
    active_connection_count: u64,
    started_at_unix: i64,
    last_seen_at_unix: i64,
}

pub(super) fn collect_session_snapshot(
    settings: &AgentSettings,
    active_config: Option<&AccessConfig>,
    state: &mut SessionState,
    now_unix: i64,
) -> Vec<AccessUserSession> {
    let Some(config) = active_config else {
        state.sessions.clear();
        return Vec::new();
    };

    let email_lookup = access_log_email_lookup(config);
    match read_new_access_log_text(&settings.xray_access_log_path, &mut state.log_offset) {
        Ok(text) => {
            for line in text.lines() {
                if let Some(event) = parse_access_log_session_event(line, &email_lookup) {
                    let key = format!(
                        "{}\n{}\n{}",
                        event.access_line_id, event.xray_user_key, event.client_ip_hash
                    );
                    let entry = state.sessions.entry(key).or_insert(SessionObservation {
                        access_line_id: event.access_line_id,
                        xray_user_key: event.xray_user_key,
                        client_ip: event.client_ip,
                        client_ip_hash: event.client_ip_hash,
                        active_connection_count: 0,
                        started_at_unix: now_unix,
                        last_seen_at_unix: now_unix,
                    });
                    entry.active_connection_count = entry.active_connection_count.saturating_add(1);
                    entry.last_seen_at_unix = now_unix;
                }
            }
        }
        Err(error) => warn!(%error, "read xray access log failed"),
    }

    let idle_after = now_unix.saturating_sub(settings.session_idle_seconds as i64);
    state
        .sessions
        .retain(|_, session| session.last_seen_at_unix >= idle_after);

    state
        .sessions
        .values_mut()
        .map(|session| {
            let active_connection_count = session.active_connection_count.max(1);
            session.active_connection_count = 0;
            AccessUserSession {
                access_line_id: session.access_line_id.clone(),
                xray_user_key: session.xray_user_key.clone(),
                client_ip: session.client_ip.clone(),
                client_ip_hash: session.client_ip_hash.clone(),
                active_connection_count,
                started_at_unix: session.started_at_unix,
                last_seen_at_unix: session.last_seen_at_unix,
            }
        })
        .collect()
}

fn access_log_email_lookup(config: &AccessConfig) -> HashMap<String, (String, String)> {
    let mut lookup = HashMap::new();
    for line in &config.access_lines {
        for user in &line.users {
            let (access_line_id, xray_user_key) = parse_stats_user_email(&user.email)
                .unwrap_or_else(|| (line.report_line_id().to_owned(), user.xray_user_key.clone()));
            lookup.insert(user.email.clone(), (access_line_id, xray_user_key));
        }
    }
    lookup
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionLogEvent {
    access_line_id: String,
    xray_user_key: String,
    client_ip: String,
    client_ip_hash: String,
}

fn parse_access_log_session_event(
    line: &str,
    email_lookup: &HashMap<String, (String, String)>,
) -> Option<SessionLogEvent> {
    let email = extract_user_email_from_access_log(line)?;
    let (access_line_id, xray_user_key) = email_lookup.get(email)?;
    let client_ip = extract_client_ip_from_access_log(line)?;
    Some(SessionLogEvent {
        access_line_id: access_line_id.clone(),
        xray_user_key: xray_user_key.clone(),
        client_ip: client_ip.clone(),
        client_ip_hash: hash_client_ip(&client_ip),
    })
}

fn extract_user_email_from_access_log(line: &str) -> Option<&str> {
    extract_email_suffix_from_access_log(line)
        .or_else(|| extract_bracket_email_from_access_log(line))
}

fn extract_email_suffix_from_access_log(line: &str) -> Option<&str> {
    let (_, rest) = line.rsplit_once(" email:")?;
    rest.split_whitespace()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_bracket_email_from_access_log(line: &str) -> Option<&str> {
    let close = line.rfind(']')?;
    let open = line[..close].rfind('[')?;
    let email = line[open + 1..close].trim();
    Some(email).filter(|value| !value.is_empty())
}

fn read_new_access_log_text(path: &Path, offset: &mut u64) -> anyhow::Result<String> {
    if path.as_os_str().is_empty() || !path.exists() {
        return Ok(String::new());
    }
    let metadata = fs::metadata(path).with_context(|| format!("stat {:?}", path))?;
    let len = metadata.len();
    if len < *offset {
        *offset = 0;
    }
    let mut file = fs::File::open(path).with_context(|| format!("open {:?}", path))?;
    file.seek(SeekFrom::Start(*offset))
        .with_context(|| format!("seek {:?}", path))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .with_context(|| format!("read {:?}", path))?;
    *offset = len;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn extract_client_ip_from_access_log(line: &str) -> Option<String> {
    for marker in [" from tcp:", " from udp:", " from "] {
        if let Some(index) = line.find(marker) {
            let start = index + marker.len();
            if let Some(ip) = parse_host_port_token(&line[start..]) {
                return Some(ip);
            }
        }
    }
    None
}

fn parse_host_port_token(value: &str) -> Option<String> {
    let token = value
        .split_whitespace()
        .next()?
        .trim_matches(|ch| ch == ',' || ch == ';');
    if let Some(rest) = token.strip_prefix('[') {
        let (host, _) = rest.split_once(']')?;
        return Some(host.to_owned()).filter(|host| !host.is_empty());
    }
    let token = token
        .strip_prefix("tcp:")
        .or_else(|| token.strip_prefix("udp:"))
        .unwrap_or(token);
    let (host, _) = token.rsplit_once(':')?;
    Some(host.to_owned()).filter(|host| !host.is_empty())
}

fn hash_client_ip(ip: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ip.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}
