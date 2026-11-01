//! The first-run wizard that creates the administrator account.

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;

use crate::auth;
use crate::error::AppResult;
use crate::routes::{request_origin, OriginalPath};
use crate::settings::{normalize_base_url, Settings};
use crate::state::AppState;
use crate::themes;

use super::{admin_context, redirect, render, Ctx};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct SetupForm {
    setup_code: String,
    site_name: String,
    base_url: String,
    username: String,
    password: String,
    password_confirm: String,
    theme: String,
}

fn setup_context(s: &Settings, form: &SetupForm, errors: &[String]) -> Ctx {
    let mut ctx = admin_context(s, None, "setup");
    ctx.insert(
        "form".into(),
        json!({"site_name": form.site_name, "base_url": form.base_url, "username": form.username, "theme": form.theme, "setup_code": form.setup_code}),
    );
    ctx.insert("themes".into(), json!(themes::ALL));
    ctx.insert("errors".into(), json!(errors));
    ctx.insert("min_password".into(), json!(auth::MIN_PASSWORD_LEN));
    ctx
}

pub async fn setup_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Extension(original): Extension<OriginalPath>,
) -> AppResult<Response> {
    let s = state.settings();
    if !state.needs_setup() {
        return Ok(redirect(&s, "/admin/login"));
    }
    // Guess the public URL from how the browser reached us.
    let prefix = original.0.strip_suffix("/admin/setup").unwrap_or("");
    let guess = request_origin(&headers)
        .map(|o| format!("{o}{prefix}"))
        .unwrap_or_default();
    let form = SetupForm {
        site_name: "My Blog".into(),
        base_url: guess,
        theme: themes::DEFAULT.into(),
        ..Default::default()
    };
    Ok(render(&state, "setup", &setup_context(&s, &form, &[]))?.into_response())
}

pub async fn setup_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SetupForm>,
) -> AppResult<Response> {
    let mut s = state.settings();
    let Some(code) = state.setup_code() else {
        return Ok(redirect(&s, "/admin/login"));
    };
    let mut errors = Vec::new();
    if state.login_limiter.is_blocked("setup") {
        errors.push("Too many incorrect setup codes. Wait 15 minutes and try again.".to_string());
    } else if auth::normalize_setup_code(&form.setup_code) != auth::normalize_setup_code(&code) {
        state.login_limiter.record_failure("setup");
        errors.push("The setup code doesn't match. Copy it from the server's startup output (for Docker: docker logs <container>).".into());
    }
    if form.site_name.trim().is_empty() {
        errors.push("Give your blog a name. You can change it later.".into());
    }
    let base_url = normalize_base_url(&form.base_url)
        .map_err(|e| errors.push(e))
        .ok();
    let username = form.username.trim();
    if username.is_empty()
        || username.len() > 64
        || !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-@".contains(c))
    {
        errors.push("Choose a username of letters, numbers, dots, dashes or underscores.".into());
    }
    if form.password.chars().count() < auth::MIN_PASSWORD_LEN {
        errors.push(format!(
            "Use a password of at least {} characters.",
            auth::MIN_PASSWORD_LEN
        ));
    } else if form.password != form.password_confirm {
        errors.push("The two passwords don't match.".into());
    }
    let theme = themes::find(&form.theme)
        .map(|t| t.id)
        .unwrap_or(themes::DEFAULT);
    if !errors.is_empty() {
        let ctx = setup_context(&s, &form, &errors);
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            render(&state, "setup", &ctx)?,
        )
            .into_response());
    }

    let hash = auth::hash_password(&form.password)?;
    let mut tx = state.db.begin().await?;
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx)
        .await?;
    if existing > 0 {
        state.finish_setup();
        return Ok(redirect(&s, "/admin/login"));
    }
    let user_id: i64 = sqlx::query_scalar("INSERT INTO users (username, password_hash, role, created_at) VALUES (?, ?, 'admin', ?) RETURNING id")
        .bind(username)
        .bind(hash)
        .bind(Utc::now())
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;

    s.site_name = form.site_name.trim().to_string();
    s.set_base_url(&base_url.unwrap_or_default());
    s.theme = theme.to_string();
    s.author_name = username.to_string();
    state.save_settings(s.clone()).await?;
    state.finish_setup();
    state.login_limiter.clear("setup");
    tracing::info!("setup complete; administrator “{username}” created");

    let token = auth::create_session(&state.db, user_id).await?;
    let mut res = redirect(&s, "/admin?notice=welcome");
    res.headers_mut().insert(
        header::SET_COOKIE,
        auth::session_cookie(&token, auth::is_https(&headers)).parse()?,
    );
    Ok(res)
}
