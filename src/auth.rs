use std::time::{Duration, Instant};

use anyhow::Result;
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use chrono::Utc;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::state::AppState;

pub const SESSION_COOKIE: &str = "quill_session";
const SESSION_DAYS: i64 = 30;
pub const MIN_PASSWORD_LEN: usize = 10;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex(&buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A one-time code that must be entered to claim a fresh installation, so
/// nobody else who reaches the server first can create the admin account.
pub fn generate_setup_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut buf = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    let code: String = buf.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect();
    format!("{}-{}", &code[..4], &code[4..])
}

pub fn normalize_setup_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// Creates a session and returns the raw token for the cookie. Only a hash of
/// the token is stored.
pub async fn create_session(db: &SqlitePool, user_id: i64) -> Result<String> {
    let token = random_hex(32);
    let now = Utc::now();
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(now).execute(db).await?;
    sqlx::query("INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, expires_at) VALUES (?, ?, ?, ?, ?)")
        .bind(hash_token(&token))
        .bind(user_id)
        .bind(random_hex(32))
        .bind(now)
        .bind(now + chrono::Duration::days(SESSION_DAYS))
        .execute(db)
        .await?;
    Ok(token)
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(hash_token(token))
        .execute(db)
        .await?;
    Ok(())
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.to_string())
}

/// True when the browser reached us over HTTPS through a reverse proxy.
pub fn is_https(headers: &HeaderMap) -> bool {
    let forwarded_proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').next().unwrap_or("").trim().eq_ignore_ascii_case("https"));
    let forwarded = headers
        .get(header::FORWARDED)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("proto=https"));
    forwarded_proto.or(forwarded).unwrap_or(false)
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        SESSION_DAYS * 86_400,
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// The signed-in administrator. Handlers that take this extractor redirect
/// anonymous visitors to the login page.
#[derive(Debug, Clone)]
pub struct AdminUser {
    pub id: i64,
    pub username: String,
    pub csrf: String,
}

impl AdminUser {
    #[allow(clippy::result_large_err)] // the error is returned straight to the client
    pub fn check_csrf(&self, submitted: &str) -> Result<(), Response> {
        if constant_time_eq(&self.csrf, submitted) {
            Ok(())
        } else {
            Err((
                StatusCode::FORBIDDEN,
                "This form has expired or was submitted from another site. Go back, reload the page and try again.",
            )
                .into_response())
        }
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let base = state.settings().base_path;
        if state.needs_setup() {
            return Err(Redirect::to(&format!("{base}/admin/setup")).into_response());
        }
        let login = || Redirect::to(&format!("{base}/admin/login")).into_response();
        let Some(token) = session_token(&parts.headers) else { return Err(login()) };
        let row: Option<(i64, String, String)> = sqlx::query_as(
            "SELECT u.id, u.username, s.csrf_token FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(hash_token(&token))
        .bind(Utc::now())
        .fetch_optional(&state.db)
        .await
        .map_err(|e| crate::error::AppError::from(e).into_response())?;
        match row {
            Some((id, username, csrf)) => Ok(AdminUser { id, username, csrf }),
            None => Err(login()),
        }
    }
}

/// Throttles password guessing: after `MAX_FAILURES` failures for one key
/// within `WINDOW`, further attempts are refused until the window passes.
#[derive(Default)]
pub struct LoginLimiter {
    failures: std::sync::Mutex<std::collections::HashMap<String, Vec<Instant>>>,
}

const MAX_FAILURES: usize = 5;
const WINDOW: Duration = Duration::from_secs(15 * 60);

impl LoginLimiter {
    pub fn is_blocked(&self, key: &str) -> bool {
        let mut map = self.failures.lock().unwrap();
        let entry = map.entry(key.to_lowercase()).or_default();
        entry.retain(|t| t.elapsed() < WINDOW);
        entry.len() >= MAX_FAILURES
    }

    pub fn record_failure(&self, key: &str) {
        let mut map = self.failures.lock().unwrap();
        if map.len() > 10_000 {
            map.retain(|_, v| v.iter().any(|t| t.elapsed() < WINDOW));
        }
        map.entry(key.to_lowercase()).or_default().push(Instant::now());
    }

    pub fn clear(&self, key: &str) {
        self.failures.lock().unwrap().remove(&key.to_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn setup_code_normalizes() {
        let code = generate_setup_code();
        assert_eq!(code.len(), 9);
        assert_eq!(normalize_setup_code(&code.to_lowercase().replace('-', " ")), normalize_setup_code(&code));
    }

    #[test]
    fn limiter_blocks_after_failures() {
        let l = LoginLimiter::default();
        for _ in 0..MAX_FAILURES {
            assert!(!l.is_blocked("Admin"));
            l.record_failure("admin");
        }
        assert!(l.is_blocked("ADMIN"));
        l.clear("admin");
        assert!(!l.is_blocked("admin"));
    }
}
