use std::{collections::HashMap, sync::Mutex, time::UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, header};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::config::AuthConfig;

type HmacSha256 = Hmac<Sha256>;

const SESSION_COOKIE: &str = "starry_session";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SessionData {
    #[serde(default)]
    pub authenticated: bool,
    #[serde(default)]
    pub csrf_token: String,
    #[serde(default)]
    pub flash_message: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub oidc_state: Option<String>,
    #[serde(default)]
    pub oidc_nonce: Option<String>,
    #[serde(default)]
    pub oidc_verifier: Option<String>,
    #[serde(default)]
    pub oidc_next: Option<String>,
}

#[derive(Default)]
struct LoginAttempts {
    failed: HashMap<String, Vec<u64>>,
    lockouts: HashMap<String, u64>,
    last_cleanup: u64,
}

#[derive(Default)]
pub struct LoginLimiter {
    state: Mutex<LoginAttempts>,
}

pub fn now_seconds() -> u64 {
    UNIX_EPOCH.elapsed().unwrap_or_default().as_secs()
}

pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn is_safe_next_url(target: &str) -> bool {
    target.starts_with('/')
        && !target.starts_with("//")
        && !target.chars().any(char::is_control)
        && url::Url::parse(target).is_err()
}

pub fn csrf_matches(session: &SessionData, submitted: &str) -> bool {
    !session.csrf_token.is_empty()
        && !submitted.is_empty()
        && bool::from(session.csrf_token.as_bytes().ct_eq(submitted.as_bytes()))
}

pub fn is_valid_password(password_hash: &str, password: &str) -> bool {
    let Some((method, salt, digest)) = parse_password_hash(password_hash) else {
        return false;
    };
    let Ok(expected) = hex::decode(digest) else {
        return false;
    };
    if expected.is_empty() {
        return false;
    }

    let mut actual = vec![0; expected.len()];
    let result = if let Some(params) = method.strip_prefix("scrypt:") {
        verify_scrypt(params, salt.as_bytes(), password.as_bytes(), &mut actual)
    } else if let Some(params) = method.strip_prefix("pbkdf2:sha256:") {
        verify_pbkdf2(params, salt.as_bytes(), password.as_bytes(), &mut actual)
    } else {
        false
    };

    result && bool::from(actual.ct_eq(&expected))
}

fn parse_password_hash(value: &str) -> Option<(&str, &str, &str)> {
    let (method, rest) = value.split_once('$')?;
    let (salt, digest) = rest.split_once('$')?;
    if method.is_empty() || salt.is_empty() || digest.is_empty() {
        return None;
    }
    Some((method, salt, digest))
}

fn verify_scrypt(method: &str, salt: &[u8], password: &[u8], output: &mut [u8]) -> bool {
    let mut parts = method.split(':');
    let Some(n) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    let Some(r) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    let Some(p) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    if parts.next().is_some() || !n.is_power_of_two() || !(2..=20).contains(&n.trailing_zeros()) {
        return false;
    }
    let params = match scrypt::Params::new(n.trailing_zeros() as u8, r, p, output.len()) {
        Ok(params) => params,
        Err(_) => return false,
    };
    scrypt::scrypt(password, salt, &params, output).is_ok()
}

fn verify_pbkdf2(method: &str, salt: &[u8], password: &[u8], output: &mut [u8]) -> bool {
    let Ok(iterations) = method.parse::<u32>() else {
        return false;
    };
    if iterations == 0 {
        return false;
    }
    pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, iterations, output);
    true
}

pub fn load_session(headers: &HeaderMap, secret: &str) -> SessionData {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok());
    let Some(cookie) = cookie_header.and_then(find_session_cookie) else {
        return SessionData::default();
    };
    let Some((payload, signature)) = cookie.split_once('.') else {
        return SessionData::default();
    };
    let Ok(payload_bytes) = URL_SAFE_NO_PAD.decode(payload) else {
        return SessionData::default();
    };
    let Ok(signature_bytes) = URL_SAFE_NO_PAD.decode(signature) else {
        return SessionData::default();
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return SessionData::default();
    };
    mac.update(payload.as_bytes());
    if mac.verify_slice(&signature_bytes).is_err() {
        return SessionData::default();
    }
    let Ok(mut session) = serde_json::from_slice::<SessionData>(&payload_bytes) else {
        return SessionData::default();
    };
    if session
        .expires_at
        .is_some_and(|expires_at| expires_at <= now_seconds())
    {
        session = SessionData::default();
    }
    session
}

fn find_session_cookie(cookies: &str) -> Option<&str> {
    cookies.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == SESSION_COOKIE).then_some(value)
    })
}

pub fn attach_session_cookie(
    headers: &mut HeaderMap,
    session: &SessionData,
    auth: &AuthConfig,
) -> Result<(), String> {
    let payload = serde_json::to_vec(session).map_err(|error| error.to_string())?;
    let encoded = URL_SAFE_NO_PAD.encode(payload);
    let mut mac = HmacSha256::new_from_slice(auth.secret_key.as_bytes())
        .map_err(|error| error.to_string())?;
    mac.update(encoded.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    let mut cookie =
        format!("{SESSION_COOKIE}={encoded}.{signature}; Path=/; HttpOnly; SameSite=Lax");
    if auth.secure_cookie {
        cookie.push_str("; Secure");
    }
    if let Some(expires_at) = session.expires_at {
        cookie.push_str(&format!(
            "; Max-Age={}",
            expires_at.saturating_sub(now_seconds())
        ));
    }
    let header_value = HeaderValue::from_str(&cookie).map_err(|error| error.to_string())?;
    headers.append(header::SET_COOKIE, header_value);
    Ok(())
}

pub fn clear_session_cookie(headers: &mut HeaderMap, secure_cookie: bool) {
    let secure = if secure_cookie { "; Secure" } else { "" };
    if let Ok(value) = HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{secure}"
    )) {
        headers.append(header::SET_COOKIE, value);
    }
}

impl LoginLimiter {
    pub fn remaining_seconds(&self, ip: &str, auth: &AuthConfig) -> u64 {
        let now = now_seconds();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        cleanup_if_due(&mut state, now, auth);
        let until = state.lockouts.get(ip).copied().unwrap_or_default();
        if until <= now {
            state.lockouts.remove(ip);
            0
        } else {
            until - now
        }
    }

    pub fn register_failure(&self, ip: &str, auth: &AuthConfig) {
        let now = now_seconds();
        let threshold = now.saturating_sub(auth.login_window_seconds);
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        cleanup_if_due(&mut state, now, auth);
        let attempts = state.failed.entry(ip.to_owned()).or_default();
        attempts.retain(|timestamp| *timestamp >= threshold);
        attempts.push(now);
        if attempts.len() >= auth.login_max_attempts {
            state
                .lockouts
                .insert(ip.to_owned(), now + auth.login_lockout_seconds);
            state.failed.remove(ip);
        }
    }

    pub fn clear(&self, ip: &str) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.failed.remove(ip);
        state.lockouts.remove(ip);
    }
}

fn cleanup_if_due(state: &mut LoginAttempts, now: u64, auth: &AuthConfig) {
    if now.saturating_sub(state.last_cleanup) < auth.state_cleanup_interval_seconds {
        return;
    }
    let threshold = now.saturating_sub(auth.login_window_seconds);
    state.failed.retain(|_, timestamps| {
        timestamps.retain(|timestamp| *timestamp >= threshold);
        !timestamps.is_empty()
    });
    state.lockouts.retain(|_, until| *until > now);
    state.last_cleanup = now;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_url_rejects_external_and_protocol_relative_targets() {
        assert!(is_safe_next_url("/dashboard"));
        assert!(!is_safe_next_url("https://example.com"));
        assert!(!is_safe_next_url("//example.com"));
    }

    #[test]
    fn session_cookie_round_trips_and_rejects_tampering() {
        let auth = AuthConfig {
            secret_key: "test-secret".to_owned(),
            session_days: 30,
            secure_cookie: true,
            password_enabled: true,
            username: None,
            password_hash: None,
            oidc_enabled: false,
            oidc_provider_name: "OIDC".to_owned(),
            oidc_discovery_url: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            oidc_scope: Vec::new(),
            oidc_allowed_emails: Vec::new(),
            login_max_attempts: 5,
            login_window_seconds: 300,
            login_lockout_seconds: 900,
            state_cleanup_interval_seconds: 300,
        };
        let mut session = SessionData::default();
        session.authenticated = true;
        session.csrf_token = "token".to_owned();
        let mut response_headers = HeaderMap::new();
        attach_session_cookie(&mut response_headers, &session, &auth).unwrap();
        let mut request_headers = HeaderMap::new();
        request_headers.insert(header::COOKIE, response_headers[header::SET_COOKIE].clone());
        let parsed = load_session(&request_headers, &auth.secret_key);
        assert_eq!(parsed.csrf_token, "token");
        assert!(parsed.authenticated);
    }

    #[test]
    fn werkzeug_scrypt_hash_format_is_verified() {
        let salt = "0123456789abcdef";
        let params = scrypt::Params::new(15, 8, 1, 64).unwrap();
        let mut digest = [0u8; 64];
        scrypt::scrypt(b"password", salt.as_bytes(), &params, &mut digest).unwrap();
        let hash = format!("scrypt:32768:8:1${salt}${}", hex::encode(digest));

        assert!(is_valid_password(&hash, "password"));
        assert!(!is_valid_password(&hash, "wrong"));
    }
}
