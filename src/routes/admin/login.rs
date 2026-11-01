//! Signing in and out.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Form;
use serde::Deserialize;
use serde_json::json;

use crate::auth::{self, AdminUser};
use crate::error::AppResult;
use crate::state::AppState;

use super::{admin_context, redirect, render, CsrfForm};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub async fn login_form(State(state): State<AppState>) -> AppResult<Response> {
    let s = state.settings();
    if state.needs_setup() {
        return Ok(redirect(&s, "/admin/setup"));
    }
    let mut ctx = admin_context(&s, None, "login");
    ctx.insert("username".into(), json!(""));
    Ok(render(&state, "login", &ctx)?.into_response())
}

pub async fn login_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let s = state.settings();
    if state.needs_setup() {
        return Ok(redirect(&s, "/admin/setup"));
    }
    let username = form.username.trim();
    let fail = |msg: &str| -> AppResult<Response> {
        let mut ctx = admin_context(&s, None, "login");
        ctx.insert("username".into(), json!(username));
        ctx.insert("error".into(), json!(msg));
        Ok((StatusCode::UNAUTHORIZED, render(&state, "login", &ctx)?).into_response())
    };
    if state.login_limiter.is_blocked(username) {
        return fail(
            "Too many failed attempts for this account. Wait 15 minutes and try again, or reset the password from the server with “quill reset-password”.",
        );
    }
    let user: Option<(i64, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&state.db)
            .await?;
    // Hash even for unknown users so response timing doesn't reveal which exist.
    let valid = match &user {
        Some((_, hash)) => auth::verify_password(&form.password, hash),
        None => {
            let _ = auth::verify_password(
                &form.password,
                "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            );
            false
        }
    };
    let Some((user_id, _)) = user.filter(|_| valid) else {
        state.login_limiter.record_failure(username);
        return fail("That username and password don't match.");
    };
    state.login_limiter.clear(username);
    let token = auth::create_session(&state.db, user_id).await?;
    let mut res = redirect(&s, "/admin");
    res.headers_mut().insert(
        header::SET_COOKIE,
        auth::session_cookie(&token, auth::is_https(&headers)).parse()?,
    );
    Ok(res)
}

pub async fn logout(
    State(state): State<AppState>,
    user: AdminUser,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> AppResult<Response> {
    if let Err(r) = user.check_csrf(&form.csrf) {
        return Ok(r);
    }
    if let Some(token) = auth::session_token(&headers) {
        auth::delete_session(&state.db, &token).await?;
    }
    let mut res = redirect(&state.settings(), "/admin/login");
    res.headers_mut()
        .insert(header::SET_COOKIE, auth::clear_session_cookie().parse()?);
    Ok(res)
}
